use hound::{SampleFormat, WavReader};
use recordit::capture_api::{
    CaptureChunk, CaptureEvent, CaptureSink, capture_telemetry_path_for_output,
};
use recordit::live_asr_pool::{
    LiveAsrExecutor, LiveAsrJob, LiveAsrJobClass, LiveAsrJobResult, LiveAsrPoolConfig,
    LiveAsrPoolTelemetry, LiveAsrService, TempAudioPolicy,
};
use recordit::live_capture::{
    CallbackContractMode as LiveCaptureCallbackMode, LiveCaptureConfig,
    SampleRateMismatchPolicy as LiveCaptureSampleRateMismatchPolicy, run_capture_session,
    run_streaming_capture_session,
};
use recordit::live_stream_runtime::{
    LiveAsrJobClass as RuntimeAsrJobClass, LiveAsrJobSpec as RuntimeAsrJobSpec, LiveAsrResult,
    LiveRuntimePhase, LiveRuntimeSummary, LiveStreamCoordinator, RuntimeFinalizer,
    RuntimeOutputEvent, RuntimeOutputSink, StreamingSchedulerConfig, StreamingVadConfig,
    StreamingVadScheduler,
};
use screencapturekit::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::env;
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

mod artifacts;
mod asr_backend;
use asr_backend::*;
mod cli_parse;
mod runtime_events;
mod runtime_live_stream;
mod runtime_representative;
mod transcript_flow;

const PARTIAL_LATENCY_SLO_MS: f64 = 1_500.0;
const FINAL_LATENCY_SLO_MS: f64 = 2_500.0;
const OVERLAP_WINDOW_MS: u64 = 120;
const OUT_WAV_SEMANTICS: &str = "canonical session WAV artifact for the run";
const DEFAULT_CHUNK_WINDOW_MS: u64 = 2_000;
const DEFAULT_CHUNK_STRIDE_MS: u64 = 500;
const DEFAULT_CHUNK_QUEUE_CAP: usize = 4;
const DEFAULT_LIVE_ASR_WORKERS: usize = 2;
const JSONL_SYNC_EVERY_LINES: usize = 24;
const LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE: &str = "live_capture_interruption_recovered";
const LIVE_CAPTURE_CONTINUITY_UNVERIFIED_CODE: &str = "live_capture_continuity_unverified";
const LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE: &str = "live_capture_transport_degraded";
const LIVE_CAPTURE_CALLBACK_CONTRACT_DEGRADED_CODE: &str =
    "live_capture_callback_contract_degraded";
const LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE: &str = "live_chunk_queue_drop_oldest";
const LIVE_CHUNK_QUEUE_BACKPRESSURE_SEVERE_CODE: &str = "live_chunk_queue_backpressure_severe";
const RECONCILIATION_APPLIED_CODE: &str = "reconciliation_applied_after_backpressure";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModelChecksumCacheKey {
    canonical_path: PathBuf,
    file_len: u64,
    modified_unix_nanos: u128,
}

const LIVE_CAPTURE_TRANSPORT_SOURCES: [&str; 4] = [
    "slot_miss_drops",
    "fill_failures",
    "queue_full_drops",
    "recycle_failures",
];

const LIVE_CAPTURE_CALLBACK_SOURCES: [&str; 6] = [
    "missing_audio_buffer_list",
    "missing_first_audio_buffer",
    "missing_format_description",
    "missing_sample_rate",
    "non_float_pcm",
    "chunk_too_large",
];

const HELP_TEXT: &str = "\
transcribe-live

Define and validate the live transcription CLI contract for the next phase of recordit.

Migration guidance:
  For normal operator usage, prefer `recordit run --mode live` (or `recordit run --mode offline`).
  `transcribe-live` remains stable for legacy scripts, gates, and expert workflows.

Usage:
  transcribe-live [options]

Options:
  --duration-sec <seconds>        Capture duration in seconds (default: 10; with --live-stream, 0 means run until interrupted)
  --input-wav <path>              Runtime WAV path (offline: representative fixture path; live-stream: progressive capture scratch; default: artifacts/bench/corpus/gate_a/tts_phrase.wav)
  --out-wav <path>                Canonical session WAV artifact path (materialized on successful runtime completion; default: artifacts/transcribe-live.wav)
  --out-jsonl <path>              Output JSONL transcript path (default: artifacts/transcribe-live.jsonl)
  --out-manifest <path>           Output session manifest path (default: artifacts/transcribe-live.manifest.json)
  --sample-rate <hz>              Capture sample rate in Hz (default: 48000)
  --asr-backend <backend>         ASR backend: whispercpp | whisperkit | moonshine (default: whispercpp)
  --asr-model <path>              Local model path for the selected backend
  --asr-language <code>           Language code (default: en)
  --asr-threads <n>               ASR worker thread count (default: 4)
  --asr-profile <profile>         ASR profile: fast | balanced | quality (default: balanced)
  --vad-backend <backend>         VAD backend: webrtc | silero (default: silero)
  --vad-threshold <float>         VAD threshold in [0.0, 1.0] (default: 0.50)
  --vad-min-speech-ms <ms>        Minimum speech duration before emit (default: 250)
  --vad-min-silence-ms <ms>       Minimum silence duration before finalize (default: 500)
  --llm-cleanup                   Enable finalized-segment cleanup
  --llm-endpoint <url>            Local cleanup endpoint URL
  --llm-model <id>                Local cleanup model id
  --llm-timeout-ms <ms>           Cleanup timeout in milliseconds (default: 1000)
  --llm-max-queue <n>             Max queued cleanup requests (default: 32)
  --llm-retries <n>               Retry count for failed cleanup requests (default: 0)
  --live-chunked                  Select representative-chunked mode (runtime label remains live-chunked)
  --live-stream                   Select live-stream runtime entrypoint (cannot combine with --live-chunked)
  --chunk-window-ms <ms>          Near-live chunk window in milliseconds (default: 2000; requires --live-chunked)
  --chunk-stride-ms <ms>          Near-live chunk stride in milliseconds (default: 500; requires --live-chunked)
  --chunk-queue-cap <n>           Bounded near-live ASR work queue capacity (default: 4; requires --live-chunked)
  --live-asr-workers <n>          Dedicated live ASR worker pool concurrency (default: 2; live modes only)
  --keep-temp-audio               Retain live temp audio shards/probe WAVs for debug inspection
  --transcribe-channels <mode>    Channel mode: separate | mixed | mixed-fallback (default: separate)
  --speaker-labels <mic,system>   Comma-separated labels for the two channels (default: mic,system)
  --benchmark-runs <n>            Number of representative latency benchmark runs (default: 3)
  --model-doctor                  Run model/backend diagnostics and exit
  --replay-jsonl <path>           Replay transcript timeline from a prior JSONL artifact
  --preflight                     Run structured preflight diagnostics and write manifest
  -h, --help                      Show this help text

Runtime mode taxonomy:
  representative-offline          Default runtime mode (no live selector flags)
  representative-chunked          Selected by --live-chunked; runtime_mode remains live-chunked for artifact compatibility
  live-stream                     Implemented runtime entrypoint for dedicated live-stream execution

Examples:
  transcribe-live --asr-model artifacts/bench/models/whispercpp/ggml-tiny.en.bin
  transcribe-live --asr-backend whisperkit --asr-model artifacts/bench/models/whisperkit/models/argmaxinc/whisperkit-coreml/openai_whisper-tiny --transcribe-channels mixed
  transcribe-live --input-wav artifacts/bench/corpus/gate_a/tts_phrase.wav --asr-model artifacts/bench/models/whispercpp/ggml-tiny.en.bin --llm-cleanup --llm-endpoint http://127.0.0.1:8080/v1/chat/completions --llm-model llama3.2:3b
  transcribe-live --live-chunked --chunk-window-ms 2000 --chunk-stride-ms 500 --chunk-queue-cap 4 --asr-model artifacts/bench/models/whispercpp/ggml-tiny.en.bin
  transcribe-live --model-doctor --asr-backend whispercpp
  transcribe-live --replay-jsonl artifacts/transcribe-live.runtime.jsonl
  transcribe-live --preflight --asr-model models/ggml-base.en.bin
";

#[derive(Debug, Clone, Copy)]
enum AsrBackend {
    WhisperCpp,
    WhisperKit,
    Moonshine,
}

impl Display for AsrBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WhisperCpp => f.write_str("whispercpp"),
            Self::WhisperKit => f.write_str("whisperkit"),
            Self::Moonshine => f.write_str("moonshine"),
        }
    }
}

impl AsrBackend {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "whispercpp" | "whisper-rs" => Ok(Self::WhisperCpp),
            "whisperkit" => Ok(Self::WhisperKit),
            "moonshine" => Ok(Self::Moonshine),
            _ => Err(CliError::new(format!(
                "unsupported --asr-backend `{value}`; expected `whispercpp`, `whisperkit`, or `moonshine`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AsrProfile {
    Fast,
    Balanced,
    Quality,
}

impl Display for AsrProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fast => f.write_str("fast"),
            Self::Balanced => f.write_str("balanced"),
            Self::Quality => f.write_str("quality"),
        }
    }
}

impl AsrProfile {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "fast" => Ok(Self::Fast),
            "balanced" => Ok(Self::Balanced),
            "quality" => Ok(Self::Quality),
            _ => Err(CliError::new(format!(
                "unsupported --asr-profile `{value}`; expected `fast`, `balanced`, or `quality`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum VadBackend {
    Webrtc,
    Silero,
}

impl Display for VadBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Webrtc => f.write_str("webrtc"),
            Self::Silero => f.write_str("silero"),
        }
    }
}

impl VadBackend {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "webrtc" => Ok(Self::Webrtc),
            "silero" => Ok(Self::Silero),
            _ => Err(CliError::new(format!(
                "unsupported --vad-backend `{value}`; expected `webrtc` or `silero`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelMode {
    Separate,
    Mixed,
    MixedFallback,
}

impl Display for ChannelMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Separate => f.write_str("separate"),
            Self::Mixed => f.write_str("mixed"),
            Self::MixedFallback => f.write_str("mixed-fallback"),
        }
    }
}

impl ChannelMode {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "separate" => Ok(Self::Separate),
            "mixed" => Ok(Self::Mixed),
            "mixed-fallback" => Ok(Self::MixedFallback),
            _ => Err(CliError::new(format!(
                "unsupported --transcribe-channels `{value}`; expected `separate`, `mixed`, or `mixed-fallback`"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
struct SpeakerLabels {
    mic: String,
    system: String,
}

impl Display for SpeakerLabels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.mic, self.system)
    }
}

impl SpeakerLabels {
    fn parse(value: &str) -> Result<Self, CliError> {
        let mut parts = value.split(',').map(str::trim);
        let mic = parts
            .next()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| CliError::new("`--speaker-labels` requires two non-empty labels"))?;
        let system = parts
            .next()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| CliError::new("`--speaker-labels` requires two non-empty labels"))?;

        if parts.next().is_some() {
            return Err(CliError::new(
                "`--speaker-labels` accepts exactly two comma-separated labels",
            ));
        }

        Ok(Self {
            mic: mic.to_owned(),
            system: system.to_owned(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct RuntimeModeCompatibility {
    runtime_mode: &'static str,
    taxonomy_mode: &'static str,
    selector: &'static str,
    status: &'static str,
    replay_jsonl_compat: &'static str,
    preflight_compat: &'static str,
    chunk_tuning_compat: &'static str,
}

const REPRESENTATIVE_OFFLINE_COMPATIBILITY: RuntimeModeCompatibility = RuntimeModeCompatibility {
    runtime_mode: "representative-offline",
    taxonomy_mode: "representative-offline",
    selector: "<default>",
    status: "implemented",
    replay_jsonl_compat: "compatible",
    preflight_compat: "compatible",
    chunk_tuning_compat: "forbidden",
};

const REPRESENTATIVE_CHUNKED_COMPATIBILITY: RuntimeModeCompatibility = RuntimeModeCompatibility {
    runtime_mode: "live-chunked",
    taxonomy_mode: "representative-chunked",
    selector: "--live-chunked",
    status: "implemented",
    replay_jsonl_compat: "incompatible",
    preflight_compat: "incompatible",
    chunk_tuning_compat: "compatible",
};

const LIVE_STREAM_COMPATIBILITY: RuntimeModeCompatibility = RuntimeModeCompatibility {
    runtime_mode: "live-stream",
    taxonomy_mode: "live-stream",
    selector: "--live-stream",
    status: "implemented",
    replay_jsonl_compat: "incompatible",
    preflight_compat: "incompatible",
    chunk_tuning_compat: "compatible",
};

#[cfg(test)]
fn runtime_mode_compatibility_matrix() -> &'static [RuntimeModeCompatibility; 3] {
    &[
        REPRESENTATIVE_OFFLINE_COMPATIBILITY,
        REPRESENTATIVE_CHUNKED_COMPATIBILITY,
        LIVE_STREAM_COMPATIBILITY,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeExecutionBranch {
    RepresentativeOffline,
    RepresentativeChunked,
    LiveStream,
}

#[derive(Debug, Clone)]
struct TranscribeConfig {
    duration_sec: u64,
    input_wav: PathBuf,
    out_wav: PathBuf,
    out_jsonl: PathBuf,
    out_manifest: PathBuf,
    sample_rate_hz: u32,
    asr_backend: AsrBackend,
    asr_model: PathBuf,
    asr_language: String,
    asr_threads: usize,
    asr_profile: AsrProfile,
    vad_backend: VadBackend,
    vad_threshold: f32,
    vad_min_speech_ms: u32,
    vad_min_silence_ms: u32,
    llm_cleanup: bool,
    llm_endpoint: Option<String>,
    llm_model: Option<String>,
    llm_timeout_ms: u64,
    llm_max_queue: usize,
    llm_retries: usize,
    live_chunked: bool,
    live_stream: bool,
    chunk_window_ms: u64,
    chunk_stride_ms: u64,
    chunk_queue_cap: usize,
    live_asr_workers: usize,
    keep_temp_audio: bool,
    channel_mode: ChannelMode,
    speaker_labels: SpeakerLabels,
    benchmark_runs: usize,
    model_doctor: bool,
    replay_jsonl: Option<PathBuf>,
    preflight: bool,
}

impl Default for TranscribeConfig {
    fn default() -> Self {
        Self {
            duration_sec: 10,
            input_wav: PathBuf::from("artifacts/bench/corpus/gate_a/tts_phrase.wav"),
            out_wav: PathBuf::from("artifacts/transcribe-live.wav"),
            out_jsonl: PathBuf::from("artifacts/transcribe-live.jsonl"),
            out_manifest: PathBuf::from("artifacts/transcribe-live.manifest.json"),
            sample_rate_hz: 48_000,
            asr_backend: AsrBackend::WhisperCpp,
            asr_model: PathBuf::new(),
            asr_language: "en".to_owned(),
            asr_threads: 4,
            asr_profile: AsrProfile::Balanced,
            vad_backend: VadBackend::Silero,
            vad_threshold: 0.50,
            vad_min_speech_ms: 250,
            vad_min_silence_ms: 500,
            llm_cleanup: false,
            llm_endpoint: None,
            llm_model: None,
            llm_timeout_ms: 1_000,
            llm_max_queue: 32,
            llm_retries: 0,
            live_chunked: false,
            live_stream: false,
            chunk_window_ms: DEFAULT_CHUNK_WINDOW_MS,
            chunk_stride_ms: DEFAULT_CHUNK_STRIDE_MS,
            chunk_queue_cap: DEFAULT_CHUNK_QUEUE_CAP,
            live_asr_workers: DEFAULT_LIVE_ASR_WORKERS,
            keep_temp_audio: false,
            channel_mode: ChannelMode::Separate,
            speaker_labels: SpeakerLabels {
                mic: "mic".to_owned(),
                system: "system".to_owned(),
            },
            benchmark_runs: 3,
            model_doctor: false,
            replay_jsonl: None,
            preflight: false,
        }
    }
}

impl TranscribeConfig {
    fn validate(&self) -> Result<(), CliError> {
        if self.duration_sec == 0 && !self.live_stream {
            return Err(CliError::new(
                "`--duration-sec` must be greater than zero unless `--live-stream` is enabled",
            ));
        }

        if self.input_wav.as_os_str().is_empty() {
            return Err(CliError::new("`--input-wav` cannot be empty"));
        }

        if self.sample_rate_hz == 0 {
            return Err(CliError::new("`--sample-rate` must be greater than zero"));
        }

        if self.asr_threads == 0 {
            return Err(CliError::new("`--asr-threads` must be greater than zero"));
        }

        if !self.vad_threshold.is_finite() || !(0.0..=1.0).contains(&self.vad_threshold) {
            return Err(CliError::new(
                "`--vad-threshold` must be a finite value in [0.0, 1.0]",
            ));
        }

        if self.vad_min_speech_ms == 0 {
            return Err(CliError::new(
                "`--vad-min-speech-ms` must be greater than zero",
            ));
        }

        if self.vad_min_silence_ms == 0 {
            return Err(CliError::new(
                "`--vad-min-silence-ms` must be greater than zero",
            ));
        }

        if self.llm_timeout_ms == 0 {
            return Err(CliError::new(
                "`--llm-timeout-ms` must be greater than zero",
            ));
        }

        if self.llm_max_queue == 0 {
            return Err(CliError::new("`--llm-max-queue` must be greater than zero"));
        }

        if self.chunk_window_ms == 0 {
            return Err(CliError::new(
                "`--chunk-window-ms` must be greater than zero",
            ));
        }

        if self.chunk_stride_ms == 0 {
            return Err(CliError::new(
                "`--chunk-stride-ms` must be greater than zero",
            ));
        }

        if self.chunk_stride_ms > self.chunk_window_ms {
            return Err(CliError::new(
                "`--chunk-stride-ms` must be less than or equal to `--chunk-window-ms`; keep the stride inside the rolling window",
            ));
        }

        if self.chunk_queue_cap == 0 {
            return Err(CliError::new(
                "`--chunk-queue-cap` must be greater than zero",
            ));
        }

        if self.live_asr_workers == 0 {
            return Err(CliError::new(
                "`--live-asr-workers` must be greater than zero",
            ));
        }

        if self.benchmark_runs == 0 {
            return Err(CliError::new(
                "`--benchmark-runs` must be greater than zero",
            ));
        }

        if self.llm_cleanup {
            if self.llm_endpoint.as_deref().unwrap_or("").is_empty() {
                return Err(CliError::new(
                    "`--llm-endpoint <url>` is required when `--llm-cleanup` is enabled",
                ));
            }
            if self.llm_model.as_deref().unwrap_or("").is_empty() {
                return Err(CliError::new(
                    "`--llm-model <id>` is required when `--llm-cleanup` is enabled",
                ));
            }
        }

        self.validate_runtime_mode_compatibility()?;

        if self.model_doctor && self.replay_jsonl.is_some() {
            return Err(CliError::new(
                "`--model-doctor` cannot be combined with `--replay-jsonl`; doctor validates runtime prerequisites while replay consumes existing artifacts",
            ));
        }

        if self.model_doctor && self.preflight {
            return Err(CliError::new(
                "`--model-doctor` cannot be combined with `--preflight`; run either diagnostics mode separately",
            ));
        }

        validate_output_path("--out-wav", &self.out_wav)?;
        validate_output_path("--out-jsonl", &self.out_jsonl)?;
        validate_output_path("--out-manifest", &self.out_manifest)?;

        Ok(())
    }

    fn active_runtime_mode_compatibility(&self) -> &'static RuntimeModeCompatibility {
        if self.live_stream {
            &LIVE_STREAM_COMPATIBILITY
        } else if self.live_chunked {
            &REPRESENTATIVE_CHUNKED_COMPATIBILITY
        } else {
            &REPRESENTATIVE_OFFLINE_COMPATIBILITY
        }
    }

    fn runtime_mode_label(&self) -> &'static str {
        self.active_runtime_mode_compatibility().runtime_mode
    }

    fn runtime_mode_taxonomy_label(&self) -> &'static str {
        self.active_runtime_mode_compatibility().taxonomy_mode
    }

    fn runtime_mode_selector_label(&self) -> &'static str {
        self.active_runtime_mode_compatibility().selector
    }

    fn runtime_mode_status_label(&self) -> &'static str {
        self.active_runtime_mode_compatibility().status
    }

    fn validate_runtime_mode_compatibility(&self) -> Result<(), CliError> {
        if self.live_stream && self.live_chunked {
            return Err(CliError::new(
                "`--live-stream` cannot be combined with `--live-chunked`; choose exactly one live runtime selector",
            ));
        }

        if !self.live_chunked
            && !self.live_stream
            && (self.chunk_window_ms != DEFAULT_CHUNK_WINDOW_MS
                || self.chunk_stride_ms != DEFAULT_CHUNK_STRIDE_MS
                || self.chunk_queue_cap != DEFAULT_CHUNK_QUEUE_CAP)
        {
            return Err(CliError::new(
                "`--chunk-window-ms`, `--chunk-stride-ms`, and `--chunk-queue-cap` require `--live-chunked` or `--live-stream`; omit them for representative offline runs",
            ));
        }

        if self.live_chunked && self.replay_jsonl.is_some() {
            return Err(CliError::new(
                "`--live-chunked` cannot be combined with `--replay-jsonl`; replay reads a completed artifact instead of configuring a live runtime",
            ));
        }
        if self.live_stream && self.replay_jsonl.is_some() {
            return Err(CliError::new(
                "`--live-stream` cannot be combined with `--replay-jsonl`; replay reads a completed artifact instead of configuring a live runtime",
            ));
        }

        Ok(())
    }

    fn live_mode_summary_lines(&self) -> Vec<String> {
        let compatibility = self.active_runtime_mode_compatibility();
        let inactive_suffix = if self.live_chunked || self.live_stream {
            ""
        } else {
            " (inactive; enable with --live-chunked)"
        };
        vec![
            format!("  runtime_mode: {}", self.runtime_mode_label()),
            format!("  runtime_mode_taxonomy: {}", compatibility.taxonomy_mode),
            format!("  runtime_mode_selector: {}", compatibility.selector),
            format!("  runtime_mode_status: {}", compatibility.status),
            format!(
                "  runtime_mode_replay_jsonl: {}",
                compatibility.replay_jsonl_compat
            ),
            format!(
                "  runtime_mode_preflight: {}",
                compatibility.preflight_compat
            ),
            format!(
                "  runtime_mode_chunk_tuning: {}",
                compatibility.chunk_tuning_compat
            ),
            format!("  live_chunked_flag: {}", self.live_chunked),
            format!("  live_stream_flag: {}", self.live_stream),
            format!(
                "  chunk_window_ms: {}{}",
                self.chunk_window_ms, inactive_suffix
            ),
            format!(
                "  chunk_stride_ms: {}{}",
                self.chunk_stride_ms, inactive_suffix
            ),
            format!(
                "  chunk_queue_cap: {}{}",
                self.chunk_queue_cap, inactive_suffix
            ),
            format!(
                "  live_asr_workers: {}{}",
                self.live_asr_workers, inactive_suffix
            ),
            format!("  keep_temp_audio: {}", self.keep_temp_audio),
        ]
    }

    fn print_summary(&self, concise_only: bool) {
        println!("Startup banner");
        for line in build_startup_banner_lines(self) {
            println!("  {line}");
        }
        if concise_only {
            return;
        }

        println!();
        println!("Transcribe-live configuration");
        println!("  status: contract validated + runtime entrypoint enabled");
        println!("  duration_sec: {}", self.duration_sec);
        println!("  input_wav: {}", display_path(&self.input_wav));
        println!("  sample_rate_hz: {}", self.sample_rate_hz);
        println!("  out_wav: {}", display_path(&self.out_wav));
        println!("  out_jsonl: {}", display_path(&self.out_jsonl));
        println!("  out_manifest: {}", display_path(&self.out_manifest));
        println!("  asr_backend: {}", self.asr_backend);
        println!(
            "  asr_model: {}",
            if self.asr_model.as_os_str().is_empty() {
                "<auto-discover>".to_string()
            } else {
                display_path(&self.asr_model)
            }
        );
        println!("  asr_model_resolution: --asr-model > RECORDIT_ASR_MODEL > backend defaults");
        println!("  asr_language: {}", self.asr_language);
        println!("  asr_threads: {}", self.asr_threads);
        println!("  asr_profile: {}", self.asr_profile);
        println!("  vad_backend: {}", self.vad_backend);
        println!("  vad_threshold: {:.2}", self.vad_threshold);
        println!("  vad_min_speech_ms: {}", self.vad_min_speech_ms);
        println!("  vad_min_silence_ms: {}", self.vad_min_silence_ms);
        println!("  llm_cleanup: {}", self.llm_cleanup);
        println!(
            "  llm_endpoint: {}",
            self.llm_endpoint.as_deref().unwrap_or("<disabled>")
        );
        println!(
            "  llm_model: {}",
            self.llm_model.as_deref().unwrap_or("<disabled>")
        );
        println!("  llm_timeout_ms: {}", self.llm_timeout_ms);
        println!("  llm_max_queue: {}", self.llm_max_queue);
        println!("  llm_retries: {}", self.llm_retries);
        for line in self.live_mode_summary_lines() {
            println!("{line}");
        }
        println!("  transcribe_channels: {}", self.channel_mode);
        println!("  speaker_labels: {}", self.speaker_labels);
        println!("  benchmark_runs: {}", self.benchmark_runs);
        println!("  model_doctor: {}", self.model_doctor);
        println!(
            "  replay_jsonl: {}",
            self.replay_jsonl
                .as_ref()
                .map(|path| display_path(path))
                .unwrap_or_else(|| "<disabled>".to_string())
        );
        println!("  preflight: {}", self.preflight);
    }
}

#[derive(Debug, Clone, Copy)]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass => f.write_str("PASS"),
            Self::Warn => f.write_str("WARN"),
            Self::Fail => f.write_str("FAIL"),
        }
    }
}

#[derive(Debug, Clone)]
struct PreflightCheck {
    id: &'static str,
    status: CheckStatus,
    detail: String,
    remediation: Option<String>,
}

impl PreflightCheck {
    fn pass(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            status: CheckStatus::Pass,
            detail: detail.into(),
            remediation: None,
        }
    }

    fn warn(id: &'static str, detail: impl Into<String>, remediation: impl Into<String>) -> Self {
        Self {
            id,
            status: CheckStatus::Warn,
            detail: detail.into(),
            remediation: Some(remediation.into()),
        }
    }

    fn fail(id: &'static str, detail: impl Into<String>, remediation: impl Into<String>) -> Self {
        Self {
            id,
            status: CheckStatus::Fail,
            detail: detail.into(),
            remediation: Some(remediation.into()),
        }
    }
}

#[derive(Debug, Clone)]
struct PreflightReport {
    generated_at_utc: String,
    checks: Vec<PreflightCheck>,
}

impl PreflightReport {
    fn overall_status(&self) -> CheckStatus {
        if self
            .checks
            .iter()
            .any(|check| matches!(check.status, CheckStatus::Fail))
        {
            return CheckStatus::Fail;
        }
        if self
            .checks
            .iter()
            .any(|check| matches!(check.status, CheckStatus::Warn))
        {
            return CheckStatus::Warn;
        }
        CheckStatus::Pass
    }
}

#[derive(Debug)]
struct CliError {
    message: String,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

enum ParseOutcome {
    Help,
    Config(TranscribeConfig),
}

#[derive(Debug, Clone)]
struct VadBoundary {
    id: usize,
    start_ms: u64,
    end_ms: u64,
    source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelVadBoundary {
    channel: String,
    start_ms: u64,
    end_ms: u64,
    source: &'static str,
}

#[derive(Debug, Clone)]
struct IncrementalVadTracker {
    channel: String,
    sample_rate_hz: u32,
    threshold: f32,
    min_speech_samples: usize,
    min_silence_samples: usize,
    in_speech: bool,
    speech_run: usize,
    silence_run: usize,
    segment_start_idx: usize,
    boundaries: Vec<ChannelVadBoundary>,
}

impl IncrementalVadTracker {
    fn new(
        channel: impl Into<String>,
        sample_rate_hz: u32,
        threshold: f32,
        min_speech_ms: u32,
        min_silence_ms: u32,
    ) -> Self {
        Self {
            channel: channel.into(),
            sample_rate_hz,
            threshold,
            min_speech_samples: ((sample_rate_hz as u64 * min_speech_ms as u64) / 1_000).max(1)
                as usize,
            min_silence_samples: ((sample_rate_hz as u64 * min_silence_ms as u64) / 1_000).max(1)
                as usize,
            in_speech: false,
            speech_run: 0,
            silence_run: 0,
            segment_start_idx: 0,
            boundaries: Vec::new(),
        }
    }

    fn observe(&mut self, frame_idx: usize, level: f32) {
        let is_speech = level >= self.threshold;
        if is_speech {
            self.speech_run += 1;
            self.silence_run = 0;
            if !self.in_speech && self.speech_run >= self.min_speech_samples {
                self.in_speech = true;
                self.segment_start_idx = frame_idx + 1 - self.speech_run;
            }
            return;
        }

        self.speech_run = 0;
        if !self.in_speech {
            self.silence_run = 0;
            return;
        }

        self.silence_run += 1;
        if self.silence_run >= self.min_silence_samples {
            let end_frame_exclusive = frame_idx + 1 - self.silence_run;
            self.push_boundary(end_frame_exclusive, "energy_threshold");
            self.in_speech = false;
            self.silence_run = 0;
        }
    }

    fn finish(mut self, total_frames: usize) -> Vec<ChannelVadBoundary> {
        if self.in_speech {
            self.push_boundary(total_frames, "shutdown_flush");
        }
        self.boundaries
    }

    fn push_boundary(&mut self, end_frame_exclusive: usize, source: &'static str) {
        if end_frame_exclusive <= self.segment_start_idx {
            return;
        }
        self.boundaries.push(ChannelVadBoundary {
            channel: self.channel.clone(),
            start_ms: sample_to_ms(self.segment_start_idx as u64, self.sample_rate_hz),
            end_ms: sample_to_ms(end_frame_exclusive as u64, self.sample_rate_hz),
            source,
        });
    }
}

#[derive(Debug, Clone)]
struct LiveRunReport {
    generated_at_utc: String,
    backend_id: &'static str,
    resolved_model_path: PathBuf,
    resolved_model_source: String,
    channel_mode: ChannelMode,
    active_channel_mode: ChannelMode,
    transcript_text: String,
    channel_transcripts: Vec<ChannelTranscriptSummary>,
    vad_boundaries: Vec<VadBoundary>,
    events: Vec<TranscriptEvent>,
    degradation_events: Vec<ModeDegradationEvent>,
    trust_notices: Vec<TrustNotice>,
    lifecycle: LiveLifecycleTelemetry,
    reconciliation: ReconciliationMatrix,
    asr_worker_pool: LiveAsrPoolTelemetry,
    final_buffering: FinalBufferingTelemetry,
    chunk_queue: LiveChunkQueueTelemetry,
    cleanup_queue: CleanupQueueTelemetry,
    benchmark: BenchmarkSummary,
    benchmark_summary_csv: PathBuf,
    benchmark_runs_csv: PathBuf,
}

#[derive(Debug, Clone, Copy, Default)]
struct FinalBufferingTelemetry {
    submit_window: usize,
    deferred_final_submissions: usize,
    max_pending_final_backlog: usize,
}

#[derive(Debug, Clone)]
struct ReconciliationTrigger {
    code: &'static str,
}

#[derive(Debug, Clone)]
struct ReconciliationMatrix {
    required: bool,
    applied: bool,
    triggers: Vec<ReconciliationTrigger>,
}

impl ReconciliationMatrix {
    fn none() -> Self {
        Self {
            required: false,
            applied: false,
            triggers: Vec::new(),
        }
    }

    fn trigger_codes_csv(&self) -> String {
        if self.triggers.is_empty() {
            return "<none>".to_string();
        }
        self.triggers
            .iter()
            .map(|trigger| trigger.code)
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveLifecyclePhase {
    Warmup,
    Active,
    Draining,
    Shutdown,
}

impl LiveLifecyclePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Active => "active",
            Self::Draining => "draining",
            Self::Shutdown => "shutdown",
        }
    }

    fn ready_for_transcripts(self) -> bool {
        !matches!(self, Self::Warmup)
    }
}

impl Display for LiveLifecyclePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
struct LiveLifecycleTransition {
    phase: LiveLifecyclePhase,
    entered_at_utc: String,
    detail: String,
}

#[derive(Debug, Clone)]
struct LiveLifecycleTelemetry {
    current_phase: LiveLifecyclePhase,
    ready_for_transcripts: bool,
    transitions: Vec<LiveLifecycleTransition>,
}

impl LiveLifecycleTelemetry {
    fn new() -> Self {
        Self {
            current_phase: LiveLifecyclePhase::Warmup,
            ready_for_transcripts: false,
            transitions: Vec::new(),
        }
    }

    fn transition(&mut self, phase: LiveLifecyclePhase, detail: impl Into<String>) {
        self.current_phase = phase;
        self.ready_for_transcripts = phase.ready_for_transcripts();
        self.transitions.push(LiveLifecycleTransition {
            phase,
            entered_at_utc: runtime_timestamp_utc(),
            detail: detail.into(),
        });
    }
}

#[derive(Debug, Clone)]
struct LiveChunkQueueTelemetry {
    enabled: bool,
    max_queue: usize,
    submitted: usize,
    enqueued: usize,
    dropped_oldest: usize,
    processed: usize,
    pending: usize,
    high_water: usize,
    drain_completed: bool,
    lag_sample_count: usize,
    lag_p50_ms: u64,
    lag_p95_ms: u64,
    lag_max_ms: u64,
}

impl LiveChunkQueueTelemetry {
    fn disabled(config: &TranscribeConfig) -> Self {
        Self {
            enabled: false,
            max_queue: config.chunk_queue_cap,
            submitted: 0,
            enqueued: 0,
            dropped_oldest: 0,
            processed: 0,
            pending: 0,
            high_water: 0,
            drain_completed: true,
            lag_sample_count: 0,
            lag_p50_ms: 0,
            lag_p95_ms: 0,
            lag_max_ms: 0,
        }
    }

    fn enabled(max_queue: usize) -> Self {
        Self {
            enabled: true,
            max_queue,
            submitted: 0,
            enqueued: 0,
            dropped_oldest: 0,
            processed: 0,
            pending: 0,
            high_water: 0,
            drain_completed: true,
            lag_sample_count: 0,
            lag_p50_ms: 0,
            lag_p95_ms: 0,
            lag_max_ms: 0,
        }
    }
}

fn update_chunk_lag_telemetry(telemetry: &mut LiveChunkQueueTelemetry, lag_samples_ms: &[u64]) {
    telemetry.lag_sample_count = lag_samples_ms.len();
    if lag_samples_ms.is_empty() {
        telemetry.lag_p50_ms = 0;
        telemetry.lag_p95_ms = 0;
        telemetry.lag_max_ms = 0;
        return;
    }
    let mut sorted = lag_samples_ms.to_vec();
    sorted.sort_unstable();
    telemetry.lag_p50_ms = percentile_nearest_rank_ms(&sorted, 50);
    telemetry.lag_p95_ms = percentile_nearest_rank_ms(&sorted, 95);
    telemetry.lag_max_ms = *sorted.last().unwrap_or(&0);
}

fn chunk_queue_backpressure_is_severe(telemetry: &LiveChunkQueueTelemetry) -> bool {
    if !telemetry.enabled || telemetry.dropped_oldest == 0 || telemetry.submitted == 0 {
        return false;
    }

    let sustained_full_pressure = telemetry.max_queue > 0
        && telemetry.high_water >= telemetry.max_queue
        && telemetry.dropped_oldest >= telemetry.max_queue;
    let elevated_drop_ratio = telemetry.dropped_oldest.saturating_mul(3) >= telemetry.submitted;

    sustained_full_pressure || elevated_drop_ratio
}

fn percentile_nearest_rank_ms(sorted_samples: &[u64], percentile: usize) -> u64 {
    if sorted_samples.is_empty() {
        return 0;
    }
    let rank = ((sorted_samples.len() * percentile).saturating_add(99)) / 100;
    let idx = rank.saturating_sub(1).min(sorted_samples.len() - 1);
    sorted_samples[idx]
}

#[derive(Debug, Clone)]
struct CleanupQueueTelemetry {
    enabled: bool,
    max_queue: usize,
    timeout_ms: u64,
    retries: usize,
    submitted: usize,
    enqueued: usize,
    dropped_queue_full: usize,
    processed: usize,
    succeeded: usize,
    timed_out: usize,
    failed: usize,
    retry_attempts: usize,
    pending: usize,
    drain_budget_ms: u64,
    drain_completed: bool,
}

impl CleanupQueueTelemetry {
    fn disabled(config: &TranscribeConfig) -> Self {
        Self {
            enabled: false,
            max_queue: config.llm_max_queue,
            timeout_ms: config.llm_timeout_ms,
            retries: config.llm_retries,
            submitted: 0,
            enqueued: 0,
            dropped_queue_full: 0,
            processed: 0,
            succeeded: 0,
            timed_out: 0,
            failed: 0,
            retry_attempts: 0,
            pending: 0,
            drain_budget_ms: 0,
            drain_completed: true,
        }
    }
}

#[derive(Debug, Clone)]
struct CleanupRequest {
    segment_id: String,
    channel: String,
    start_ms: u64,
    end_ms: u64,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupTaskStatus {
    Succeeded,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone)]
struct CleanupTaskResult {
    request: CleanupRequest,
    status: CleanupTaskStatus,
    retry_attempts: usize,
    cleaned_text: Option<String>,
}

#[derive(Debug, Clone)]
struct CleanupClientConfig {
    endpoint: String,
    model: String,
    timeout_ms: u64,
    retries: usize,
}

#[derive(Debug, Clone)]
struct ResolvedModelPath {
    path: PathBuf,
    source: String,
}

#[derive(Debug, Clone)]
struct ModelChecksumInfo {
    sha256: String,
    status: String,
}

#[derive(Debug, Clone)]
struct CleanupRunResult {
    telemetry: CleanupQueueTelemetry,
    llm_events: Vec<TranscriptEvent>,
}

#[derive(Debug, Clone)]
struct CleanupAttemptOutcome {
    status: CleanupTaskStatus,
    cleaned_text: Option<String>,
}

#[derive(Debug, Clone)]
struct TranscriptEvent {
    event_type: &'static str,
    channel: String,
    segment_id: String,
    start_ms: u64,
    end_ms: u64,
    text: String,
    source_final_segment_id: Option<String>,
}

struct RuntimeJsonlStream {
    file: File,
    lines_written: usize,
}

impl RuntimeJsonlStream {
    fn open(path: &Path) -> Result<Self, CliError> {
        ensure_runtime_jsonl_parent(path)?;
        let file = File::create(path).map_err(|err| {
            CliError::new(format!(
                "failed to create JSONL file {}: {err}",
                display_path(path)
            ))
        })?;
        Ok(Self {
            file,
            lines_written: 0,
        })
    }

    fn write_line(&mut self, line: &str) -> Result<(), CliError> {
        writeln!(self.file, "{line}").map_err(io_to_cli)?;
        self.lines_written += 1;
        if self.lines_written % JSONL_SYNC_EVERY_LINES == 0 {
            self.file.sync_data().map_err(io_to_cli)?;
        }
        Ok(())
    }

    fn checkpoint(&mut self) -> Result<(), CliError> {
        self.file.sync_data().map_err(io_to_cli)
    }

    fn finalize(self) -> Result<(), CliError> {
        if self.lines_written > 0 {
            self.file.sync_data().map_err(io_to_cli)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalRenderMode {
    InteractiveTty,
    DeterministicNonTty,
}

impl TerminalRenderMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::InteractiveTty => "interactive-tty",
            Self::DeterministicNonTty => "deterministic-non-tty",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalRenderAction {
    kind: TerminalRenderActionKind,
    line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalRenderActionKind {
    PartialOverwrite,
    StableLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RollingChunkWindow {
    index: usize,
    start_ms: u64,
    end_ms: u64,
    overlap_prev_ms: u64,
}

#[derive(Debug, Clone)]
struct AsrWorkItem {
    class: AsrWorkClass,
    tick_index: usize,
    channel: String,
    segment_id: String,
    start_ms: u64,
    end_ms: u64,
    text: String,
    source_final_segment_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsrWorkClass {
    Partial,
    Final,
    Reconcile,
}

impl AsrWorkClass {
    fn priority_rank(self) -> u8 {
        match self {
            Self::Final => 0,
            Self::Reconcile => 1,
            Self::Partial => 2,
        }
    }
}

#[derive(Debug, Clone)]
struct LiveChunkBuildResult {
    events: Vec<TranscriptEvent>,
    telemetry: LiveChunkQueueTelemetry,
}

#[derive(Debug, Clone)]
struct BenchmarkSummary {
    run_count: usize,
    wall_ms_p50: f64,
    wall_ms_p95: f64,
    partial_slo_met: bool,
    final_slo_met: bool,
}

#[derive(Debug, Clone)]
struct ChannelInputPlan {
    role: &'static str,
    label: String,
    audio_path: PathBuf,
    is_temp_audio: bool,
}

#[derive(Debug, Clone)]
struct ModeDegradationEvent {
    code: &'static str,
    detail: String,
}

#[derive(Debug, Clone)]
struct TrustNotice {
    code: String,
    severity: String,
    cause: String,
    impact: String,
    guidance: String,
}

#[derive(Debug, Clone)]
struct ChannelInputPlanResult {
    inputs: Vec<ChannelInputPlan>,
    active_mode: ChannelMode,
    degradation_events: Vec<ModeDegradationEvent>,
}

#[derive(Debug, Clone)]
struct ChannelTranscriptSummary {
    role: &'static str,
    label: String,
    text: String,
}

#[derive(Debug, Clone)]
struct ChannelTranscriptionRun {
    summaries: Vec<ChannelTranscriptSummary>,
    asr_worker_pool: LiveAsrPoolTelemetry,
    final_buffering: FinalBufferingTelemetry,
}

struct LiveStreamIncrementalJsonlWriter {
    stream: RuntimeJsonlStream,
    backend_id: &'static str,
    channel_mode: ChannelMode,
    speaker_labels: SpeakerLabels,
    lifecycle_transition_index: usize,
}

impl LiveStreamIncrementalJsonlWriter {
    fn open(config: &TranscribeConfig) -> Result<Self, CliError> {
        Ok(Self {
            stream: RuntimeJsonlStream::open(&config.out_jsonl)?,
            backend_id: backend_id_for_asr_backend(config.asr_backend),
            channel_mode: config.channel_mode,
            speaker_labels: config.speaker_labels.clone(),
            lifecycle_transition_index: 0,
        })
    }

    fn emit_runtime_event(&mut self, event: &RuntimeOutputEvent) -> Result<(), CliError> {
        match event {
            RuntimeOutputEvent::Lifecycle { phase, detail, .. } => {
                let transition = LiveLifecycleTransition {
                    phase: runtime_phase_to_lifecycle_phase(*phase),
                    entered_at_utc: runtime_timestamp_utc(),
                    detail: detail.clone(),
                };
                self.stream.write_line(&jsonl_lifecycle_phase_line(
                    self.lifecycle_transition_index,
                    &transition,
                ))?;
                self.lifecycle_transition_index += 1;
                self.stream.checkpoint()?;
            }
            RuntimeOutputEvent::AsrCompleted { result, .. } => {
                let Some(event) = transcript_event_from_runtime_asr_result(
                    self.channel_mode,
                    &self.speaker_labels,
                    result,
                ) else {
                    return Ok(());
                };
                self.stream
                    .write_line(&jsonl_transcript_event_line(&event, self.backend_id, 0))?;
            }
            RuntimeOutputEvent::CaptureEvent { .. } | RuntimeOutputEvent::AsrQueued { .. } => {}
        }
        Ok(())
    }

    fn finalize(self) -> Result<(), CliError> {
        self.stream.finalize()
    }
}

struct CollectingRuntimeOutputSink {
    events: Vec<RuntimeOutputEvent>,
    incremental_jsonl: Option<LiveStreamIncrementalJsonlWriter>,
}

impl Default for CollectingRuntimeOutputSink {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            incremental_jsonl: None,
        }
    }
}

impl CollectingRuntimeOutputSink {
    fn with_incremental_jsonl(writer: LiveStreamIncrementalJsonlWriter) -> Self {
        Self {
            events: Vec::new(),
            incremental_jsonl: Some(writer),
        }
    }

    fn finalize_incremental_jsonl(&mut self) -> Result<(), CliError> {
        if let Some(writer) = self.incremental_jsonl.take() {
            writer.finalize()?;
        }
        Ok(())
    }
}

impl RuntimeOutputSink for CollectingRuntimeOutputSink {
    fn emit(&mut self, event: RuntimeOutputEvent) -> Result<(), String> {
        if let Some(writer) = self.incremental_jsonl.as_mut() {
            writer
                .emit_runtime_event(&event)
                .map_err(|err| err.to_string())?;
        }
        self.events.push(event);
        Ok(())
    }
}

#[derive(Debug, Default)]
struct CollectingRuntimeFinalizer {
    summary: Option<LiveRuntimeSummary>,
}

impl RuntimeFinalizer for CollectingRuntimeFinalizer {
    fn finalize(&mut self, summary: &LiveRuntimeSummary) -> Result<(), String> {
        self.summary = Some(summary.clone());
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ReadableChannelTranscript {
    channel: String,
    text: String,
}

fn select_runtime_execution_branch(config: &TranscribeConfig) -> RuntimeExecutionBranch {
    if config.live_stream {
        RuntimeExecutionBranch::LiveStream
    } else if config.live_chunked {
        RuntimeExecutionBranch::RepresentativeChunked
    } else {
        RuntimeExecutionBranch::RepresentativeOffline
    }
}

fn run_runtime_pipeline(config: &TranscribeConfig) -> Result<LiveRunReport, CliError> {
    match select_runtime_execution_branch(config) {
        RuntimeExecutionBranch::RepresentativeOffline => {
            run_representative_offline_pipeline(config)
        }
        RuntimeExecutionBranch::RepresentativeChunked => {
            run_representative_chunked_pipeline(config)
        }
        RuntimeExecutionBranch::LiveStream => run_live_stream_pipeline(config),
    }
}

const fn backend_id_for_asr_backend(backend: AsrBackend) -> &'static str {
    match backend {
        AsrBackend::WhisperCpp => "whispercpp",
        AsrBackend::WhisperKit => "whisperkit",
        AsrBackend::Moonshine => "moonshine",
    }
}

fn run_representative_offline_pipeline(
    config: &TranscribeConfig,
) -> Result<LiveRunReport, CliError> {
    runtime_representative::run_representative_offline_pipeline(config)
}

fn run_representative_chunked_pipeline(
    config: &TranscribeConfig,
) -> Result<LiveRunReport, CliError> {
    runtime_representative::run_representative_chunked_pipeline(config)
}

fn run_live_stream_pipeline(config: &TranscribeConfig) -> Result<LiveRunReport, CliError> {
    runtime_live_stream::run_live_stream_pipeline(config)
}

#[cfg(test)]
fn live_stream_vad_thresholds_per_mille(vad_threshold: f32) -> (u16, u16) {
    runtime_live_stream::live_stream_vad_thresholds_per_mille(vad_threshold)
}

fn build_transcript_events(
    transcript_text: &str,
    vad_boundaries: &[VadBoundary],
    channel_label: &str,
    segment_key: &str,
    live_chunked: bool,
    chunk_window_ms: u64,
    chunk_stride_ms: u64,
) -> Vec<TranscriptEvent> {
    let start_ms = vad_boundaries.first().map(|v| v.start_ms).unwrap_or(0);
    let end_ms = vad_boundaries.last().map(|v| v.end_ms).unwrap_or(0);
    if live_chunked {
        return build_live_chunked_events(
            transcript_text,
            channel_label,
            segment_key,
            start_ms,
            end_ms,
            chunk_window_ms,
            chunk_stride_ms,
        );
    }

    let partial_end_ms = start_ms + ((end_ms.saturating_sub(start_ms)) / 2);
    let partial_text = partial_text(transcript_text);

    vec![
        TranscriptEvent {
            event_type: "partial",
            channel: channel_label.to_string(),
            segment_id: format!("{segment_key}-representative-0"),
            start_ms,
            end_ms: partial_end_ms,
            text: partial_text,
            source_final_segment_id: None,
        },
        TranscriptEvent {
            event_type: "final",
            channel: channel_label.to_string(),
            segment_id: format!("{segment_key}-representative-0"),
            start_ms,
            end_ms,
            text: transcript_text.to_string(),
            source_final_segment_id: None,
        },
    ]
}

fn build_live_chunked_events(
    transcript_text: &str,
    channel_label: &str,
    segment_key: &str,
    start_ms: u64,
    end_ms: u64,
    chunk_window_ms: u64,
    chunk_stride_ms: u64,
) -> Vec<TranscriptEvent> {
    let chunks = build_rolling_chunk_windows(start_ms, end_ms, chunk_window_ms, chunk_stride_ms);
    if chunks.is_empty() {
        return Vec::new();
    }

    let mut events = Vec::with_capacity(chunks.len() * 2);
    for chunk in &chunks {
        let segment_id = near_live_segment_id(segment_key, chunk);
        let final_text = chunk_scoped_text(transcript_text, chunk.index, chunks.len());
        let partial_end_ms = chunk.start_ms + ((chunk.end_ms.saturating_sub(chunk.start_ms)) / 2);

        events.push(TranscriptEvent {
            event_type: "partial",
            channel: channel_label.to_string(),
            segment_id: segment_id.clone(),
            start_ms: chunk.start_ms,
            end_ms: partial_end_ms,
            text: partial_text(&final_text),
            source_final_segment_id: None,
        });
        events.push(TranscriptEvent {
            event_type: "final",
            channel: channel_label.to_string(),
            segment_id,
            start_ms: chunk.start_ms,
            end_ms: chunk.end_ms,
            text: final_text,
            source_final_segment_id: None,
        });
    }

    events
}

fn build_live_chunked_events_with_queue(
    channel_transcripts: &[ChannelTranscriptSummary],
    vad_boundaries: &[VadBoundary],
    chunk_window_ms: u64,
    chunk_stride_ms: u64,
    chunk_queue_cap: usize,
) -> LiveChunkBuildResult {
    if vad_boundaries.is_empty() || channel_transcripts.is_empty() {
        return LiveChunkBuildResult {
            events: Vec::new(),
            telemetry: LiveChunkQueueTelemetry::enabled(chunk_queue_cap),
        };
    }

    let mut tasks = Vec::new();
    let ordered_boundaries = ordered_vad_boundaries_for_segments(vad_boundaries);
    let ordered_transcripts = ordered_channel_transcripts(channel_transcripts);
    let boundary_count = ordered_boundaries.len();
    for (stable_boundary_idx, boundary) in ordered_boundaries.iter().enumerate() {
        let chunks = build_rolling_chunk_windows(
            boundary.start_ms,
            boundary.end_ms,
            chunk_window_ms,
            chunk_stride_ms,
        );
        if chunks.is_empty() {
            continue;
        }
        let closure_tick_index = boundary_tick_index(boundary.end_ms, chunk_stride_ms);
        for transcript in &ordered_transcripts {
            let boundary_text =
                chunk_scoped_text(&transcript.text, stable_boundary_idx, boundary_count);
            let last_chunk_index = chunks.len().saturating_sub(1);
            for chunk in chunks.iter().take(last_chunk_index) {
                tasks.push(AsrWorkItem {
                    class: AsrWorkClass::Partial,
                    tick_index: boundary_tick_index(chunk.end_ms, chunk_stride_ms),
                    channel: transcript.label.clone(),
                    segment_id: near_live_partial_segment_id(
                        transcript.role,
                        stable_boundary_idx,
                        chunk,
                    ),
                    start_ms: chunk.start_ms,
                    end_ms: chunk.end_ms,
                    text: chunk_scoped_text(&boundary_text, chunk.index, chunks.len()),
                    source_final_segment_id: None,
                });
            }
            tasks.push(AsrWorkItem {
                class: AsrWorkClass::Final,
                tick_index: closure_tick_index,
                channel: transcript.label.clone(),
                segment_id: near_live_boundary_segment_id(
                    transcript.role,
                    stable_boundary_idx,
                    boundary.start_ms,
                    boundary.end_ms,
                ),
                start_ms: boundary.start_ms,
                end_ms: boundary.end_ms,
                text: boundary_text,
                source_final_segment_id: None,
            });
        }
    }
    tasks.sort_by(|a, b| {
        a.tick_index
            .cmp(&b.tick_index)
            .then_with(|| a.start_ms.cmp(&b.start_ms))
            .then_with(|| a.end_ms.cmp(&b.end_ms))
            .then_with(|| a.class.priority_rank().cmp(&b.class.priority_rank()))
            .then_with(|| a.channel.cmp(&b.channel))
            .then_with(|| a.segment_id.cmp(&b.segment_id))
            .then_with(|| a.source_final_segment_id.cmp(&b.source_final_segment_id))
            .then_with(|| a.text.cmp(&b.text))
    });

    run_live_chunk_queue(tasks, chunk_queue_cap, chunk_stride_ms)
}

fn boundary_tick_index(end_ms: u64, chunk_stride_ms: u64) -> usize {
    if chunk_stride_ms == 0 {
        return 0;
    }
    (end_ms / chunk_stride_ms) as usize
}

fn ordered_vad_boundaries_for_segments(vad_boundaries: &[VadBoundary]) -> Vec<VadBoundary> {
    let mut ordered = vad_boundaries.to_vec();
    ordered.sort_by(|a, b| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| a.end_ms.cmp(&b.end_ms))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.id.cmp(&b.id))
    });
    ordered
}

fn ordered_channel_transcripts(
    channel_transcripts: &[ChannelTranscriptSummary],
) -> Vec<ChannelTranscriptSummary> {
    let mut ordered = channel_transcripts.to_vec();
    ordered.sort_by(|a, b| {
        channel_sort_key(a.role)
            .cmp(&channel_sort_key(b.role))
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.text.cmp(&b.text))
    });
    ordered
}

#[cfg(test)]
fn build_reconciliation_events(
    channel_transcripts: &[ChannelTranscriptSummary],
    vad_boundaries: &[VadBoundary],
) -> Vec<TranscriptEvent> {
    let ordered_transcripts = ordered_channel_transcripts(channel_transcripts);
    let ordered_boundaries = ordered_vad_boundaries_for_segments(vad_boundaries);
    let items = ordered_transcripts
        .iter()
        .flat_map(|transcript| {
            build_transcript_events(
                &transcript.text,
                &ordered_boundaries,
                &transcript.label,
                transcript.role,
                false,
                DEFAULT_CHUNK_WINDOW_MS,
                DEFAULT_CHUNK_STRIDE_MS,
            )
        })
        .filter_map(|event| {
            if event.event_type != "final" {
                return None;
            }
            Some(AsrWorkItem {
                class: AsrWorkClass::Reconcile,
                tick_index: 0,
                channel: event.channel,
                segment_id: format!("{}-reconciled", event.segment_id),
                start_ms: event.start_ms,
                end_ms: event.end_ms,
                text: event.text,
                source_final_segment_id: Some(event.segment_id),
            })
        })
        .collect::<Vec<_>>();
    items
        .into_iter()
        .flat_map(emit_asr_work_item_events)
        .collect()
}

fn build_targeted_reconciliation_events(
    channel_transcripts: &[ChannelTranscriptSummary],
    vad_boundaries: &[VadBoundary],
    live_events: &[TranscriptEvent],
    reconciliation: &ReconciliationMatrix,
) -> Vec<TranscriptEvent> {
    let ordered_transcripts = ordered_channel_transcripts(channel_transcripts);
    let ordered_boundaries = ordered_vad_boundaries_for_segments(vad_boundaries);
    if ordered_transcripts.is_empty() || ordered_boundaries.is_empty() || !reconciliation.required {
        return Vec::new();
    }

    let trigger_codes = reconciliation
        .triggers
        .iter()
        .map(|trigger| trigger.code)
        .collect::<HashSet<_>>();
    let live_final_ids = live_events
        .iter()
        .filter(|event| event.event_type == "final")
        .map(|event| event.segment_id.clone())
        .collect::<HashSet<_>>();

    let capture_integrity_triggered = trigger_codes.contains("continuity_recovered_with_gaps")
        || trigger_codes.contains("continuity_unverified")
        || trigger_codes.contains("capture_transport_degraded")
        || trigger_codes.contains("capture_callback_contract_degraded");
    let queue_drop_triggered = trigger_codes.contains("chunk_queue_drop_oldest");
    let shutdown_flush_triggered = trigger_codes.contains("shutdown_flush_boundary");

    let mut targeted_boundary_indexes = BTreeSet::new();
    if capture_integrity_triggered {
        targeted_boundary_indexes.extend(0..ordered_boundaries.len());
    }

    if shutdown_flush_triggered {
        for (boundary_idx, boundary) in ordered_boundaries.iter().enumerate() {
            if boundary.source == "shutdown_flush" {
                targeted_boundary_indexes.insert(boundary_idx);
            }
        }
    }

    if queue_drop_triggered {
        for (boundary_idx, boundary) in ordered_boundaries.iter().enumerate() {
            let missing_final = ordered_transcripts.iter().any(|transcript| {
                let expected_segment_id = near_live_boundary_segment_id(
                    transcript.role,
                    boundary_idx,
                    boundary.start_ms,
                    boundary.end_ms,
                );
                !live_final_ids.contains(&expected_segment_id)
            });
            if missing_final {
                targeted_boundary_indexes.insert(boundary_idx);
            }
        }
    }

    if targeted_boundary_indexes.is_empty() {
        targeted_boundary_indexes.extend(0..ordered_boundaries.len());
    }

    let boundary_count = ordered_boundaries.len();
    let mut items = Vec::new();
    for boundary_idx in targeted_boundary_indexes {
        let boundary = &ordered_boundaries[boundary_idx];
        for transcript in &ordered_transcripts {
            let source_final_segment_id = near_live_boundary_segment_id(
                transcript.role,
                boundary_idx,
                boundary.start_ms,
                boundary.end_ms,
            );
            let segment_text = chunk_scoped_text(&transcript.text, boundary_idx, boundary_count);
            if segment_text.trim().is_empty() {
                continue;
            }
            items.push(AsrWorkItem {
                class: AsrWorkClass::Reconcile,
                tick_index: 0,
                channel: transcript.label.clone(),
                segment_id: format!("{source_final_segment_id}-reconciled"),
                start_ms: boundary.start_ms,
                end_ms: boundary.end_ms,
                text: segment_text,
                source_final_segment_id: Some(source_final_segment_id),
            });
        }
    }

    items
        .into_iter()
        .flat_map(emit_asr_work_item_events)
        .collect()
}

fn build_reconciliation_matrix(
    vad_boundaries: &[VadBoundary],
    degradation_events: &[ModeDegradationEvent],
) -> ReconciliationMatrix {
    let mut triggers = Vec::new();
    let mut seen_codes = HashSet::new();

    for degradation in degradation_events {
        let trigger_code = match degradation.code {
            LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE => Some("chunk_queue_drop_oldest"),
            LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE => Some("continuity_recovered_with_gaps"),
            LIVE_CAPTURE_CONTINUITY_UNVERIFIED_CODE => Some("continuity_unverified"),
            LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE => Some("capture_transport_degraded"),
            LIVE_CAPTURE_CALLBACK_CONTRACT_DEGRADED_CODE => {
                Some("capture_callback_contract_degraded")
            }
            _ => None,
        };
        if let Some(code) = trigger_code {
            if seen_codes.insert(code) {
                triggers.push(ReconciliationTrigger { code });
            }
        }
    }

    if vad_boundaries
        .iter()
        .any(|boundary| boundary.source == "shutdown_flush")
        && seen_codes.insert("shutdown_flush_boundary")
    {
        triggers.push(ReconciliationTrigger {
            code: "shutdown_flush_boundary",
        });
    }

    ReconciliationMatrix {
        required: !triggers.is_empty(),
        applied: false,
        triggers,
    }
}

fn reconciliation_trigger_codes_json(triggers: &[ReconciliationTrigger]) -> String {
    triggers
        .iter()
        .map(|trigger| format!("\"{}\"", json_escape(trigger.code)))
        .collect::<Vec<_>>()
        .join(",")
}

fn run_live_chunk_queue(
    tasks: Vec<AsrWorkItem>,
    chunk_queue_cap: usize,
    chunk_stride_ms: u64,
) -> LiveChunkBuildResult {
    let mut telemetry = LiveChunkQueueTelemetry::enabled(chunk_queue_cap);
    let mut events = Vec::with_capacity(tasks.len() * 2);
    let mut queue = VecDeque::new();
    let mut last_tick_index: Option<usize> = None;
    let mut lag_samples_ms = Vec::new();

    for task in tasks {
        telemetry.submitted += 1;
        if last_tick_index != Some(task.tick_index) {
            process_next_live_chunk_task(
                &mut queue,
                &mut events,
                &mut telemetry,
                task.tick_index,
                chunk_stride_ms,
                &mut lag_samples_ms,
            );
            last_tick_index = Some(task.tick_index);
        }
        enqueue_live_chunk_task(&mut queue, task, &mut telemetry);
    }
    let mut drain_tick_index = last_tick_index.unwrap_or(0);
    while !queue.is_empty() {
        drain_tick_index = drain_tick_index.saturating_add(1);
        process_next_live_chunk_task(
            &mut queue,
            &mut events,
            &mut telemetry,
            drain_tick_index,
            chunk_stride_ms,
            &mut lag_samples_ms,
        );
    }

    telemetry.pending = queue.len();
    telemetry.drain_completed = telemetry.pending == 0;
    update_chunk_lag_telemetry(&mut telemetry, &lag_samples_ms);
    LiveChunkBuildResult { events, telemetry }
}

fn enqueue_live_chunk_task(
    queue: &mut VecDeque<AsrWorkItem>,
    task: AsrWorkItem,
    telemetry: &mut LiveChunkQueueTelemetry,
) {
    if telemetry.max_queue == 0 {
        telemetry.dropped_oldest += 1;
        return;
    }
    if queue.len() >= telemetry.max_queue {
        let mut drop_idx = 0usize;
        let mut drop_rank = queue
            .front()
            .map(|item| item.class.priority_rank())
            .unwrap_or(0);
        for (idx, queued) in queue.iter().enumerate().skip(1) {
            let rank = queued.class.priority_rank();
            if rank > drop_rank {
                drop_rank = rank;
                drop_idx = idx;
            }
        }
        queue.remove(drop_idx);
        telemetry.dropped_oldest += 1;
    }
    queue.push_back(task);
    telemetry.enqueued += 1;
    telemetry.high_water = telemetry.high_water.max(queue.len());
}

fn process_next_live_chunk_task(
    queue: &mut VecDeque<AsrWorkItem>,
    events: &mut Vec<TranscriptEvent>,
    telemetry: &mut LiveChunkQueueTelemetry,
    processing_tick_index: usize,
    chunk_stride_ms: u64,
    lag_samples_ms: &mut Vec<u64>,
) {
    let Some(task) = queue.pop_front() else {
        return;
    };

    telemetry.processed += 1;
    let lag_ticks = processing_tick_index.saturating_sub(task.tick_index);
    lag_samples_ms.push((lag_ticks as u64).saturating_mul(chunk_stride_ms));
    events.extend(emit_asr_work_item_events(task));
}

fn emit_asr_work_item_events(task: AsrWorkItem) -> Vec<TranscriptEvent> {
    match task.class {
        AsrWorkClass::Final => {
            let partial_end_ms = task.start_ms + ((task.end_ms.saturating_sub(task.start_ms)) / 2);
            vec![
                TranscriptEvent {
                    event_type: "partial",
                    channel: task.channel.clone(),
                    segment_id: task.segment_id.clone(),
                    start_ms: task.start_ms,
                    end_ms: partial_end_ms,
                    text: partial_text(&task.text),
                    source_final_segment_id: None,
                },
                TranscriptEvent {
                    event_type: "final",
                    channel: task.channel,
                    segment_id: task.segment_id,
                    start_ms: task.start_ms,
                    end_ms: task.end_ms,
                    text: task.text,
                    source_final_segment_id: None,
                },
            ]
        }
        AsrWorkClass::Partial => {
            let partial_end_ms = task.start_ms + ((task.end_ms.saturating_sub(task.start_ms)) / 2);
            vec![TranscriptEvent {
                event_type: "partial",
                channel: task.channel,
                segment_id: task.segment_id,
                start_ms: task.start_ms,
                end_ms: partial_end_ms,
                text: partial_text(&task.text),
                source_final_segment_id: None,
            }]
        }
        AsrWorkClass::Reconcile => vec![TranscriptEvent {
            event_type: "reconciled_final",
            channel: task.channel,
            segment_id: task.segment_id,
            start_ms: task.start_ms,
            end_ms: task.end_ms,
            text: task.text,
            source_final_segment_id: task.source_final_segment_id,
        }],
    }
}

fn build_rolling_chunk_windows(
    start_ms: u64,
    end_ms: u64,
    window_ms: u64,
    stride_ms: u64,
) -> Vec<RollingChunkWindow> {
    if end_ms <= start_ms || window_ms == 0 || stride_ms == 0 {
        return Vec::new();
    }

    let span_ms = end_ms - start_ms;
    let effective_window_ms = window_ms.min(span_ms);
    let last_start_ms = end_ms.saturating_sub(effective_window_ms);
    let mut chunk_starts = Vec::new();
    let mut next_start_ms = start_ms;
    while next_start_ms < last_start_ms {
        chunk_starts.push(next_start_ms);
        next_start_ms = next_start_ms.saturating_add(stride_ms);
    }
    chunk_starts.push(last_start_ms);
    chunk_starts.sort_unstable();
    chunk_starts.dedup();

    let mut previous_end_ms: Option<u64> = None;
    chunk_starts
        .into_iter()
        .enumerate()
        .map(|(index, chunk_start_ms)| {
            let chunk_end_ms = chunk_start_ms
                .saturating_add(effective_window_ms)
                .min(end_ms);
            let overlap_prev_ms = previous_end_ms
                .map(|prev_end_ms| prev_end_ms.saturating_sub(chunk_start_ms))
                .unwrap_or(0);
            previous_end_ms = Some(chunk_end_ms);
            RollingChunkWindow {
                index,
                start_ms: chunk_start_ms,
                end_ms: chunk_end_ms,
                overlap_prev_ms,
            }
        })
        .collect()
}

fn near_live_segment_id(segment_key: &str, chunk: &RollingChunkWindow) -> String {
    format!(
        "{segment_key}-chunk-{index:04}-{start_ms}-{end_ms}",
        index = chunk.index,
        start_ms = chunk.start_ms,
        end_ms = chunk.end_ms
    )
}

fn near_live_partial_segment_id(
    segment_key: &str,
    boundary_idx: usize,
    chunk: &RollingChunkWindow,
) -> String {
    format!(
        "{segment_key}-segment-{boundary_idx:04}-partial-{index:04}-{start_ms}-{end_ms}",
        index = chunk.index,
        start_ms = chunk.start_ms,
        end_ms = chunk.end_ms
    )
}

fn near_live_boundary_segment_id(
    segment_key: &str,
    boundary_idx: usize,
    start_ms: u64,
    end_ms: u64,
) -> String {
    format!(
        "{segment_key}-segment-{index:04}-{start_ms}-{end_ms}",
        index = boundary_idx,
        start_ms = start_ms,
        end_ms = end_ms
    )
}

fn chunk_scoped_text(transcript_text: &str, chunk_index: usize, chunk_count: usize) -> String {
    let trimmed = transcript_text.trim();
    if trimmed.is_empty() || chunk_count <= 1 {
        return trimmed.to_string();
    }

    let words = trimmed.split_whitespace().collect::<Vec<_>>();
    if words.len() <= 1 {
        return trimmed.to_string();
    }

    let start = (chunk_index * words.len()) / chunk_count;
    let mut end = ((chunk_index + 1) * words.len()) / chunk_count;
    if end <= start {
        end = (start + 1).min(words.len());
    }
    words[start..end].join(" ")
}

fn merge_transcript_events(events: Vec<TranscriptEvent>) -> Vec<TranscriptEvent> {
    transcript_flow::merge_transcript_events(events)
}

fn reconstruct_transcript(events: &[TranscriptEvent]) -> String {
    transcript_flow::reconstruct_transcript(events)
}

fn reconstruct_transcript_per_channel(
    events: &[TranscriptEvent],
) -> Vec<ReadableChannelTranscript> {
    transcript_flow::reconstruct_transcript_per_channel(events)
}

fn final_events_for_display<'a>(events: &'a [TranscriptEvent]) -> Vec<&'a TranscriptEvent> {
    transcript_flow::final_events_for_display(events)
}

fn terminal_render_mode() -> TerminalRenderMode {
    transcript_flow::terminal_render_mode()
}

fn format_stable_transcript_line(event: &TranscriptEvent) -> Option<String> {
    transcript_flow::format_stable_transcript_line(event)
}

fn format_partial_transcript_line(event: &TranscriptEvent) -> Option<String> {
    transcript_flow::format_partial_transcript_line(event)
}

fn is_stable_terminal_event(event_type: &str) -> bool {
    transcript_flow::is_stable_terminal_event(event_type)
}

fn build_terminal_render_actions(
    events: &[TranscriptEvent],
    mode: TerminalRenderMode,
) -> Vec<TerminalRenderAction> {
    transcript_flow::build_terminal_render_actions(events, mode)
}

#[cfg(test)]
fn live_terminal_render_actions(
    config: &TranscribeConfig,
    events: &[TranscriptEvent],
    mode: TerminalRenderMode,
) -> Vec<TerminalRenderAction> {
    transcript_flow::live_terminal_render_actions(config, events, mode)
}

fn maybe_emit_live_terminal_stream(config: &TranscribeConfig, events: &[TranscriptEvent]) {
    transcript_flow::maybe_emit_live_terminal_stream(config, events)
}

fn build_trust_notices(
    requested_mode: ChannelMode,
    active_mode: ChannelMode,
    degradation_events: &[ModeDegradationEvent],
    cleanup_queue: &CleanupQueueTelemetry,
    chunk_queue: &LiveChunkQueueTelemetry,
) -> Vec<TrustNotice> {
    transcript_flow::build_trust_notices(
        requested_mode,
        active_mode,
        degradation_events,
        cleanup_queue,
        chunk_queue,
    )
}

fn run_cleanup_queue(config: &TranscribeConfig, events: &[TranscriptEvent]) -> CleanupRunResult {
    run_cleanup_queue_with(config, events, invoke_cleanup_endpoint)
}

fn run_cleanup_queue_with<F>(
    config: &TranscribeConfig,
    events: &[TranscriptEvent],
    invoke_cleanup: F,
) -> CleanupRunResult
where
    F: Fn(&CleanupClientConfig, &CleanupRequest) -> CleanupAttemptOutcome + Send + Sync + 'static,
{
    if !config.llm_cleanup {
        return CleanupRunResult {
            telemetry: CleanupQueueTelemetry::disabled(config),
            llm_events: Vec::new(),
        };
    }

    let mut telemetry = CleanupQueueTelemetry {
        enabled: true,
        max_queue: config.llm_max_queue,
        timeout_ms: config.llm_timeout_ms,
        retries: config.llm_retries,
        submitted: 0,
        enqueued: 0,
        dropped_queue_full: 0,
        processed: 0,
        succeeded: 0,
        timed_out: 0,
        failed: 0,
        retry_attempts: 0,
        pending: 0,
        drain_budget_ms: config.llm_timeout_ms,
        drain_completed: true,
    };
    let mut llm_events = Vec::new();

    let Some(endpoint) = config.llm_endpoint.clone() else {
        return CleanupRunResult {
            telemetry,
            llm_events,
        };
    };
    let Some(model) = config.llm_model.clone() else {
        return CleanupRunResult {
            telemetry,
            llm_events,
        };
    };

    let requests = cleanup_requests_from_events(events);
    telemetry.submitted = requests.len();
    if requests.is_empty() {
        return CleanupRunResult {
            telemetry,
            llm_events,
        };
    }

    let client = CleanupClientConfig {
        endpoint,
        model,
        timeout_ms: config.llm_timeout_ms,
        retries: config.llm_retries,
    };
    let (request_tx, request_rx) = sync_channel::<CleanupRequest>(config.llm_max_queue);
    let (result_tx, result_rx) = mpsc::channel::<CleanupTaskResult>();
    let invoke_cleanup = Arc::new(invoke_cleanup);
    let worker_invoke = Arc::clone(&invoke_cleanup);
    let worker_handle = thread::spawn(move || {
        cleanup_worker_loop(request_rx, result_tx, client, worker_invoke);
    });

    for request in requests {
        match request_tx.try_send(request) {
            Ok(()) => telemetry.enqueued += 1,
            Err(TrySendError::Full(_)) => telemetry.dropped_queue_full += 1,
            Err(TrySendError::Disconnected(_)) => telemetry.failed += 1,
        }
    }
    drop(request_tx);

    let drain_deadline = Instant::now() + Duration::from_millis(config.llm_timeout_ms);
    while telemetry.processed < telemetry.enqueued {
        let now = Instant::now();
        if now >= drain_deadline {
            break;
        }
        let remaining = drain_deadline.saturating_duration_since(now);
        let wait_for = remaining.min(Duration::from_millis(5));
        match result_rx.recv_timeout(wait_for) {
            Ok(result) => {
                telemetry.processed += 1;
                telemetry.retry_attempts += result.retry_attempts;
                match result.status {
                    CleanupTaskStatus::Succeeded => {
                        telemetry.succeeded += 1;
                        if let Some(cleaned_text) = result.cleaned_text {
                            let source_segment_id = result.request.segment_id.clone();
                            llm_events.push(TranscriptEvent {
                                event_type: "llm_final",
                                channel: result.request.channel.clone(),
                                segment_id: format!("{source_segment_id}-llm"),
                                start_ms: result.request.start_ms,
                                end_ms: result.request.end_ms,
                                text: cleaned_text,
                                source_final_segment_id: Some(source_segment_id),
                            });
                        }
                    }
                    CleanupTaskStatus::TimedOut => telemetry.timed_out += 1,
                    CleanupTaskStatus::Failed => telemetry.failed += 1,
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    telemetry.pending = telemetry.enqueued.saturating_sub(telemetry.processed);
    telemetry.drain_completed = telemetry.pending == 0;
    if telemetry.drain_completed {
        let _ = worker_handle.join();
    }
    CleanupRunResult {
        telemetry,
        llm_events,
    }
}

fn cleanup_requests_from_events(events: &[TranscriptEvent]) -> Vec<CleanupRequest> {
    events
        .iter()
        .filter(|event| event.event_type == "final")
        .map(|event| CleanupRequest {
            segment_id: event.segment_id.clone(),
            channel: event.channel.clone(),
            start_ms: event.start_ms,
            end_ms: event.end_ms,
            text: event.text.clone(),
        })
        .collect()
}

fn cleanup_worker_loop<F>(
    request_rx: Receiver<CleanupRequest>,
    result_tx: mpsc::Sender<CleanupTaskResult>,
    client: CleanupClientConfig,
    invoke_cleanup: Arc<F>,
) where
    F: Fn(&CleanupClientConfig, &CleanupRequest) -> CleanupAttemptOutcome + Send + Sync + 'static,
{
    while let Ok(request) = request_rx.recv() {
        let mut status = CleanupTaskStatus::Failed;
        let mut retry_attempts = 0usize;
        let mut cleaned_text = None;

        for attempt in 0..=client.retries {
            let outcome = invoke_cleanup(&client, &request);
            status = outcome.status;
            cleaned_text = outcome.cleaned_text;
            if status == CleanupTaskStatus::Succeeded {
                break;
            }
            if attempt < client.retries {
                retry_attempts += 1;
            }
        }

        if result_tx
            .send(CleanupTaskResult {
                request,
                status,
                retry_attempts,
                cleaned_text,
            })
            .is_err()
        {
            break;
        }
    }
}

fn invoke_cleanup_endpoint(
    client: &CleanupClientConfig,
    request: &CleanupRequest,
) -> CleanupAttemptOutcome {
    let timeout_secs = format!("{:.3}", (client.timeout_ms as f64 / 1_000.0).max(0.001));
    let prompt = format!(
        "Polish this transcript segment for readability without changing meaning. Return only cleaned text.\nsegment_id={}\nchannel={}\ntext={}",
        request.segment_id, request.channel, request.text
    );
    let payload = format!(
        "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}],\"stream\":false}}",
        json_escape(&client.model),
        json_escape("You clean transcript text. Do not add content."),
        json_escape(&prompt)
    );

    let output = Command::new("curl")
        .arg("-sS")
        .arg("--fail-with-body")
        .arg("--max-time")
        .arg(&timeout_secs)
        .arg("-X")
        .arg("POST")
        .arg(&client.endpoint)
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(&payload)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let cleaned_text = extract_json_string_field(&stdout, "content")
                .or_else(|| {
                    if stdout.is_empty() {
                        None
                    } else {
                        Some(stdout.clone())
                    }
                })
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            match cleaned_text {
                Some(text) => CleanupAttemptOutcome {
                    status: CleanupTaskStatus::Succeeded,
                    cleaned_text: Some(text),
                },
                None => CleanupAttemptOutcome {
                    status: CleanupTaskStatus::Failed,
                    cleaned_text: None,
                },
            }
        }
        Ok(output) if output.status.code() == Some(28) => CleanupAttemptOutcome {
            status: CleanupTaskStatus::TimedOut,
            cleaned_text: None,
        },
        Ok(_) => CleanupAttemptOutcome {
            status: CleanupTaskStatus::Failed,
            cleaned_text: None,
        },
        Err(_) => CleanupAttemptOutcome {
            status: CleanupTaskStatus::Failed,
            cleaned_text: None,
        },
    }
}

fn benchmark_track(channel_mode: ChannelMode) -> &'static str {
    match channel_mode {
        ChannelMode::Separate => "transcribe-live-dual-channel",
        ChannelMode::Mixed => "transcribe-live-single-channel",
        ChannelMode::MixedFallback => "transcribe-live-dual-channel",
    }
}

fn channel_sort_key(role: &str) -> u8 {
    match role {
        "mic" => 0,
        "system" => 1,
        _ => 2,
    }
}

fn partial_text(full_text: &str) -> String {
    let words: Vec<&str> = full_text.split_whitespace().collect();
    if words.len() <= 2 {
        return full_text.to_string();
    }
    words[..(words.len() / 2).max(1)].join(" ")
}

fn write_benchmark_artifact(
    stamp: &str,
    backend_id: &str,
    artifact_track: &str,
    wall_ms_runs: &[f64],
) -> Result<(PathBuf, PathBuf, BenchmarkSummary), CliError> {
    if wall_ms_runs.is_empty() {
        return Err(CliError::new(
            "cannot write benchmark artifact with zero runs",
        ));
    }

    let run_dir = PathBuf::from("artifacts")
        .join("bench")
        .join(artifact_track)
        .join(stamp);
    fs::create_dir_all(&run_dir).map_err(|err| {
        CliError::new(format!(
            "failed to create benchmark artifact directory {}: {err}",
            run_dir.display()
        ))
    })?;

    let summary_csv = run_dir.join("summary.csv");
    let runs_csv = run_dir.join("runs.csv");
    let wall_ms_p50 = percentile_f64(wall_ms_runs, 0.50);
    let wall_ms_p95 = percentile_f64(wall_ms_runs, 0.95);
    let partial_slo_met = wall_ms_p95 <= PARTIAL_LATENCY_SLO_MS;
    let final_slo_met = wall_ms_p95 <= FINAL_LATENCY_SLO_MS;

    let mut summary_file = File::create(&summary_csv).map_err(|err| {
        CliError::new(format!(
            "failed to create benchmark summary {}: {err}",
            summary_csv.display()
        ))
    })?;
    writeln!(summary_file, "key,value").map_err(io_to_cli)?;
    writeln!(summary_file, "backend_id,{backend_id}").map_err(io_to_cli)?;
    writeln!(summary_file, "artifact_track,{artifact_track}").map_err(io_to_cli)?;
    writeln!(summary_file, "run_count,{}", wall_ms_runs.len()).map_err(io_to_cli)?;
    writeln!(summary_file, "wall_ms_p50,{wall_ms_p50:.6}").map_err(io_to_cli)?;
    writeln!(summary_file, "wall_ms_p95,{wall_ms_p95:.6}").map_err(io_to_cli)?;
    writeln!(
        summary_file,
        "partial_slo_target_ms,{PARTIAL_LATENCY_SLO_MS:.0}"
    )
    .map_err(io_to_cli)?;
    writeln!(
        summary_file,
        "final_slo_target_ms,{FINAL_LATENCY_SLO_MS:.0}"
    )
    .map_err(io_to_cli)?;
    writeln!(summary_file, "partial_slo_met,{partial_slo_met}").map_err(io_to_cli)?;
    writeln!(summary_file, "final_slo_met,{final_slo_met}").map_err(io_to_cli)?;

    let mut runs_file = File::create(&runs_csv).map_err(|err| {
        CliError::new(format!(
            "failed to create benchmark runs {}: {err}",
            runs_csv.display()
        ))
    })?;
    writeln!(runs_file, "run_index,wall_ms").map_err(io_to_cli)?;
    for (idx, wall_ms) in wall_ms_runs.iter().enumerate() {
        writeln!(runs_file, "{idx},{wall_ms:.6}").map_err(io_to_cli)?;
    }

    Ok((
        summary_csv,
        runs_csv,
        BenchmarkSummary {
            run_count: wall_ms_runs.len(),
            wall_ms_p50,
            wall_ms_p95,
            partial_slo_met,
            final_slo_met,
        },
    ))
}

fn percentile_f64(values: &[f64], quantile: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
    sorted[index]
}

fn model_checksum_info(resolved_model: Option<&ResolvedModelPath>) -> ModelChecksumInfo {
    let Some(model) = resolved_model else {
        return ModelChecksumInfo {
            sha256: "<unavailable>".to_string(),
            status: "unavailable_unresolved".to_string(),
        };
    };

    if model.path.is_dir() {
        return ModelChecksumInfo {
            sha256: "<unavailable>".to_string(),
            status: "unavailable_directory".to_string(),
        };
    }

    if !model.path.is_file() {
        return ModelChecksumInfo {
            sha256: "<unavailable>".to_string(),
            status: "unavailable_not_file".to_string(),
        };
    }

    match sha256_file_hex(&model.path) {
        Ok(sha256) => ModelChecksumInfo {
            sha256,
            status: "available".to_string(),
        },
        Err(_) => ModelChecksumInfo {
            sha256: "<unavailable>".to_string(),
            status: "unavailable_checksum_error".to_string(),
        },
    }
}

fn checksum_cache_key(path: &Path) -> Option<ModelChecksumCacheKey> {
    let metadata = fs::metadata(path).ok()?;
    let modified_unix_nanos = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let canonical_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Some(ModelChecksumCacheKey {
        canonical_path,
        file_len: metadata.len(),
        modified_unix_nanos,
    })
}

fn model_checksum_cache() -> &'static Mutex<HashMap<ModelChecksumCacheKey, String>> {
    static CACHE: OnceLock<Mutex<HashMap<ModelChecksumCacheKey, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sha256_file_hex_uncached(path: &Path) -> Result<String, CliError> {
    let mut file = File::open(path).map_err(|err| {
        CliError::new(format!(
            "failed to open model for checksum {}: {err}",
            display_path(path)
        ))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(|err| {
            CliError::new(format!(
                "failed reading model for checksum {}: {err}",
                display_path(path)
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_file_hex(path: &Path) -> Result<String, CliError> {
    if let Some(cache_key) = checksum_cache_key(path) {
        if let Ok(cache) = model_checksum_cache().lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached.clone());
            }
        }
        let digest = sha256_file_hex_uncached(path)?;
        if let Ok(mut cache) = model_checksum_cache().lock() {
            cache.insert(cache_key, digest.clone());
        }
        return Ok(digest);
    }

    sha256_file_hex_uncached(path)
}

fn prepare_runtime_input_wav(config: &TranscribeConfig) -> Result<(), CliError> {
    if config.live_chunked || config.live_stream {
        run_live_capture_session(config)
    } else {
        prepare_input_wav(&config.input_wav)
    }
}

fn input_wav_semantics(config: &TranscribeConfig) -> &'static str {
    if config.live_stream {
        "progressive live capture scratch artifact (materialized into canonical out_wav on success)"
    } else if config.live_chunked {
        "representative live scratch artifact mirrored from canonical out_wav after capture"
    } else {
        "representative offline fixture input path"
    }
}

fn live_capture_output_path(config: &TranscribeConfig) -> &Path {
    if config.live_stream {
        &config.input_wav
    } else {
        &config.out_wav
    }
}

fn live_capture_materialization_paths(config: &TranscribeConfig) -> (&Path, &Path) {
    if config.live_stream {
        (&config.input_wav, &config.out_wav)
    } else {
        (&config.out_wav, &config.input_wav)
    }
}

fn run_live_capture_session(config: &TranscribeConfig) -> Result<(), CliError> {
    let capture_output = live_capture_output_path(config).to_path_buf();
    let live_capture_config = LiveCaptureConfig {
        duration_secs: config.duration_sec,
        output: capture_output.clone(),
        target_rate_hz: config.sample_rate_hz,
        mismatch_policy: LiveCaptureSampleRateMismatchPolicy::AdaptStreamRate,
        callback_contract_mode: LiveCaptureCallbackMode::Warn,
    };
    run_capture_session(&live_capture_config)
        .map_err(|err| CliError::new(format!("live capture session failed: {err}")))?;
    ensure_live_capture_output_exists(&capture_output)?;
    if config.live_stream {
        return Ok(());
    }
    let (materialize_from, materialize_to) = live_capture_materialization_paths(config);
    materialize_out_wav(materialize_from, materialize_to)
}

fn ensure_live_capture_output_exists(path: &Path) -> Result<(), CliError> {
    if !path.is_file() {
        return Err(CliError::new(format!(
            "live capture session completed but output WAV was not materialized at {}",
            display_path(path)
        )));
    }
    Ok(())
}

fn collect_live_capture_continuity_events(config: &TranscribeConfig) -> Vec<ModeDegradationEvent> {
    if !(config.live_chunked || config.live_stream) {
        return Vec::new();
    }

    let telemetry_path = live_capture_telemetry_path_candidates(config)
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| live_capture_telemetry_path(&config.out_wav));
    match load_live_capture_telemetry_signals(&telemetry_path) {
        Ok(signals) => {
            let mut events = Vec::new();
            if signals.restart_count > 0 {
                events.push(ModeDegradationEvent {
                    code: LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE,
                    detail: format!(
                        "near-live capture recovered from {} stream interruption restart(s) under bounded restart policy; continuity may include restart gaps (telemetry={})",
                        signals.restart_count,
                        display_path(&telemetry_path)
                    ),
                });
            }
            if !signals.transport_sources.is_empty() {
                events.push(ModeDegradationEvent {
                    code: LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE,
                    detail: format!(
                        "near-live capture transport reported degradation source(s): {} (telemetry={})",
                        signals.transport_sources.join(","),
                        display_path(&telemetry_path)
                    ),
                });
            }
            if !signals.callback_sources.is_empty() {
                events.push(ModeDegradationEvent {
                    code: LIVE_CAPTURE_CALLBACK_CONTRACT_DEGRADED_CODE,
                    detail: format!(
                        "near-live capture callback contract violations detected: {} (telemetry={})",
                        signals.callback_sources.join(","),
                        display_path(&telemetry_path)
                    ),
                });
            }
            events
        }
        Err(err) => vec![ModeDegradationEvent {
            code: LIVE_CAPTURE_CONTINUITY_UNVERIFIED_CODE,
            detail: format!(
                "near-live continuity status could not be verified: {} (telemetry={})",
                err,
                display_path(&telemetry_path)
            ),
        }],
    }
}

struct LiveCaptureTelemetrySignals {
    restart_count: u64,
    transport_sources: Vec<&'static str>,
    callback_sources: Vec<&'static str>,
}

fn live_capture_telemetry_path(output_wav: &Path) -> PathBuf {
    capture_telemetry_path_for_output(output_wav)
}

fn live_capture_telemetry_path_candidates(config: &TranscribeConfig) -> Vec<PathBuf> {
    let mut candidates = vec![live_capture_telemetry_path(&config.out_wav)];
    if absolutize_candidate(config.input_wav.clone())
        != absolutize_candidate(config.out_wav.clone())
    {
        candidates.push(live_capture_telemetry_path(&config.input_wav));
    }
    candidates
}

fn load_live_capture_telemetry_signals(
    telemetry_path: &Path,
) -> Result<LiveCaptureTelemetrySignals, CliError> {
    let payload = fs::read_to_string(telemetry_path).map_err(|err| {
        CliError::new(format!(
            "failed to read live capture telemetry {}: {err}",
            display_path(telemetry_path)
        ))
    })?;
    let restart_count = extract_json_u64_field(&payload, "restart_count").ok_or_else(|| {
        CliError::new(format!(
            "live capture telemetry {} is missing `restart_count`",
            display_path(telemetry_path)
        ))
    })?;

    let transport_sources = LIVE_CAPTURE_TRANSPORT_SOURCES
        .iter()
        .copied()
        .filter(|source| payload_has_json_string_field_value(&payload, "source", source))
        .collect::<Vec<_>>();
    let callback_sources = LIVE_CAPTURE_CALLBACK_SOURCES
        .iter()
        .copied()
        .filter(|source| payload_has_json_string_field_value(&payload, "source", source))
        .collect::<Vec<_>>();

    Ok(LiveCaptureTelemetrySignals {
        restart_count,
        transport_sources,
        callback_sources,
    })
}

fn payload_has_json_string_field_value(payload: &str, key: &str, value: &str) -> bool {
    payload.contains(&format!("\"{key}\":\"{value}\""))
        || payload.contains(&format!("\"{key}\": \"{value}\""))
}

fn prepare_input_wav(path: &Path) -> Result<(), CliError> {
    if !path.exists() {
        synthesize_input_wav(path)?;
    }
    if !path.is_file() {
        return Err(CliError::new(format!(
            "representative input WAV is not a file: {}",
            display_path(path)
        )));
    }
    Ok(())
}

fn materialize_out_wav(input_wav: &Path, out_wav: &Path) -> Result<(), CliError> {
    let same_output = match (fs::canonicalize(input_wav), fs::canonicalize(out_wav)) {
        (Ok(lhs), Ok(rhs)) => lhs == rhs,
        _ => {
            absolutize_candidate(input_wav.to_path_buf())
                == absolutize_candidate(out_wav.to_path_buf())
        }
    };
    if same_output {
        return Ok(());
    }

    if let Some(parent) = out_wav.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CliError::new(format!(
                "failed to create out-wav directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    fs::copy(input_wav, out_wav).map_err(|err| {
        CliError::new(format!(
            "failed to materialize canonical out-wav {} from input {}: {err}",
            display_path(out_wav),
            display_path(input_wav)
        ))
    })?;
    Ok(())
}

fn synthesize_input_wav(path: &Path) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CliError::new(format!(
                "failed to create representative input directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let bundled_fixture = PathBuf::from("/opt/homebrew/opt/whisper-cpp/share/whisper-cpp/jfk.wav");
    if bundled_fixture.exists() {
        fs::copy(&bundled_fixture, path).map_err(|err| {
            CliError::new(format!(
                "failed to copy representative fixture {} -> {}: {err}",
                bundled_fixture.display(),
                display_path(path)
            ))
        })?;
        return Ok(());
    }

    let temp_aiff = path.with_extension("aiff");
    let say_status = Command::new("say")
        .args([
            "-o",
            &temp_aiff.to_string_lossy(),
            "recordit representative transcription sample",
        ])
        .status()
        .map_err(|err| CliError::new(format!("failed to execute `say`: {err}")))?;
    if !say_status.success() {
        return Err(CliError::new(format!(
            "`say` exited with status {} while generating representative audio",
            say_status
        )));
    }

    let convert_status = Command::new("afconvert")
        .args([
            "-f",
            "WAVE",
            "-d",
            "LEI16@16000",
            &temp_aiff.to_string_lossy(),
            &path.to_string_lossy(),
        ])
        .status()
        .map_err(|err| CliError::new(format!("failed to execute `afconvert`: {err}")))?;
    let _ = fs::remove_file(&temp_aiff);
    if !convert_status.success() {
        return Err(CliError::new(format!(
            "`afconvert` exited with status {} while converting representative audio",
            convert_status
        )));
    }

    Ok(())
}

fn detect_vad_boundaries_from_wav(
    wav_path: &Path,
    threshold: f32,
    min_speech_ms: u32,
    min_silence_ms: u32,
) -> Result<Vec<VadBoundary>, CliError> {
    let mut reader = WavReader::open(wav_path).map_err(|err| {
        CliError::new(format!(
            "failed to open representative WAV {}: {err}",
            display_path(wav_path)
        ))
    })?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err(CliError::new(format!(
            "WAV has invalid channel count: {}",
            display_path(wav_path)
        )));
    }

    let mut normalized_samples = Vec::new();
    match spec.sample_format {
        SampleFormat::Float => {
            for sample in reader.samples::<f32>() {
                normalized_samples.push(
                    sample.map_err(|err| {
                        CliError::new(format!("failed to read float sample: {err}"))
                    })?,
                );
            }
        }
        SampleFormat::Int => {
            if spec.bits_per_sample <= 16 {
                for sample in reader.samples::<i16>() {
                    let value = sample.map_err(|err| {
                        CliError::new(format!("failed to read i16 sample: {err}"))
                    })?;
                    normalized_samples.push(value as f32 / i16::MAX as f32);
                }
            } else {
                let denom = (1i64 << (spec.bits_per_sample.saturating_sub(1) as u32)) as f32;
                for sample in reader.samples::<i32>() {
                    let value = sample.map_err(|err| {
                        CliError::new(format!("failed to read i32 sample: {err}"))
                    })?;
                    normalized_samples.push((value as f32 / denom).clamp(-1.0, 1.0));
                }
            }
        }
    }

    let channels = spec.channels as usize;
    let mut per_channel_levels = (0..channels)
        .map(|idx| {
            (
                vad_channel_label_for_index(idx, channels),
                Vec::with_capacity(normalized_samples.len() / channels + 1),
            )
        })
        .collect::<Vec<_>>();
    for frame in normalized_samples.chunks(channels) {
        for (idx, sample) in frame.iter().enumerate() {
            if let Some((_, levels)) = per_channel_levels.get_mut(idx) {
                levels.push(sample.abs().clamp(0.0, 1.0));
            }
        }
    }

    let total_frames = per_channel_levels
        .first()
        .map(|(_, levels)| levels.len())
        .unwrap_or(0);
    let channel_boundaries = detect_per_channel_vad_boundaries(
        &per_channel_levels,
        spec.sample_rate,
        threshold,
        min_speech_ms,
        min_silence_ms,
    );
    Ok(merge_channel_vad_boundaries(
        &channel_boundaries,
        spec.sample_rate,
        total_frames,
    ))
}

#[cfg(test)]
fn detect_vad_boundaries(
    frame_levels: &[f32],
    sample_rate_hz: u32,
    threshold: f32,
    min_speech_ms: u32,
    min_silence_ms: u32,
) -> Vec<VadBoundary> {
    if frame_levels.is_empty() || sample_rate_hz == 0 {
        return Vec::new();
    }

    let channel_boundaries = detect_per_channel_vad_boundaries(
        &[("merged".to_string(), frame_levels.to_vec())],
        sample_rate_hz,
        threshold,
        min_speech_ms,
        min_silence_ms,
    );
    merge_channel_vad_boundaries(&channel_boundaries, sample_rate_hz, frame_levels.len())
}

fn vad_channel_label_for_index(channel_index: usize, channel_count: usize) -> String {
    if channel_count == 2 {
        return if channel_index == 0 {
            "mic".to_string()
        } else {
            "system".to_string()
        };
    }
    format!("ch{channel_index}")
}

fn detect_per_channel_vad_boundaries(
    channel_levels: &[(String, Vec<f32>)],
    sample_rate_hz: u32,
    threshold: f32,
    min_speech_ms: u32,
    min_silence_ms: u32,
) -> Vec<ChannelVadBoundary> {
    if sample_rate_hz == 0 {
        return Vec::new();
    }
    let mut boundaries = Vec::new();
    for (channel, levels) in channel_levels {
        let mut tracker = IncrementalVadTracker::new(
            channel.clone(),
            sample_rate_hz,
            threshold,
            min_speech_ms,
            min_silence_ms,
        );
        for (idx, level) in levels.iter().copied().enumerate() {
            tracker.observe(idx, level);
        }
        boundaries.extend(tracker.finish(levels.len()));
    }
    boundaries.sort_by(|a, b| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| a.end_ms.cmp(&b.end_ms))
            .then_with(|| a.channel.cmp(&b.channel))
    });
    boundaries
}

fn merge_channel_vad_boundaries(
    channel_boundaries: &[ChannelVadBoundary],
    sample_rate_hz: u32,
    total_frames: usize,
) -> Vec<VadBoundary> {
    if total_frames == 0 || sample_rate_hz == 0 {
        return Vec::new();
    }
    if channel_boundaries.is_empty() {
        return vec![VadBoundary {
            id: 0,
            start_ms: 0,
            end_ms: sample_to_ms(total_frames as u64, sample_rate_hz),
            source: "fallback_full_audio",
        }];
    }

    let mut merged = Vec::new();
    let mut current_start_ms = channel_boundaries[0].start_ms;
    let mut current_end_ms = channel_boundaries[0].end_ms;
    let mut current_source = channel_boundaries[0].source;
    for boundary in channel_boundaries.iter().skip(1) {
        if boundary.start_ms <= current_end_ms {
            current_end_ms = current_end_ms.max(boundary.end_ms);
            if boundary.source == "shutdown_flush" {
                current_source = "shutdown_flush";
            }
        } else {
            merged.push(VadBoundary {
                id: merged.len(),
                start_ms: current_start_ms,
                end_ms: current_end_ms,
                source: current_source,
            });
            current_start_ms = boundary.start_ms;
            current_end_ms = boundary.end_ms;
            current_source = boundary.source;
        }
    }
    merged.push(VadBoundary {
        id: merged.len(),
        start_ms: current_start_ms,
        end_ms: current_end_ms,
        source: current_source,
    });
    merged
}

fn sample_to_ms(sample_idx: u64, sample_rate_hz: u32) -> u64 {
    sample_idx.saturating_mul(1_000) / sample_rate_hz.max(1) as u64
}

fn ms_to_sample_index(ms: u64, sample_rate_hz: u32) -> usize {
    if sample_rate_hz == 0 {
        return 0;
    }
    ((ms as u128 * sample_rate_hz as u128 + 500) / 1_000) as usize
}

fn runtime_job_class_to_pool_job_class(class: RuntimeAsrJobClass) -> LiveAsrJobClass {
    runtime_events::runtime_job_class_to_pool_job_class(class)
}

fn runtime_channel_role(channel: &str) -> &'static str {
    runtime_events::runtime_channel_role(channel)
}

fn runtime_channel_label(
    channel_mode: ChannelMode,
    speaker_labels: &SpeakerLabels,
    channel: &str,
) -> String {
    runtime_events::runtime_channel_label(channel_mode, speaker_labels, channel)
}

fn write_runtime_job_wav(
    path: &Path,
    sample_rate_hz: u32,
    samples: &[f32],
) -> Result<(), CliError> {
    runtime_events::write_runtime_job_wav(path, sample_rate_hz, samples)
}

fn runtime_phase_to_lifecycle_phase(phase: LiveRuntimePhase) -> LiveLifecyclePhase {
    runtime_events::runtime_phase_to_lifecycle_phase(phase)
}

fn lifecycle_from_runtime_output_events(events: &[RuntimeOutputEvent]) -> LiveLifecycleTelemetry {
    runtime_events::lifecycle_from_runtime_output_events(events)
}

fn transcript_events_from_runtime_output_events(
    config: &TranscribeConfig,
    events: &[RuntimeOutputEvent],
) -> Vec<TranscriptEvent> {
    runtime_events::transcript_events_from_runtime_output_events(config, events)
}

fn transcript_event_from_runtime_asr_result(
    channel_mode: ChannelMode,
    speaker_labels: &SpeakerLabels,
    result: &LiveAsrResult,
) -> Option<TranscriptEvent> {
    runtime_events::transcript_event_from_runtime_asr_result(channel_mode, speaker_labels, result)
}

fn vad_boundaries_from_runtime_output_events(events: &[RuntimeOutputEvent]) -> Vec<VadBoundary> {
    runtime_events::vad_boundaries_from_runtime_output_events(events)
}

fn fallback_vad_boundaries_from_events(events: &[TranscriptEvent]) -> Vec<VadBoundary> {
    runtime_events::fallback_vad_boundaries_from_events(events)
}

fn merge_live_transcript_events_for_display(events: Vec<TranscriptEvent>) -> Vec<TranscriptEvent> {
    runtime_events::merge_live_transcript_events_for_display(events)
}

fn live_stream_chunk_queue_telemetry(
    config: &TranscribeConfig,
    runtime_summary: &LiveRuntimeSummary,
    asr_worker_pool: &LiveAsrPoolTelemetry,
) -> LiveChunkQueueTelemetry {
    runtime_events::live_stream_chunk_queue_telemetry(config, runtime_summary, asr_worker_pool)
}

fn channel_transcript_summaries_from_events(
    config: &TranscribeConfig,
    events: &[TranscriptEvent],
) -> Vec<ChannelTranscriptSummary> {
    runtime_events::channel_transcript_summaries_from_events(config, events)
}

fn active_channel_mode_from_transcripts(
    config: &TranscribeConfig,
    channel_transcripts: &[ChannelTranscriptSummary],
) -> ChannelMode {
    runtime_events::active_channel_mode_from_transcripts(config, channel_transcripts)
}

fn stable_terminal_summary_lines(events: &[TranscriptEvent]) -> Vec<String> {
    build_terminal_render_actions(events, TerminalRenderMode::DeterministicNonTty)
        .into_iter()
        .filter(|action| action.kind == TerminalRenderActionKind::StableLine)
        .map(|action| action.line)
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
struct FirstEmitTiming {
    first_any_end_ms: Option<u64>,
    first_partial_end_ms: Option<u64>,
    first_final_end_ms: Option<u64>,
    first_stable_end_ms: Option<u64>,
}

fn first_emit_timing(events: &[TranscriptEvent]) -> FirstEmitTiming {
    let mut timing = FirstEmitTiming::default();
    for event in events {
        let end_ms = event.end_ms;
        if timing.first_any_end_ms.is_none() {
            timing.first_any_end_ms = Some(end_ms);
        }
        if event.event_type == "partial" && timing.first_partial_end_ms.is_none() {
            timing.first_partial_end_ms = Some(end_ms);
        }
        if event.event_type == "final" && timing.first_final_end_ms.is_none() {
            timing.first_final_end_ms = Some(end_ms);
        }
        if is_stable_terminal_event(event.event_type) && timing.first_stable_end_ms.is_none() {
            timing.first_stable_end_ms = Some(end_ms);
        }
        if timing.first_any_end_ms.is_some()
            && timing.first_partial_end_ms.is_some()
            && timing.first_final_end_ms.is_some()
            && timing.first_stable_end_ms.is_some()
        {
            break;
        }
    }
    timing
}

fn json_optional_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn transcript_event_count(events: &[TranscriptEvent], event_type: &str) -> usize {
    events
        .iter()
        .filter(|event| event.event_type == event_type)
        .count()
}

fn build_startup_banner_lines(config: &TranscribeConfig) -> Vec<String> {
    vec![
        format!("runtime_mode={}", config.runtime_mode_label()),
        format!(
            "runtime_mode_taxonomy={}",
            config.runtime_mode_taxonomy_label()
        ),
        format!(
            "runtime_mode_selector={}",
            config.runtime_mode_selector_label()
        ),
        format!("runtime_mode_status={}", config.runtime_mode_status_label()),
        format!("channel_mode_requested={}", config.channel_mode),
        format!("duration_sec={}", config.duration_sec),
        format!("input_wav={}", display_path(&config.input_wav)),
        format!(
            "artifacts=out_wav:{} out_jsonl:{} out_manifest:{}",
            display_path(&config.out_wav),
            display_path(&config.out_jsonl),
            display_path(&config.out_manifest),
        ),
    ]
}

fn session_status(report: &LiveRunReport) -> &'static str {
    if report.trust_notices.is_empty() {
        "ok"
    } else {
        "degraded"
    }
}

fn top_codes<I>(codes: I, limit: usize) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut sorted = codes.into_iter().collect::<Vec<_>>();
    sorted.sort();
    sorted.dedup();
    sorted.into_iter().take(limit).collect()
}

fn top_codes_csv(codes: &[String]) -> String {
    if codes.is_empty() {
        "<none>".to_string()
    } else {
        codes.join("|")
    }
}

fn top_remediation_hints(report: &LiveRunReport, limit: usize) -> Vec<String> {
    let mut hints = BTreeSet::new();
    for notice in &report.trust_notices {
        let guidance = notice.guidance.trim();
        if !guidance.is_empty() {
            hints.insert(guidance.to_string());
        }
    }

    if hints.is_empty() && !report.degradation_events.is_empty() {
        for event in &report.degradation_events {
            hints.insert(format!(
                "inspect degradation code `{}` in the runtime manifest",
                event.code
            ));
            if hints.len() >= limit {
                break;
            }
        }
    }

    hints.into_iter().take(limit).collect()
}

fn remediation_hints_csv(hints: &[String]) -> String {
    if hints.is_empty() {
        "<none>".to_string()
    } else {
        hints.join(" | ")
    }
}

fn top_codes_json(codes: &[String]) -> String {
    codes
        .iter()
        .map(|code| format!("\"{}\"", json_escape(code)))
        .collect::<Vec<_>>()
        .join(",")
}

fn build_live_close_summary_lines(
    config: &TranscribeConfig,
    report: &LiveRunReport,
) -> Vec<String> {
    let trust_top_codes = top_codes(
        report
            .trust_notices
            .iter()
            .map(|notice| notice.code.clone()),
        3,
    );
    let degradation_top_codes = top_codes(
        report
            .degradation_events
            .iter()
            .map(|event| event.code.to_string()),
        3,
    );

    vec![
        format!("session_status={}", session_status(report)),
        format!("duration_sec={}", config.duration_sec),
        format!("channel_mode_requested={}", report.channel_mode),
        format!("channel_mode_active={}", report.active_channel_mode),
        format!(
            "transcript_events=partial:{} final:{} llm_final:{} reconciled_final:{}",
            transcript_event_count(&report.events, "partial"),
            transcript_event_count(&report.events, "final"),
            transcript_event_count(&report.events, "llm_final"),
            transcript_event_count(&report.events, "reconciled_final"),
        ),
        format!(
            "chunk_queue=submitted:{} enqueued:{} dropped_oldest:{} processed:{} pending:{} high_water:{} drain_completed:{}",
            report.chunk_queue.submitted,
            report.chunk_queue.enqueued,
            report.chunk_queue.dropped_oldest,
            report.chunk_queue.processed,
            report.chunk_queue.pending,
            report.chunk_queue.high_water,
            report.chunk_queue.drain_completed,
        ),
        format!(
            "chunk_lag=lag_sample_count:{} lag_p50_ms:{} lag_p95_ms:{} lag_max_ms:{}",
            report.chunk_queue.lag_sample_count,
            report.chunk_queue.lag_p50_ms,
            report.chunk_queue.lag_p95_ms,
            report.chunk_queue.lag_max_ms,
        ),
        format!(
            "trust_notices=count:{} top_codes:{}",
            report.trust_notices.len(),
            top_codes_csv(&trust_top_codes),
        ),
        format!(
            "degradation_events=count:{} top_codes:{}",
            report.degradation_events.len(),
            top_codes_csv(&degradation_top_codes),
        ),
        format!(
            "cleanup_queue=enabled:{} submitted:{} enqueued:{} dropped_queue_full:{} processed:{} succeeded:{} timed_out:{} failed:{} retry_attempts:{} pending:{} drain_completed:{}",
            report.cleanup_queue.enabled,
            report.cleanup_queue.submitted,
            report.cleanup_queue.enqueued,
            report.cleanup_queue.dropped_queue_full,
            report.cleanup_queue.processed,
            report.cleanup_queue.succeeded,
            report.cleanup_queue.timed_out,
            report.cleanup_queue.failed,
            report.cleanup_queue.retry_attempts,
            report.cleanup_queue.pending,
            report.cleanup_queue.drain_completed,
        ),
        format!(
            "artifacts=out_wav:{} out_jsonl:{} out_manifest:{}",
            display_path(&config.out_wav),
            display_path(&config.out_jsonl),
            display_path(&config.out_manifest),
        ),
    ]
}

fn print_live_report(config: &TranscribeConfig, report: &LiveRunReport, concise_only: bool) {
    let model_checksum = model_checksum_info(Some(&ResolvedModelPath {
        path: report.resolved_model_path.clone(),
        source: report.resolved_model_source.clone(),
    }));
    let remediation_hints = top_remediation_hints(report, 3);

    println!();
    println!("Runtime result");
    println!("  runtime_mode: {}", config.runtime_mode_label());
    println!(
        "  runtime_mode_taxonomy: {}",
        config.runtime_mode_taxonomy_label()
    );
    println!(
        "  runtime_mode_selector: {}",
        config.runtime_mode_selector_label()
    );
    println!(
        "  runtime_mode_status: {}",
        config.runtime_mode_status_label()
    );
    println!("  generated_at_utc: {}", report.generated_at_utc);
    println!("  backend: {}", report.backend_id);
    println!(
        "  asr_model_resolved: {}",
        report.resolved_model_path.display()
    );
    println!("  asr_model_source: {}", report.resolved_model_source);
    println!("  asr_model_checksum_sha256: {}", model_checksum.sha256);
    println!("  asr_model_checksum_status: {}", model_checksum.status);
    println!("  run_status: {}", session_status(report));
    println!(
        "  remediation_hints: {}",
        remediation_hints_csv(&remediation_hints)
    );
    println!("  channel_mode_requested: {}", report.channel_mode);
    println!("  channel_mode_active: {}", report.active_channel_mode);
    println!("  close_summary:");
    for line in build_live_close_summary_lines(config, report) {
        println!("    {line}");
    }
    if concise_only {
        println!(
            "  telemetry_manifest: {}",
            display_path(&config.out_manifest)
        );
        println!("  telemetry_jsonl: {}", display_path(&config.out_jsonl));
        return;
    }

    let per_channel_defaults = reconstruct_transcript_per_channel(&report.events);
    println!(
        "  lifecycle: current_phase={} ready_for_transcripts={} transition_count={}",
        report.lifecycle.current_phase,
        report.lifecycle.ready_for_transcripts,
        report.lifecycle.transitions.len()
    );
    println!("  lifecycle_transitions:");
    for transition in &report.lifecycle.transitions {
        println!(
            "    - phase={} entered_at_utc={} detail={}",
            transition.phase, transition.entered_at_utc, transition.detail
        );
    }
    println!("  transcript_default_line_format: [MM:SS.mmm-MM:SS.mmm] <channel>: <text>");
    println!(
        "  transcript_overlap_policy: adjacent cross-channel finals within {OVERLAP_WINDOW_MS}ms keep sort order and add overlap annotation"
    );
    println!("  transcript_text:");
    for line in report.transcript_text.lines() {
        println!("    {line}");
    }
    println!("  transcript_per_channel:");
    for channel in &per_channel_defaults {
        println!("    - channel={}", channel.channel);
        for line in channel.text.lines() {
            println!("      {line}");
        }
    }
    println!("  channel_transcripts:");
    for channel in &report.channel_transcripts {
        println!(
            "    - role={} label={} text={}",
            channel.role, channel.label, channel.text
        );
    }
    println!(
        "  benchmark_wall_ms: p50={:.2} p95={:.2} (runs={})",
        report.benchmark.wall_ms_p50, report.benchmark.wall_ms_p95, report.benchmark.run_count
    );
    println!(
        "  slo_check: partial_p95<=1500ms={} final_p95<=2500ms={}",
        report.benchmark.partial_slo_met, report.benchmark.final_slo_met
    );
    println!(
        "  benchmark_summary_csv: {}",
        report.benchmark_summary_csv.display()
    );
    println!(
        "  benchmark_runs_csv: {}",
        report.benchmark_runs_csv.display()
    );
    println!("  input_wav_semantics: {}", input_wav_semantics(config));
    println!("  out_wav_semantics: {OUT_WAV_SEMANTICS}");
    println!("  vad_boundaries: {}", report.vad_boundaries.len());
    for boundary in &report.vad_boundaries {
        println!(
            "    - id={} start_ms={} end_ms={} source={}",
            boundary.id, boundary.start_ms, boundary.end_ms, boundary.source
        );
    }
    println!("  terminal_transcript_stream:");
    let live_mode = report.chunk_queue.enabled;
    let stable_terminal_lines = stable_terminal_summary_lines(&report.events);
    if live_mode {
        println!(
            "    <rendered during active runtime; summary suppresses duplicate stable-line replay>"
        );
    } else {
        if stable_terminal_lines.is_empty() {
            println!("    <no stable transcript events>");
        } else {
            for line in stable_terminal_lines {
                println!("    {line}");
            }
        }
    }
    println!("  degradation_events: {}", report.degradation_events.len());
    for event in &report.degradation_events {
        println!("    - code={} detail={}", event.code, event.detail);
    }
    println!("  trust_notices: {}", report.trust_notices.len());
    if report.trust_notices.is_empty() {
        println!("    - none (runtime trust posture: nominal)");
    } else {
        println!("  degraded_mode_notices:");
        for notice in &report.trust_notices {
            println!(
                "    - [{}] code={} cause={} | impact={} | next={}",
                notice.severity, notice.code, notice.cause, notice.impact, notice.guidance
            );
        }
    }
    println!(
        "  reconciliation_matrix: required={} applied={} trigger_count={} trigger_codes={}",
        report.reconciliation.required,
        report.reconciliation.applied,
        report.reconciliation.triggers.len(),
        report.reconciliation.trigger_codes_csv()
    );
    println!(
        "  asr_worker_pool: prewarm_ok={} submitted={} enqueued={} dropped_queue_full={} processed={} succeeded={} failed={} retry_attempts={} temp_audio_deleted={} temp_audio_retained={}",
        report.asr_worker_pool.prewarm_ok,
        report.asr_worker_pool.submitted,
        report.asr_worker_pool.enqueued,
        report.asr_worker_pool.dropped_queue_full,
        report.asr_worker_pool.processed,
        report.asr_worker_pool.succeeded,
        report.asr_worker_pool.failed,
        report.asr_worker_pool.retry_attempts,
        report.asr_worker_pool.temp_audio_deleted,
        report.asr_worker_pool.temp_audio_retained
    );
    println!(
        "  chunk_queue: enabled={} max_queue={} submitted={} enqueued={} dropped_oldest={} processed={} pending={} high_water={} drain_completed={} lag_sample_count={} lag_p50_ms={} lag_p95_ms={} lag_max_ms={}",
        report.chunk_queue.enabled,
        report.chunk_queue.max_queue,
        report.chunk_queue.submitted,
        report.chunk_queue.enqueued,
        report.chunk_queue.dropped_oldest,
        report.chunk_queue.processed,
        report.chunk_queue.pending,
        report.chunk_queue.high_water,
        report.chunk_queue.drain_completed,
        report.chunk_queue.lag_sample_count,
        report.chunk_queue.lag_p50_ms,
        report.chunk_queue.lag_p95_ms,
        report.chunk_queue.lag_max_ms
    );
    println!(
        "  cleanup_queue: enabled={} submitted={} enqueued={} dropped_queue_full={} processed={} succeeded={} timed_out={} failed={} retry_attempts={} pending={} drain_completed={}",
        report.cleanup_queue.enabled,
        report.cleanup_queue.submitted,
        report.cleanup_queue.enqueued,
        report.cleanup_queue.dropped_queue_full,
        report.cleanup_queue.processed,
        report.cleanup_queue.succeeded,
        report.cleanup_queue.timed_out,
        report.cleanup_queue.failed,
        report.cleanup_queue.retry_attempts,
        report.cleanup_queue.pending,
        report.cleanup_queue.drain_completed
    );
    println!("  jsonl_written: true");
    println!("  manifest_written: true");
}

fn emit_latest_lifecycle_transition_jsonl(
    stream: &mut RuntimeJsonlStream,
    lifecycle: &LiveLifecycleTelemetry,
) -> Result<(), CliError> {
    artifacts::emit_latest_lifecycle_transition_jsonl(stream, lifecycle)
}

fn ensure_runtime_jsonl_parent(path: &Path) -> Result<(), CliError> {
    artifacts::ensure_runtime_jsonl_parent(path)
}

fn jsonl_vad_boundary_line(boundary: &VadBoundary, config: &TranscribeConfig) -> String {
    artifacts::jsonl_vad_boundary_line(boundary, config)
}

fn jsonl_transcript_event_line(
    event: &TranscriptEvent,
    backend_id: &str,
    vad_boundary_count: usize,
) -> String {
    artifacts::jsonl_transcript_event_line(event, backend_id, vad_boundary_count)
}

fn jsonl_mode_degradation_line(
    requested_mode: ChannelMode,
    active_mode: ChannelMode,
    degradation: &ModeDegradationEvent,
) -> String {
    artifacts::jsonl_mode_degradation_line(requested_mode, active_mode, degradation)
}

fn jsonl_trust_notice_line(notice: &TrustNotice) -> String {
    artifacts::jsonl_trust_notice_line(notice)
}

fn jsonl_lifecycle_phase_line(index: usize, transition: &LiveLifecycleTransition) -> String {
    artifacts::jsonl_lifecycle_phase_line(index, transition)
}

fn jsonl_reconciliation_matrix_line(reconciliation: &ReconciliationMatrix) -> String {
    artifacts::jsonl_reconciliation_matrix_line(reconciliation)
}

fn jsonl_asr_worker_pool_line(asr_worker_pool: &LiveAsrPoolTelemetry) -> String {
    artifacts::jsonl_asr_worker_pool_line(asr_worker_pool)
}

fn jsonl_chunk_queue_line(chunk_queue: &LiveChunkQueueTelemetry) -> String {
    artifacts::jsonl_chunk_queue_line(chunk_queue)
}

fn jsonl_cleanup_queue_line(cleanup_queue: &CleanupQueueTelemetry) -> String {
    artifacts::jsonl_cleanup_queue_line(cleanup_queue)
}

fn write_runtime_jsonl(config: &TranscribeConfig, report: &LiveRunReport) -> Result<(), CliError> {
    artifacts::write_runtime_jsonl(config, report)
}

fn write_runtime_manifest(
    config: &TranscribeConfig,
    report: &LiveRunReport,
) -> Result<(), CliError> {
    artifacts::write_runtime_manifest(config, report)
}

fn replay_timeline(path: &Path) -> Result<(), CliError> {
    let content = fs::read_to_string(path).map_err(|err| {
        CliError::new(format!(
            "failed to read replay JSONL {}: {err}",
            display_path(path)
        ))
    })?;

    let mut events = Vec::new();
    let mut trust_notices = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(event_type) = extract_json_string_field(trimmed, "event_type") else {
            continue;
        };
        if event_type == "trust_notice" {
            if let Some(notice) = parse_trust_notice(trimmed) {
                trust_notices.push(notice);
            }
            continue;
        }
        if let Some(event) = parse_replay_transcript_event(trimmed, line_no + 1)? {
            events.push(event);
        }
    }

    let events = merge_transcript_events(events);

    println!("Replay timeline");
    println!("  source_jsonl: {}", display_path(path));
    println!("  events: {}", events.len());
    for event in &events {
        println!(
            "  {} channel={} [{}-{}ms] {}",
            event.event_type, event.channel, event.start_ms, event.end_ms, event.text
        );
    }
    println!("  trust_notices: {}", trust_notices.len());
    for notice in &trust_notices {
        println!(
            "    - [{}] {} | impact={} | next={}",
            notice.severity, notice.cause, notice.impact, notice.guidance
        );
    }

    let reconstructed = reconstruct_transcript(&events);
    let per_channel = reconstruct_transcript_per_channel(&events);
    println!();
    println!("Readable transcript (default merged format)");
    for line in reconstructed.lines() {
        println!("  {line}");
    }
    println!();
    println!("Readable transcript (per-channel defaults)");
    for channel in &per_channel {
        println!("  [{}]", channel.channel);
        for line in channel.text.lines() {
            println!("    {line}");
        }
    }
    Ok(())
}

fn parse_replay_transcript_event(
    line: &str,
    line_no: usize,
) -> Result<Option<TranscriptEvent>, CliError> {
    let Some(event_type) = extract_json_string_field(line, "event_type") else {
        return Ok(None);
    };
    if event_type != "partial"
        && event_type != "final"
        && event_type != "llm_final"
        && event_type != "reconciled_final"
    {
        return Ok(None);
    }

    let segment_id = extract_json_string_field(line, "segment_id").ok_or_else(|| {
        CliError::new(format!(
            "invalid replay line {}: missing segment_id",
            line_no
        ))
    })?;
    let channel =
        extract_json_string_field(line, "channel").unwrap_or_else(|| "merged".to_string());
    let text = extract_json_string_field(line, "text").unwrap_or_default();
    let start_ms = extract_json_u64_field(line, "start_ms").ok_or_else(|| {
        CliError::new(format!("invalid replay line {}: missing start_ms", line_no))
    })?;
    let end_ms = extract_json_u64_field(line, "end_ms")
        .ok_or_else(|| CliError::new(format!("invalid replay line {}: missing end_ms", line_no)))?;
    let event_name = match event_type.as_str() {
        "partial" => "partial",
        "llm_final" => "llm_final",
        "reconciled_final" => "reconciled_final",
        _ => "final",
    };
    Ok(Some(TranscriptEvent {
        event_type: event_name,
        channel,
        segment_id,
        start_ms,
        end_ms,
        text,
        source_final_segment_id: extract_json_string_field(line, "source_final_segment_id"),
    }))
}

fn parse_trust_notice(line: &str) -> Option<TrustNotice> {
    Some(TrustNotice {
        code: extract_json_string_field(line, "code")?,
        severity: extract_json_string_field(line, "severity")?,
        cause: extract_json_string_field(line, "cause")?,
        impact: extract_json_string_field(line, "impact")?,
        guidance: extract_json_string_field(line, "guidance")?,
    })
}

fn extract_json_string_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let bytes = line.as_bytes();
    let mut start = line.find(&needle)? + needle.len();
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= bytes.len() || bytes[start] != b':' {
        return None;
    }
    start += 1;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= bytes.len() || bytes[start] != b'"' {
        return None;
    }
    start += 1;
    let mut escaped = false;
    let mut result = String::new();
    for ch in line[start..].chars() {
        if escaped {
            let decoded = match ch {
                '"' => '"',
                '\\' => '\\',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            };
            result.push(decoded);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            return Some(result);
        }
        result.push(ch);
    }
    None
}

fn extract_json_u64_field(line: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{}\":", key);
    let start = line.find(&needle)? + needle.len();
    let mut digits = String::new();
    for ch in line[start..].chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if digits.is_empty() {
            continue;
        }
        break;
    }
    digits.parse::<u64>().ok()
}

#[allow(dead_code)]
pub(crate) fn main() -> ExitCode {
    run_with_parse_outcome(parse_args())
}

#[allow(dead_code)]
pub(crate) fn run_with_args(args: impl Iterator<Item = String>) -> ExitCode {
    run_with_args_in_operator_mode(args, false)
}

#[allow(dead_code)]
pub(crate) fn run_with_args_in_operator_mode(
    args: impl Iterator<Item = String>,
    concise_operator_mode: bool,
) -> ExitCode {
    run_with_parse_outcome_with_render_mode(parse_args_from(args), concise_operator_mode)
}

fn print_failed_status_hint(remediation_hint: &str) {
    eprintln!("run_status=failed");
    eprintln!("remediation_hint={remediation_hint}");
}

fn run_with_parse_outcome(parse_outcome: Result<ParseOutcome, CliError>) -> ExitCode {
    run_with_parse_outcome_with_render_mode(parse_outcome, false)
}

fn run_with_parse_outcome_with_render_mode(
    parse_outcome: Result<ParseOutcome, CliError>,
    concise_operator_mode: bool,
) -> ExitCode {
    match parse_outcome {
        Ok(ParseOutcome::Help) => {
            println!("{HELP_TEXT}");
            ExitCode::SUCCESS
        }
        Ok(ParseOutcome::Config(config)) => {
            if config.preflight {
                match run_preflight(&config) {
                    Ok(report) => {
                        print_preflight_report(&report);
                        if let Err(err) = write_preflight_manifest(&config, &report) {
                            eprintln!("error: failed writing preflight manifest: {err}");
                            print_failed_status_hint(
                                "verify the manifest output path is writable, then rerun `recordit preflight --mode live`.",
                            );
                            return ExitCode::from(2);
                        }
                        match report.overall_status() {
                            CheckStatus::Fail => ExitCode::from(2),
                            _ => ExitCode::SUCCESS,
                        }
                    }
                    Err(err) => {
                        eprintln!("error: preflight failed unexpectedly: {err}");
                        print_failed_status_hint(
                            "rerun `recordit preflight --mode live` to inspect prerequisite failures and remediation columns.",
                        );
                        ExitCode::from(2)
                    }
                }
            } else {
                if config.model_doctor {
                    match run_model_doctor(&config) {
                        Ok(report) => {
                            print_model_doctor_report(&report);
                            return match report.overall_status() {
                                CheckStatus::Fail => ExitCode::from(2),
                                _ => ExitCode::SUCCESS,
                            };
                        }
                        Err(err) => {
                            eprintln!("error: model doctor failed unexpectedly: {err}");
                            print_failed_status_hint(
                                "run `recordit doctor --format json` and verify `--model` or `RECORDIT_ASR_MODEL`.",
                            );
                            return ExitCode::from(2);
                        }
                    }
                }

                if let Some(replay_path) = &config.replay_jsonl {
                    match replay_timeline(replay_path) {
                        Ok(()) => return ExitCode::SUCCESS,
                        Err(err) => {
                            eprintln!("error: replay failed: {err}");
                            print_failed_status_hint(
                                "verify the replay JSONL path exists and rerun `recordit replay --jsonl <path>`.",
                            );
                            return ExitCode::from(2);
                        }
                    }
                }

                config.print_summary(concise_operator_mode);
                match run_runtime_pipeline(&config) {
                    Ok(run_report) => {
                        print_live_report(&config, &run_report, concise_operator_mode);
                        ExitCode::SUCCESS
                    }
                    Err(err) => {
                        eprintln!("error: runtime execution failed: {err}");
                        print_failed_status_hint(
                            "rerun `recordit preflight --mode live` and inspect the manifest trust/degradation fields for the next action.",
                        );
                        ExitCode::from(2)
                    }
                }
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!();
            eprintln!("Run `transcribe-live --help` to see the supported contract.");
            eprintln!("For the canonical operator path, use `recordit run --mode live`.");
            print_failed_status_hint(
                "run `recordit --help` or `recordit run --mode live` for the canonical operator path.",
            );
            ExitCode::from(2)
        }
    }
}

#[allow(dead_code)]
fn parse_args() -> Result<ParseOutcome, CliError> {
    cli_parse::parse_args()
}

#[allow(dead_code)]
fn parse_args_from(args: impl Iterator<Item = String>) -> Result<ParseOutcome, CliError> {
    cli_parse::parse_args_from(args)
}

fn validate_output_path(flag: &str, path: &Path) -> Result<(), CliError> {
    if path.as_os_str().is_empty() {
        return Err(CliError::new(format!("`{flag}` cannot be empty")));
    }

    if let Some(parent) = path.parent() {
        if parent.exists() && !parent.is_dir() {
            return Err(CliError::new(format!(
                "`{flag}` points into `{}` but that parent exists and is not a directory",
                parent.display()
            )));
        }
    }

    Ok(())
}

fn display_path(path: &Path) -> String {
    if path.is_absolute() {
        return path.display().to_string();
    }

    match env::current_dir() {
        Ok(cwd) => cwd.join(path).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

fn run_model_doctor(config: &TranscribeConfig) -> Result<PreflightReport, CliError> {
    let mut checks = Vec::new();
    checks.push(check_backend_runtime(config.asr_backend));

    match validate_model_path_for_backend(config) {
        Ok(resolved) => {
            let expected_kind = expected_model_kind(config.asr_backend);
            checks.push(PreflightCheck::pass(
                "model_path",
                format!(
                    "model path resolved: {} via {} (expected {expected_kind} for backend {})",
                    display_path(&resolved.path),
                    resolved.source,
                    config.asr_backend,
                ),
            ));
            checks.push(check_model_asset_readability(&resolved));
        }
        Err(err) => {
            checks.push(PreflightCheck::fail(
                "model_path",
                err.to_string(),
                "Pass --asr-model, set RECORDIT_ASR_MODEL, or install the backend default asset in the documented location.",
            ));
            checks.push(PreflightCheck::fail(
                "model_readability",
                "skipped because model_path did not validate".to_string(),
                "Fix model_path first, then rerun --model-doctor.",
            ));
        }
    }

    let generated_at_utc = command_stdout("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(PreflightReport {
        generated_at_utc,
        checks,
    })
}

fn check_model_asset_readability(resolved: &ResolvedModelPath) -> PreflightCheck {
    if resolved.path.is_file() {
        return match File::open(&resolved.path) {
            Ok(_) => PreflightCheck::pass(
                "model_readability",
                format!("model file is readable: {}", display_path(&resolved.path)),
            ),
            Err(err) => PreflightCheck::fail(
                "model_readability",
                format!(
                    "cannot read model file {}: {err}",
                    display_path(&resolved.path)
                ),
                "Fix file permissions or pass a different readable model path.",
            ),
        };
    }

    if resolved.path.is_dir() {
        return match fs::read_dir(&resolved.path) {
            Ok(_) => PreflightCheck::pass(
                "model_readability",
                format!(
                    "model directory is readable: {}",
                    display_path(&resolved.path)
                ),
            ),
            Err(err) => PreflightCheck::fail(
                "model_readability",
                format!(
                    "cannot read model directory {}: {err}",
                    display_path(&resolved.path)
                ),
                "Fix directory permissions or pass a different readable model path.",
            ),
        };
    }

    PreflightCheck::fail(
        "model_readability",
        format!(
            "model path is neither a file nor directory: {}",
            display_path(&resolved.path)
        ),
        "Use a readable file/directory path matching backend expectations.",
    )
}

fn run_preflight(config: &TranscribeConfig) -> Result<PreflightReport, CliError> {
    let mut checks = Vec::new();
    checks.push(check_model_path(config));
    checks.push(check_output_target("out_wav", &config.out_wav));
    checks.push(check_output_target("out_jsonl", &config.out_jsonl));
    checks.push(check_output_target("out_manifest", &config.out_manifest));
    checks.push(check_sample_rate(config.sample_rate_hz));
    checks.push(check_screen_capture_access());
    checks.push(check_microphone_stream(config.sample_rate_hz));
    checks.push(check_backend_runtime(config.asr_backend));

    let generated_at_utc = command_stdout("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(PreflightReport {
        generated_at_utc,
        checks,
    })
}

fn check_model_path(config: &TranscribeConfig) -> PreflightCheck {
    match validate_model_path_for_backend(config) {
        Ok(resolved) => {
            let expected_kind = expected_model_kind(config.asr_backend);
            PreflightCheck::pass(
                "model_path",
                format!(
                    "model path resolved: {} via {} (expected {expected_kind} for backend {})",
                    display_path(&resolved.path),
                    resolved.source,
                    config.asr_backend,
                ),
            )
        }
        Err(err) => PreflightCheck::fail(
            "model_path",
            err.to_string(),
            "Pass --asr-model, set RECORDIT_ASR_MODEL, or install the backend default asset in the documented location.",
        ),
    }
}

fn check_output_target(id: &'static str, path: &Path) -> PreflightCheck {
    let absolute = display_path(path);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    if path.exists() && path.is_dir() {
        return PreflightCheck::fail(
            id,
            format!("target path is a directory: {absolute}"),
            "Provide a file path, not a directory.",
        );
    }

    if let Err(err) = fs::create_dir_all(&parent) {
        return PreflightCheck::fail(
            id,
            format!("cannot create parent directory {}: {err}", parent.display()),
            "Choose an output location in a writable directory.",
        );
    }

    let probe = parent.join(format!(
        ".recordit-preflight-write-{}-{}",
        id,
        std::process::id()
    ));
    match File::create(&probe).and_then(|mut file| file.write_all(b"ok")) {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            PreflightCheck::pass(id, format!("writable output target: {absolute}"))
        }
        Err(err) => PreflightCheck::fail(
            id,
            format!("cannot write under {}: {err}", parent.display()),
            "Choose an output path in a writable directory.",
        ),
    }
}

fn check_sample_rate(sample_rate_hz: u32) -> PreflightCheck {
    if sample_rate_hz == 48_000 {
        return PreflightCheck::pass("sample_rate", "sample rate is 48000 Hz");
    }

    PreflightCheck::warn(
        "sample_rate",
        format!("non-default sample rate configured: {sample_rate_hz} Hz"),
        "Use --sample-rate 48000 unless you intentionally need a different rate.",
    )
}

fn check_screen_capture_access() -> PreflightCheck {
    let content = match SCShareableContent::get() {
        Ok(content) => content,
        Err(err) => {
            return PreflightCheck::fail(
                "screen_capture_access",
                format!("failed to query ScreenCaptureKit content: {err}"),
                "Grant Screen Recording permission and ensure at least one active display.",
            );
        }
    };

    let displays = content.displays();
    if displays.is_empty() {
        return PreflightCheck::fail(
            "display_availability",
            "ScreenCaptureKit returned no displays".to_string(),
            "Connect/enable a display and retry. Closed-lid headless mode is unsupported.",
        );
    }

    PreflightCheck::pass(
        "screen_capture_access",
        format!(
            "ScreenCaptureKit access OK; displays available={}",
            displays.len()
        ),
    )
}

fn check_microphone_stream(sample_rate_hz: u32) -> PreflightCheck {
    let content = match SCShareableContent::get() {
        Ok(content) => content,
        Err(err) => {
            return PreflightCheck::fail(
                "microphone_access",
                format!("cannot initialize microphone preflight (shareable content error): {err}"),
                "Grant Screen Recording first, then rerun preflight.",
            );
        }
    };

    let displays = content.displays();
    if displays.is_empty() {
        return PreflightCheck::fail(
            "microphone_access",
            "cannot run microphone preflight without an active display".to_string(),
            "Connect/enable a display and rerun preflight.",
        );
    }

    let filter = SCContentFilter::create()
        .with_display(&displays[0])
        .with_excluding_windows(&[])
        .build();

    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(false)
        .with_captures_microphone(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(sample_rate_hz as i32)
        .with_channel_count(1);

    let queue = DispatchQueue::new(
        "com.recordit.transcribe.preflight",
        DispatchQoS::UserInteractive,
    );
    let (tx, rx) = sync_channel::<()>(1);

    let mut stream = SCStream::new(&filter, &config);
    let tx_mic = tx.clone();
    if stream
        .add_output_handler_with_queue(
            move |_sample, _kind| {
                let _ = tx_mic.try_send(());
            },
            SCStreamOutputType::Microphone,
            Some(&queue),
        )
        .is_none()
    {
        return PreflightCheck::fail(
            "microphone_access",
            "failed to register microphone output handler".to_string(),
            "Retry preflight; if it persists, restart the app/session.",
        );
    }

    if let Err(err) = stream.start_capture() {
        return PreflightCheck::fail(
            "microphone_access",
            format!("failed to start microphone capture: {err}"),
            "Grant Microphone permission and verify an input device is connected and enabled.",
        );
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut observed_mic = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(_) => {
                observed_mic = true;
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let stop_result = stream.stop_capture();
    if let Err(err) = stop_result {
        return PreflightCheck::warn(
            "microphone_access",
            format!("microphone stream started but stop_capture reported: {err}"),
            "Retry preflight; if repeated, restart the app/session.",
        );
    }

    if observed_mic {
        PreflightCheck::pass(
            "microphone_access",
            "observed at least one microphone sample buffer".to_string(),
        )
    } else {
        PreflightCheck::fail(
            "microphone_access",
            "no microphone sample buffer observed within 2s".to_string(),
            "Grant Microphone permission, unmute/select input device, and speak briefly during preflight.",
        )
    }
}

fn check_backend_runtime(backend: AsrBackend) -> PreflightCheck {
    let tool_name = match backend {
        AsrBackend::WhisperCpp => "whisper-cli",
        AsrBackend::WhisperKit => "whisperkit-cli",
        AsrBackend::Moonshine => "moonshine",
    };

    match command_stdout("which", &[tool_name]) {
        Ok(path) => PreflightCheck::pass(
            "backend_runtime",
            format!("detected backend helper binary `{tool_name}` at {path}"),
        ),
        Err(_) => PreflightCheck::warn(
            "backend_runtime",
            format!("backend helper binary `{tool_name}` not found in PATH"),
            "Install backend tooling or keep using Rust-native integration once wired.",
        ),
    }
}

fn print_preflight_report(report: &PreflightReport) {
    let mut pass_count = 0usize;
    let mut warn_count = 0usize;
    let mut fail_count = 0usize;

    println!("Transcribe-live preflight diagnostics");
    println!("  generated_at_utc: {}", report.generated_at_utc);
    println!("  overall_status: {}", report.overall_status());
    println!();
    println!("id\tstatus\tdetail\tremediation");

    for check in &report.checks {
        match check.status {
            CheckStatus::Pass => pass_count += 1,
            CheckStatus::Warn => warn_count += 1,
            CheckStatus::Fail => fail_count += 1,
        }
        println!(
            "{}\t{}\t{}\t{}",
            check.id,
            check.status,
            clean_field(&check.detail),
            clean_field(check.remediation.as_deref().unwrap_or("-")),
        );
    }

    println!();
    println!(
        "summary\t{}\tpass={}\twarn={}\tfail={}",
        report.overall_status(),
        pass_count,
        warn_count,
        fail_count
    );
}

fn print_model_doctor_report(report: &PreflightReport) {
    let mut pass_count = 0usize;
    let mut warn_count = 0usize;
    let mut fail_count = 0usize;

    println!("Transcribe-live model doctor");
    println!("  generated_at_utc: {}", report.generated_at_utc);
    println!("  overall_status: {}", report.overall_status());
    println!();
    println!("id\tstatus\tdetail\tremediation");

    for check in &report.checks {
        match check.status {
            CheckStatus::Pass => pass_count += 1,
            CheckStatus::Warn => warn_count += 1,
            CheckStatus::Fail => fail_count += 1,
        }
        println!(
            "{}\t{}\t{}\t{}",
            check.id,
            check.status,
            clean_field(&check.detail),
            clean_field(check.remediation.as_deref().unwrap_or("-")),
        );
    }

    println!();
    println!(
        "summary\t{}\tpass={}\twarn={}\tfail={}",
        report.overall_status(),
        pass_count,
        warn_count,
        fail_count
    );
}

fn write_preflight_manifest(
    config: &TranscribeConfig,
    report: &PreflightReport,
) -> Result<(), CliError> {
    artifacts::write_preflight_manifest(config, report)
}

fn io_to_cli(err: std::io::Error) -> CliError {
    CliError::new(format!("manifest write error: {err}"))
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn clean_field(value: &str) -> String {
    value
        .replace('\t', " ")
        .replace('\n', " ")
        .replace('\r', " ")
}

fn runtime_timestamp_utc() -> String {
    command_stdout("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_else(|_| "unknown".to_string())
}

fn command_stdout(program: &str, args: &[&str]) -> Result<String, CliError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| CliError::new(format!("failed to execute `{program}`: {err}")))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "`{program}` exited with status {}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        AsrBackend, AsrWorkClass, AsrWorkItem, BenchmarkSummary, CaptureChunk, CaptureEvent,
        CaptureSink, ChannelMode, ChannelVadBoundary, CheckStatus, CleanupAttemptOutcome,
        CleanupQueueTelemetry, CleanupTaskStatus, CollectingRuntimeOutputSink,
        FinalBufferingTelemetry, HELP_TEXT, IncrementalVadTracker,
        LIVE_CAPTURE_CALLBACK_CONTRACT_DEGRADED_CODE, LIVE_CAPTURE_CONTINUITY_UNVERIFIED_CODE,
        LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE, LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE,
        LIVE_CHUNK_QUEUE_BACKPRESSURE_SEVERE_CODE, LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE,
        LiveAsrExecutor, LiveAsrJob, LiveAsrJobClass, LiveAsrPoolConfig, LiveAsrPoolTelemetry,
        LiveCaptureCallbackMode, LiveCaptureConfig, LiveCaptureSampleRateMismatchPolicy,
        LiveChunkQueueTelemetry, LiveLifecyclePhase, LiveLifecycleTelemetry, LiveRunReport,
        ModeDegradationEvent, ParseOutcome, PreflightCheck, PreflightReport,
        RECONCILIATION_APPLIED_CODE, ReconciliationMatrix, ResolvedModelPath,
        RuntimeExecutionBranch, RuntimeJsonlStream, RuntimeOutputSink, TempAudioPolicy,
        TerminalRenderActionKind, TerminalRenderMode, TranscribeConfig, TranscriptEvent,
        VadBoundary, build_live_chunked_events_with_queue, build_live_close_summary_lines,
        build_reconciliation_events, build_reconciliation_matrix, build_rolling_chunk_windows,
        build_startup_banner_lines, build_targeted_reconciliation_events,
        build_terminal_render_actions, build_transcript_events, build_trust_notices,
        bundled_backend_program_from_exe, chunk_queue_backpressure_is_severe,
        collect_live_capture_continuity_events, detect_per_channel_vad_boundaries,
        detect_vad_boundaries, emit_latest_lifecycle_transition_jsonl, input_wav_semantics,
        live_capture_materialization_paths, live_capture_output_path,
        live_capture_telemetry_path_candidates, live_stream_chunk_queue_telemetry,
        live_stream_vad_thresholds_per_mille, live_terminal_render_actions, materialize_out_wav,
        merge_channel_vad_boundaries, merge_transcript_events, model_checksum_info,
        parse_args_from, parse_replay_transcript_event, parse_trust_notice, reconstruct_transcript,
        reconstruct_transcript_per_channel, remediation_hints_csv, replay_timeline,
        resolve_backend_program, resolve_model_path, run_cleanup_queue_with, run_live_chunk_queue,
        run_streaming_capture_session, runtime_mode_compatibility_matrix,
        select_runtime_execution_branch, top_remediation_hints,
        transcript_events_from_runtime_output_events, write_preflight_manifest,
        write_runtime_jsonl, write_runtime_manifest,
    };
    use hound::{SampleFormat, WavSpec, WavWriter};
    use recordit::live_stream_runtime::{
        LiveAsrJobClass as RuntimeAsrJobClass, LiveAsrJobSpec as RuntimeAsrJobSpec,
        LiveAsrResult as RuntimeAsrResult, LiveRuntimePhase, LiveRuntimeSummary,
        RuntimeOutputEvent,
    };
    use serde_json::Value;
    use std::env;
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct MockLiveAsrExecutor {
        prewarm_calls: AtomicUsize,
        transcribe_calls: AtomicUsize,
        sleep_ms: u64,
    }

    impl LiveAsrExecutor for MockLiveAsrExecutor {
        fn prewarm(&self) -> Result<(), String> {
            self.prewarm_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn transcribe(&self, audio_path: &Path) -> Result<String, String> {
            self.transcribe_calls.fetch_add(1, Ordering::Relaxed);
            if self.sleep_ms > 0 {
                std::thread::sleep(Duration::from_millis(self.sleep_ms));
            }
            Ok(format!("ok:{}", audio_path.display()))
        }
    }

    struct NoopCaptureSink;

    impl CaptureSink for NoopCaptureSink {
        fn on_chunk(&mut self, _chunk: CaptureChunk) -> Result<(), String> {
            Ok(())
        }

        fn on_event(&mut self, _event: CaptureEvent) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn asr_backend_parse_accepts_current_and_legacy_labels() {
        assert!(matches!(
            AsrBackend::parse("whispercpp").unwrap(),
            AsrBackend::WhisperCpp
        ));
        assert!(matches!(
            AsrBackend::parse("whisper-rs").unwrap(),
            AsrBackend::WhisperCpp
        ));
        assert!(matches!(
            AsrBackend::parse("whisperkit").unwrap(),
            AsrBackend::WhisperKit
        ));
        assert!(matches!(
            AsrBackend::parse("moonshine").unwrap(),
            AsrBackend::Moonshine
        ));
    }

    #[test]
    fn channel_mode_parse_accepts_mixed_fallback() {
        assert!(matches!(
            ChannelMode::parse("mixed-fallback").unwrap(),
            ChannelMode::MixedFallback
        ));
    }

    #[test]
    fn help_text_documents_live_chunk_contract_flags() {
        assert!(HELP_TEXT.contains("--live-chunked"));
        assert!(HELP_TEXT.contains("--live-stream"));
        assert!(HELP_TEXT.contains("--chunk-window-ms"));
        assert!(HELP_TEXT.contains("--chunk-stride-ms"));
        assert!(HELP_TEXT.contains("--chunk-queue-cap"));
        assert!(HELP_TEXT.contains("--live-asr-workers"));
        assert!(HELP_TEXT.contains("--keep-temp-audio"));
    }

    #[test]
    fn help_text_documents_runtime_mode_taxonomy() {
        assert!(HELP_TEXT.contains("Runtime mode taxonomy"));
        assert!(HELP_TEXT.contains("representative-offline"));
        assert!(HELP_TEXT.contains("representative-chunked"));
        assert!(HELP_TEXT.contains("live-stream"));
    }

    #[test]
    fn help_text_includes_recordit_migration_guidance() {
        assert!(HELP_TEXT.contains("Migration guidance"));
        assert!(HELP_TEXT.contains("recordit run --mode live"));
        assert!(HELP_TEXT.contains("transcribe-live` remains stable"));
    }

    #[test]
    fn runtime_mode_compatibility_matrix_includes_live_stream_implemented_row() {
        let matrix = runtime_mode_compatibility_matrix();
        assert_eq!(matrix.len(), 3);
        assert!(
            matrix
                .iter()
                .any(|row| row.taxonomy_mode == "live-stream" && row.status == "implemented")
        );
    }

    #[test]
    fn runtime_execution_branch_defaults_to_representative_offline() {
        let config = TranscribeConfig::default();
        assert_eq!(
            select_runtime_execution_branch(&config),
            RuntimeExecutionBranch::RepresentativeOffline
        );
    }

    #[test]
    fn runtime_execution_branch_selects_representative_chunked_when_enabled() {
        let mut config = TranscribeConfig::default();
        config.live_chunked = true;
        assert_eq!(
            select_runtime_execution_branch(&config),
            RuntimeExecutionBranch::RepresentativeChunked
        );
    }

    #[test]
    fn runtime_execution_branch_prefers_live_stream_over_chunked() {
        let mut config = TranscribeConfig::default();
        config.live_chunked = true;
        config.live_stream = true;
        assert_eq!(
            select_runtime_execution_branch(&config),
            RuntimeExecutionBranch::LiveStream
        );
    }

    #[test]
    fn live_stream_vad_threshold_mapping_stays_in_scheduler_calibrated_range() {
        assert_eq!(live_stream_vad_thresholds_per_mille(0.50), (40, 20));
        assert_eq!(live_stream_vad_thresholds_per_mille(0.25), (20, 10));
        assert_eq!(live_stream_vad_thresholds_per_mille(1.00), (80, 40));
        assert_eq!(live_stream_vad_thresholds_per_mille(0.0), (1, 1));
    }

    #[test]
    fn parse_rejects_chunk_overrides_without_live_chunked() {
        let args = vec![
            "--chunk-window-ms".to_string(),
            "5000".to_string(),
            "--asr-model".to_string(),
            "artifacts/bench/models/whispercpp/ggml-tiny.en.bin".to_string(),
        ];
        match parse_args_from(args.into_iter()) {
            Ok(_) => panic!("expected parse failure"),
            Err(err) => assert!(
                err.to_string()
                    .contains("require `--live-chunked` or `--live-stream`")
            ),
        }
    }

    #[test]
    fn parse_rejects_live_chunked_with_replay() {
        let args = vec![
            "--live-chunked".to_string(),
            "--replay-jsonl".to_string(),
            "artifacts/transcribe-live.runtime.jsonl".to_string(),
        ];
        match parse_args_from(args.into_iter()) {
            Ok(_) => panic!("expected parse failure"),
            Err(err) => assert!(
                err.to_string()
                    .contains("cannot be combined with `--replay-jsonl`")
            ),
        }
    }

    #[test]
    fn parse_rejects_live_stream_with_replay() {
        let args = vec![
            "--live-stream".to_string(),
            "--replay-jsonl".to_string(),
            "artifacts/transcribe-live.runtime.jsonl".to_string(),
        ];
        match parse_args_from(args.into_iter()) {
            Ok(_) => panic!("expected parse failure"),
            Err(err) => assert!(
                err.to_string()
                    .contains("cannot be combined with `--replay-jsonl`")
            ),
        }
    }

    #[test]
    fn parse_accepts_live_stream_with_preflight() {
        let args = vec!["--live-stream".to_string(), "--preflight".to_string()];
        match parse_args_from(args.into_iter()).unwrap() {
            ParseOutcome::Help => panic!("expected config"),
            ParseOutcome::Config(config) => {
                assert!(config.preflight);
                assert!(config.live_stream);
                assert_eq!(config.runtime_mode_label(), "live-stream");
                assert_eq!(config.runtime_mode_selector_label(), "--live-stream");
            }
        }
    }

    #[test]
    fn parse_rejects_live_stream_with_live_chunked() {
        let args = vec!["--live-stream".to_string(), "--live-chunked".to_string()];
        match parse_args_from(args.into_iter()) {
            Ok(_) => panic!("expected parse failure"),
            Err(err) => assert!(
                err.to_string()
                    .contains("cannot be combined with `--live-chunked`")
            ),
        }
    }

    #[test]
    fn parse_accepts_live_stream_runtime_entrypoint() {
        let args = vec!["--live-stream".to_string()];
        match parse_args_from(args.into_iter()).unwrap() {
            ParseOutcome::Help => panic!("expected config parse outcome"),
            ParseOutcome::Config(config) => {
                assert!(config.live_stream);
                assert_eq!(config.runtime_mode_label(), "live-stream");
                assert_eq!(config.runtime_mode_taxonomy_label(), "live-stream");
                assert_eq!(config.runtime_mode_selector_label(), "--live-stream");
                assert_eq!(config.runtime_mode_status_label(), "implemented");
            }
        }
    }

    #[test]
    fn parse_accepts_model_doctor_with_live_stream() {
        let args = vec!["--live-stream".to_string(), "--model-doctor".to_string()];
        match parse_args_from(args.into_iter()).unwrap() {
            ParseOutcome::Help => panic!("expected config parse outcome"),
            ParseOutcome::Config(config) => {
                assert!(config.live_stream);
                assert!(config.model_doctor);
                assert_eq!(config.runtime_mode_label(), "live-stream");
                assert_eq!(config.runtime_mode_selector_label(), "--live-stream");
            }
        }
    }

    #[test]
    fn parse_accepts_live_stream_chunk_tuning_values() {
        let args = vec![
            "--live-stream".to_string(),
            "--chunk-window-ms".to_string(),
            "5000".to_string(),
            "--chunk-stride-ms".to_string(),
            "1500".to_string(),
            "--chunk-queue-cap".to_string(),
            "8".to_string(),
        ];
        match parse_args_from(args.into_iter()).unwrap() {
            ParseOutcome::Help => panic!("expected config parse outcome"),
            ParseOutcome::Config(config) => {
                assert!(config.live_stream);
                assert_eq!(config.chunk_window_ms, 5000);
                assert_eq!(config.chunk_stride_ms, 1500);
                assert_eq!(config.chunk_queue_cap, 8);
            }
        }
    }

    #[test]
    fn parse_accepts_live_worker_pool_tuning_flags() {
        let args = vec![
            "--live-stream".to_string(),
            "--live-asr-workers".to_string(),
            "3".to_string(),
            "--keep-temp-audio".to_string(),
        ];
        match parse_args_from(args.into_iter()).unwrap() {
            ParseOutcome::Help => panic!("expected config parse outcome"),
            ParseOutcome::Config(config) => {
                assert!(config.live_stream);
                assert_eq!(config.live_asr_workers, 3);
                assert!(config.keep_temp_audio);
            }
        }
    }

    #[test]
    fn parse_accepts_live_chunked_contract_values() {
        let args = vec!["--live-chunked".to_string()];
        match parse_args_from(args.into_iter()).unwrap() {
            ParseOutcome::Help => panic!("expected config parse outcome"),
            ParseOutcome::Config(config) => {
                assert!(config.live_chunked);
                assert_eq!(config.chunk_window_ms, 2000);
                assert_eq!(config.chunk_stride_ms, 500);
                assert_eq!(config.chunk_queue_cap, 4);
                assert_eq!(config.runtime_mode_label(), "live-chunked");
                assert_eq!(
                    config.runtime_mode_taxonomy_label(),
                    "representative-chunked"
                );
                assert_eq!(config.runtime_mode_selector_label(), "--live-chunked");
            }
        }
    }

    #[test]
    fn parse_defaults_to_representative_offline_taxonomy() {
        match parse_args_from(Vec::<String>::new().into_iter()).unwrap() {
            ParseOutcome::Help => panic!("expected config parse outcome"),
            ParseOutcome::Config(config) => {
                assert_eq!(config.runtime_mode_label(), "representative-offline");
                assert_eq!(
                    config.runtime_mode_taxonomy_label(),
                    "representative-offline"
                );
                assert_eq!(config.runtime_mode_selector_label(), "<default>");
            }
        }
    }

    #[test]
    fn rolling_chunk_scheduler_uses_2s_window_and_0_5s_stride() {
        let windows = build_rolling_chunk_windows(0, 5_000, 2_000, 500);
        let observed = windows
            .iter()
            .map(|window| (window.start_ms, window.end_ms, window.overlap_prev_ms))
            .collect::<Vec<_>>();

        assert_eq!(
            observed,
            vec![
                (0, 2_000, 0),
                (500, 2_500, 1_500),
                (1_000, 3_000, 1_500),
                (1_500, 3_500, 1_500),
                (2_000, 4_000, 1_500),
                (2_500, 4_500, 1_500),
                (3_000, 5_000, 1_500),
            ]
        );
    }

    #[test]
    fn rolling_chunk_scheduler_aligns_last_window_to_session_end() {
        let windows = build_rolling_chunk_windows(0, 2_300, 2_000, 500);
        let observed = windows
            .iter()
            .map(|window| (window.start_ms, window.end_ms, window.overlap_prev_ms))
            .collect::<Vec<_>>();

        assert_eq!(observed, vec![(0, 2_000, 0), (300, 2_300, 1_700)]);
    }

    #[test]
    fn rolling_chunk_scheduler_handles_short_sessions_with_single_window() {
        let windows = build_rolling_chunk_windows(0, 1_200, 2_000, 500);
        let observed = windows
            .iter()
            .map(|window| (window.start_ms, window.end_ms, window.overlap_prev_ms))
            .collect::<Vec<_>>();

        assert_eq!(observed, vec![(0, 1_200, 0)]);
    }

    #[test]
    fn live_chunked_events_use_deterministic_chunk_segment_ids() {
        let vad_boundaries = vec![VadBoundary {
            id: 0,
            start_ms: 0,
            end_ms: 10_000,
            source: "energy_threshold",
        }];

        let events = build_transcript_events(
            "alpha beta gamma delta epsilon zeta eta theta",
            &vad_boundaries,
            "mic",
            "mic",
            true,
            2_000,
            500,
        );
        let finals = events
            .iter()
            .filter(|event| event.event_type == "final")
            .collect::<Vec<_>>();

        assert_eq!(finals.len(), 17);
        assert_eq!(finals[0].segment_id, "mic-chunk-0000-0-2000");
        assert_eq!(finals[0].start_ms, 0);
        assert_eq!(finals[0].end_ms, 2_000);
        assert_eq!(finals[1].segment_id, "mic-chunk-0001-500-2500");
        assert_eq!(finals[16].segment_id, "mic-chunk-0016-8000-10000");
    }

    #[test]
    fn parse_rejects_model_doctor_with_replay() {
        let args = vec![
            "--model-doctor".to_string(),
            "--replay-jsonl".to_string(),
            "artifacts/transcribe-live.runtime.jsonl".to_string(),
        ];
        match parse_args_from(args.into_iter()) {
            Ok(_) => panic!("expected parse failure"),
            Err(err) => assert!(
                err.to_string()
                    .contains("`--model-doctor` cannot be combined with `--replay-jsonl`")
            ),
        }
    }

    #[test]
    fn parse_rejects_model_doctor_with_preflight() {
        let args = vec!["--model-doctor".to_string(), "--preflight".to_string()];
        match parse_args_from(args.into_iter()) {
            Ok(_) => panic!("expected parse failure"),
            Err(err) => assert!(
                err.to_string()
                    .contains("`--model-doctor` cannot be combined with `--preflight`")
            ),
        }
    }

    #[test]
    fn extract_json_string_field_accepts_whitespace_after_colon() {
        let payload = "{\"choices\":[{\"message\":{\"content\": \"cleaned local segment\"}}]}";
        let parsed = super::extract_json_string_field(payload, "content");
        assert_eq!(parsed.as_deref(), Some("cleaned local segment"));
    }

    #[test]
    fn trust_notice_parser_reads_replay_context() {
        let line = "{\"event_type\":\"trust_notice\",\"channel\":\"control\",\"code\":\"mode_degradation\",\"severity\":\"warn\",\"cause\":\"requested mixed-fallback but input had 1 channel\",\"impact\":\"channel attribution reduced\",\"guidance\":\"use stereo input\"}";
        let notice = parse_trust_notice(line).unwrap();
        assert_eq!(notice.code, "mode_degradation");
        assert_eq!(notice.severity, "warn");
        assert_eq!(notice.impact, "channel attribution reduced");
    }

    #[test]
    fn replay_parser_preserves_source_lineage_for_reconciled_events() {
        let line = "{\"event_type\":\"reconciled_final\",\"channel\":\"mic\",\"segment_id\":\"mic-reconciled-0\",\"source_final_segment_id\":\"mic-chunk-0000\",\"start_ms\":0,\"end_ms\":2000,\"text\":\"hello\"}";
        let parsed = parse_replay_transcript_event(line, 1).unwrap().unwrap();
        assert_eq!(parsed.event_type, "reconciled_final");
        assert_eq!(
            parsed.source_final_segment_id.as_deref(),
            Some("mic-chunk-0000")
        );
    }

    #[test]
    fn replay_parser_ignores_non_transcript_control_events() {
        let line = "{\"event_type\":\"chunk_queue\",\"channel\":\"control\",\"submitted\":4}";
        let parsed = parse_replay_transcript_event(line, 1).unwrap();
        assert!(parsed.is_none());
    }

    #[test]
    fn trust_notice_builder_emits_mode_and_cleanup_notices() {
        let config = TranscribeConfig::default();
        let mut cleanup = CleanupQueueTelemetry::disabled(&config);
        let chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        cleanup.enabled = true;
        cleanup.dropped_queue_full = 1;
        cleanup.failed = 1;
        cleanup.pending = 1;
        cleanup.drain_completed = false;

        let notices = build_trust_notices(
            ChannelMode::MixedFallback,
            ChannelMode::Mixed,
            &[ModeDegradationEvent {
                code: "fallback_to_mixed",
                detail: "requested mixed-fallback but input had 1 channel".to_string(),
            }],
            &cleanup,
            &chunk_queue,
        );

        assert!(
            notices
                .iter()
                .any(|notice| notice.code == "mode_degradation")
        );
        assert!(
            notices
                .iter()
                .any(|notice| notice.code == "cleanup_queue_drop")
        );
        assert!(
            notices
                .iter()
                .any(|notice| notice.code == "cleanup_processing_failure")
        );
        assert!(
            notices
                .iter()
                .any(|notice| notice.code == "cleanup_drain_incomplete")
        );
    }

    #[test]
    fn trust_notice_builder_emits_continuity_notice_for_recovered_interruptions() {
        let config = TranscribeConfig::default();
        let cleanup = CleanupQueueTelemetry::disabled(&config);
        let chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        let notices = build_trust_notices(
            ChannelMode::Separate,
            ChannelMode::Separate,
            &[ModeDegradationEvent {
                code: "live_capture_interruption_recovered",
                detail: "near-live capture recovered from 2 stream interruption restart(s)"
                    .to_string(),
            }],
            &cleanup,
            &chunk_queue,
        );

        assert!(
            notices
                .iter()
                .any(|notice| notice.code == "continuity_recovered_with_gaps")
        );
    }

    #[test]
    fn trust_notice_builder_emits_continuity_unverified_notice_with_guidance() {
        let config = TranscribeConfig::default();
        let cleanup = CleanupQueueTelemetry::disabled(&config);
        let chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        let notices = build_trust_notices(
            ChannelMode::Separate,
            ChannelMode::Separate,
            &[ModeDegradationEvent {
                code: LIVE_CAPTURE_CONTINUITY_UNVERIFIED_CODE,
                detail: "continuity telemetry unavailable for this session".to_string(),
            }],
            &cleanup,
            &chunk_queue,
        );

        let notice = notices
            .iter()
            .find(|notice| notice.code == "continuity_unverified")
            .expect("expected continuity_unverified trust notice");
        assert_eq!(notice.severity, "warn");
        assert!(
            notice
                .guidance
                .contains("Ensure capture telemetry is writable/readable")
        );
    }

    #[test]
    fn trust_notice_builder_emits_capture_transport_degraded_notice() {
        let config = TranscribeConfig::default();
        let cleanup = CleanupQueueTelemetry::disabled(&config);
        let chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        let notices = build_trust_notices(
            ChannelMode::Separate,
            ChannelMode::Separate,
            &[ModeDegradationEvent {
                code: LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE,
                detail:
                    "near-live capture transport reported degradation source(s): queue_full_drops"
                        .to_string(),
            }],
            &cleanup,
            &chunk_queue,
        );

        let notice = notices
            .iter()
            .find(|notice| notice.code == "capture_transport_degraded")
            .expect("expected capture_transport_degraded trust notice");
        assert_eq!(notice.severity, "warn");
        assert!(notice.impact.contains("transport"));
    }

    #[test]
    fn trust_notice_builder_emits_chunk_queue_backpressure_notice() {
        let config = TranscribeConfig::default();
        let cleanup = CleanupQueueTelemetry::disabled(&config);
        let mut chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        chunk_queue.enabled = true;
        chunk_queue.max_queue = 2;
        chunk_queue.dropped_oldest = 2;
        chunk_queue.submitted = 6;
        chunk_queue.processed = 4;
        let notices = build_trust_notices(
            ChannelMode::Separate,
            ChannelMode::Separate,
            &[ModeDegradationEvent {
                code: LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE,
                detail: "near-live ASR chunk queue dropped 2 oldest task(s) under pressure"
                    .to_string(),
            }],
            &cleanup,
            &chunk_queue,
        );
        let notice = notices
            .iter()
            .find(|notice| notice.code == "chunk_queue_backpressure")
            .expect("expected chunk_queue_backpressure trust notice");
        assert_eq!(notice.severity, "warn");
        assert!(notice.guidance.contains("--chunk-queue-cap"));
    }

    #[test]
    fn trust_notice_builder_emits_severe_chunk_queue_backpressure_notice() {
        let config = TranscribeConfig::default();
        let cleanup = CleanupQueueTelemetry::disabled(&config);
        let mut chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        chunk_queue.enabled = true;
        chunk_queue.max_queue = 2;
        chunk_queue.submitted = 6;
        chunk_queue.dropped_oldest = 3;
        chunk_queue.high_water = 2;
        let notices = build_trust_notices(
            ChannelMode::Separate,
            ChannelMode::Separate,
            &[ModeDegradationEvent {
                code: LIVE_CHUNK_QUEUE_BACKPRESSURE_SEVERE_CODE,
                detail: "near-live ASR queue entered severe backpressure".to_string(),
            }],
            &cleanup,
            &chunk_queue,
        );
        let severe = notices
            .iter()
            .find(|notice| notice.code == "chunk_queue_backpressure_severe")
            .expect("expected severe queue backpressure notice");
        assert_eq!(severe.severity, "error");
    }

    #[test]
    fn chunk_queue_backpressure_severity_thresholds_detect_persistent_pressure() {
        let mut mild = LiveChunkQueueTelemetry::enabled(4);
        mild.submitted = 10;
        mild.dropped_oldest = 2;
        mild.high_water = 3;
        assert!(!chunk_queue_backpressure_is_severe(&mild));

        let mut severe = LiveChunkQueueTelemetry::enabled(4);
        severe.submitted = 9;
        severe.dropped_oldest = 3;
        severe.high_water = 4;
        assert!(chunk_queue_backpressure_is_severe(&severe));
    }

    #[test]
    fn chunk_queue_backpressure_is_severe_trips_on_drop_ratio_without_full_queue() {
        let mut ratio_only = LiveChunkQueueTelemetry::enabled(10);
        ratio_only.submitted = 9;
        ratio_only.dropped_oldest = 3;
        ratio_only.high_water = 4;

        assert!(chunk_queue_backpressure_is_severe(&ratio_only));
    }

    #[test]
    fn trust_notice_builder_emits_reconciliation_notice() {
        let config = TranscribeConfig::default();
        let cleanup = CleanupQueueTelemetry::disabled(&config);
        let chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        let notices = build_trust_notices(
            ChannelMode::Separate,
            ChannelMode::Separate,
            &[ModeDegradationEvent {
                code: RECONCILIATION_APPLIED_CODE,
                detail: "post-session reconciliation emitted reconciled_final events".to_string(),
            }],
            &cleanup,
            &chunk_queue,
        );
        let notice = notices
            .iter()
            .find(|notice| notice.code == "reconciliation_applied")
            .expect("expected reconciliation_applied trust notice");
        assert_eq!(notice.severity, "warn");
        assert!(notice.impact.contains("canonical completeness"));
        assert!(notice.guidance.contains("reconciled_final"));
        assert!(notice.guidance.contains("reconciliation_matrix"));
    }

    #[test]
    fn reconciliation_matrix_triggers_on_queue_drop_continuity_and_shutdown_flush() {
        let matrix = build_reconciliation_matrix(
            &[VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 2_000,
                source: "shutdown_flush",
            }],
            &[
                ModeDegradationEvent {
                    code: LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE,
                    detail: "dropped oldest chunk work under pressure".to_string(),
                },
                ModeDegradationEvent {
                    code: LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE,
                    detail: "capture recovered with restart gaps".to_string(),
                },
            ],
        );

        assert!(matrix.required);
        let codes = matrix
            .triggers
            .iter()
            .map(|trigger| trigger.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&"chunk_queue_drop_oldest"));
        assert!(codes.contains(&"continuity_recovered_with_gaps"));
        assert!(codes.contains(&"shutdown_flush_boundary"));
    }

    #[test]
    fn reconciliation_matrix_is_not_required_without_triggers() {
        let matrix = build_reconciliation_matrix(
            &[VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 2_000,
                source: "energy_threshold",
            }],
            &[],
        );

        assert!(!matrix.required);
        assert!(matrix.triggers.is_empty());
    }

    #[test]
    fn reconciliation_matrix_triggers_on_capture_transport_and_callback_degradation() {
        let matrix = build_reconciliation_matrix(
            &[VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 2_000,
                source: "energy_threshold",
            }],
            &[
                ModeDegradationEvent {
                    code: LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE,
                    detail: "queue pressure".to_string(),
                },
                ModeDegradationEvent {
                    code: LIVE_CAPTURE_CALLBACK_CONTRACT_DEGRADED_CODE,
                    detail: "callback violations".to_string(),
                },
            ],
        );
        let codes = matrix
            .triggers
            .iter()
            .map(|trigger| trigger.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&"capture_transport_degraded"));
        assert!(codes.contains(&"capture_callback_contract_degraded"));
    }

    #[test]
    fn live_chunk_queue_drops_oldest_queued_tasks_under_pressure() {
        let result = run_live_chunk_queue(
            vec![
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 0,
                    channel: "mic".to_string(),
                    segment_id: "s0-mic".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "a".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 0,
                    channel: "system".to_string(),
                    segment_id: "s0-system".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "b".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 1,
                    channel: "mic".to_string(),
                    segment_id: "s1-mic".to_string(),
                    start_ms: 1_000,
                    end_ms: 5_000,
                    text: "c".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 1,
                    channel: "system".to_string(),
                    segment_id: "s1-system".to_string(),
                    start_ms: 1_000,
                    end_ms: 5_000,
                    text: "d".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 2,
                    channel: "mic".to_string(),
                    segment_id: "s2-mic".to_string(),
                    start_ms: 2_000,
                    end_ms: 6_000,
                    text: "e".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 2,
                    channel: "system".to_string(),
                    segment_id: "s2-system".to_string(),
                    start_ms: 2_000,
                    end_ms: 6_000,
                    text: "f".to_string(),
                    source_final_segment_id: None,
                },
            ],
            2,
            1_000,
        );

        assert_eq!(result.telemetry.submitted, 6);
        assert_eq!(result.telemetry.dropped_oldest, 2);
        assert_eq!(result.telemetry.processed, 4);
        assert_eq!(result.telemetry.max_queue, 2);
        assert_eq!(result.telemetry.high_water, 2);
        assert!(result.telemetry.drain_completed);
        assert_eq!(result.telemetry.lag_sample_count, 4);
        assert_eq!(result.telemetry.lag_p50_ms, 1_000);
        assert_eq!(result.telemetry.lag_p95_ms, 2_000);
        assert_eq!(result.telemetry.lag_max_ms, 2_000);
        let final_ids = result
            .events
            .iter()
            .filter(|event| event.event_type == "final")
            .map(|event| event.segment_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            final_ids,
            vec![
                "s0-mic".to_string(),
                "s1-mic".to_string(),
                "s2-mic".to_string(),
                "s2-system".to_string(),
            ]
        );
    }

    #[test]
    fn asr_work_priority_drops_lower_classes_before_final_integrity_work() {
        let result = run_live_chunk_queue(
            vec![
                AsrWorkItem {
                    class: AsrWorkClass::Partial,
                    tick_index: 0,
                    channel: "mic".to_string(),
                    segment_id: "partial-a".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "preview-a".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 0,
                    channel: "mic".to_string(),
                    segment_id: "final-a".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "final-a".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Reconcile,
                    tick_index: 0,
                    channel: "mic".to_string(),
                    segment_id: "reconcile-a".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "reconciled-a".to_string(),
                    source_final_segment_id: Some("final-a".to_string()),
                },
                AsrWorkItem {
                    class: AsrWorkClass::Partial,
                    tick_index: 0,
                    channel: "mic".to_string(),
                    segment_id: "partial-b".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "preview-b".to_string(),
                    source_final_segment_id: None,
                },
            ],
            2,
            1_000,
        );

        assert_eq!(result.telemetry.submitted, 4);
        assert_eq!(result.telemetry.dropped_oldest, 2);

        let emitted_ids = result
            .events
            .iter()
            .map(|event| event.segment_id.clone())
            .collect::<Vec<_>>();
        assert!(emitted_ids.contains(&"final-a".to_string()));
        assert!(emitted_ids.contains(&"partial-b".to_string()));
        assert!(!emitted_ids.contains(&"partial-a".to_string()));
        assert!(!emitted_ids.contains(&"reconcile-a".to_string()));

        let final_events = result
            .events
            .iter()
            .filter(|event| event.event_type == "final")
            .map(|event| event.segment_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(final_events, vec!["final-a".to_string()]);
    }

    #[test]
    fn live_chunk_scheduler_emits_one_final_per_vad_boundary() {
        let result = build_live_chunked_events_with_queue(
            &[
                super::ChannelTranscriptSummary {
                    role: "mic",
                    label: "mic".to_string(),
                    text: "alpha beta gamma delta epsilon zeta".to_string(),
                },
                super::ChannelTranscriptSummary {
                    role: "system",
                    label: "system".to_string(),
                    text: "one two three four five six".to_string(),
                },
            ],
            &[
                VadBoundary {
                    id: 0,
                    start_ms: 0,
                    end_ms: 1_200,
                    source: "energy_threshold",
                },
                VadBoundary {
                    id: 1,
                    start_ms: 1_700,
                    end_ms: 2_800,
                    source: "shutdown_flush",
                },
            ],
            2_000,
            1_000,
            8,
        );

        let finals = result
            .events
            .iter()
            .filter(|event| event.event_type == "final")
            .collect::<Vec<_>>();
        assert_eq!(finals.len(), 4);
        assert_eq!(finals[0].segment_id, "mic-segment-0000-0-1200");
        assert_eq!(finals[1].segment_id, "system-segment-0000-0-1200");
        assert_eq!(finals[2].segment_id, "mic-segment-0001-1700-2800");
        assert_eq!(finals[3].segment_id, "system-segment-0001-1700-2800");
        assert_eq!(result.telemetry.submitted, 4);
        assert_eq!(result.telemetry.processed, 4);
    }

    #[test]
    fn live_chunk_scheduler_canonicalizes_boundary_and_channel_order_for_ids() {
        let shuffled = build_live_chunked_events_with_queue(
            &[
                super::ChannelTranscriptSummary {
                    role: "system",
                    label: "system".to_string(),
                    text: "one two three four".to_string(),
                },
                super::ChannelTranscriptSummary {
                    role: "mic",
                    label: "mic".to_string(),
                    text: "alpha beta gamma delta".to_string(),
                },
            ],
            &[
                VadBoundary {
                    id: 9,
                    start_ms: 1_700,
                    end_ms: 2_800,
                    source: "shutdown_flush",
                },
                VadBoundary {
                    id: 1,
                    start_ms: 0,
                    end_ms: 1_200,
                    source: "energy_threshold",
                },
            ],
            2_000,
            500,
            8,
        );

        let canonical = build_live_chunked_events_with_queue(
            &[
                super::ChannelTranscriptSummary {
                    role: "mic",
                    label: "mic".to_string(),
                    text: "alpha beta gamma delta".to_string(),
                },
                super::ChannelTranscriptSummary {
                    role: "system",
                    label: "system".to_string(),
                    text: "one two three four".to_string(),
                },
            ],
            &[
                VadBoundary {
                    id: 1,
                    start_ms: 0,
                    end_ms: 1_200,
                    source: "energy_threshold",
                },
                VadBoundary {
                    id: 9,
                    start_ms: 1_700,
                    end_ms: 2_800,
                    source: "shutdown_flush",
                },
            ],
            2_000,
            500,
            8,
        );

        let final_ids = shuffled
            .events
            .iter()
            .filter(|event| event.event_type == "final")
            .map(|event| event.segment_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            final_ids,
            vec![
                "mic-segment-0000-0-1200".to_string(),
                "system-segment-0000-0-1200".to_string(),
                "mic-segment-0001-1700-2800".to_string(),
                "system-segment-0001-1700-2800".to_string(),
            ]
        );
        let shuffled_timeline = shuffled
            .events
            .iter()
            .map(|event| {
                (
                    event.event_type,
                    event.channel.clone(),
                    event.segment_id.clone(),
                    event.start_ms,
                    event.end_ms,
                    event.text.clone(),
                    event.source_final_segment_id.clone(),
                )
            })
            .collect::<Vec<_>>();
        let canonical_timeline = canonical
            .events
            .iter()
            .map(|event| {
                (
                    event.event_type,
                    event.channel.clone(),
                    event.segment_id.clone(),
                    event.start_ms,
                    event.end_ms,
                    event.text.clone(),
                    event.source_final_segment_id.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(shuffled_timeline, canonical_timeline);
    }

    #[test]
    fn live_chunk_scheduler_emits_stride_partial_windows_before_boundary_close() {
        let result = build_live_chunked_events_with_queue(
            &[super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "alpha beta gamma delta epsilon zeta eta theta".to_string(),
            }],
            &[VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 2_600,
                source: "energy_threshold",
            }],
            2_000,
            500,
            8,
        );

        let finals = result
            .events
            .iter()
            .filter(|event| event.event_type == "final")
            .collect::<Vec<_>>();
        let partial_ids = result
            .events
            .iter()
            .filter(|event| event.event_type == "partial")
            .map(|event| event.segment_id.clone())
            .collect::<Vec<_>>();

        assert_eq!(finals.len(), 1);
        assert_eq!(finals[0].segment_id, "mic-segment-0000-0-2600");
        assert!(
            partial_ids
                .iter()
                .any(|id| id == "mic-segment-0000-partial-0000-0-2000")
        );
        assert!(
            partial_ids
                .iter()
                .any(|id| id == "mic-segment-0000-partial-0001-500-2500")
        );
        assert!(partial_ids.iter().any(|id| id == "mic-segment-0000-0-2600"));
    }

    #[test]
    fn live_chunk_scheduler_normalizes_unsorted_boundary_ids_for_deterministic_segment_ids() {
        let result = build_live_chunked_events_with_queue(
            &[super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "alpha beta gamma delta epsilon".to_string(),
            }],
            &[
                VadBoundary {
                    id: 7,
                    start_ms: 1_700,
                    end_ms: 2_800,
                    source: "shutdown_flush",
                },
                VadBoundary {
                    id: 2,
                    start_ms: 0,
                    end_ms: 1_200,
                    source: "energy_threshold",
                },
            ],
            2_000,
            1_000,
            8,
        );

        let finals = result
            .events
            .iter()
            .filter(|event| event.event_type == "final")
            .collect::<Vec<_>>();
        assert_eq!(finals.len(), 2);
        assert_eq!(finals[0].start_ms, 0);
        assert_eq!(finals[0].end_ms, 1_200);
        assert_eq!(finals[0].segment_id, "mic-segment-0000-0-1200");
        assert_eq!(finals[1].start_ms, 1_700);
        assert_eq!(finals[1].end_ms, 2_800);
        assert_eq!(finals[1].segment_id, "mic-segment-0001-1700-2800");
    }

    #[test]
    fn continuity_events_use_capture_restart_count_when_available() {
        let temp_dir = write_temp_dir("recordit-live-continuity-recovered");
        let input_wav = temp_dir.join("capture.wav");
        File::create(&input_wav).unwrap();
        let telemetry_path = temp_dir.join("capture.telemetry.json");
        fs::write(&telemetry_path, "{\"restart_count\":2}").unwrap();

        let mut config = TranscribeConfig::default();
        config.live_chunked = true;
        config.input_wav = input_wav.clone();

        let events = collect_live_capture_continuity_events(&config);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].code, "live_capture_interruption_recovered");
        assert!(
            events[0]
                .detail
                .contains("2 stream interruption restart(s)")
        );

        let _ = fs::remove_file(input_wav);
        let _ = fs::remove_file(telemetry_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn continuity_events_warn_when_capture_telemetry_missing() {
        let temp_dir = write_temp_dir("recordit-live-continuity-missing");
        let input_wav = temp_dir.join("capture.wav");
        File::create(&input_wav).unwrap();

        let mut config = TranscribeConfig::default();
        config.live_chunked = true;
        config.input_wav = input_wav.clone();

        let events = collect_live_capture_continuity_events(&config);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].code, "live_capture_continuity_unverified");
        assert!(events[0].detail.contains("could not be verified"));

        let _ = fs::remove_file(input_wav);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn continuity_events_include_transport_and_callback_signals() {
        let temp_dir = write_temp_dir("recordit-live-continuity-degradation-sources");
        let input_wav = temp_dir.join("capture.wav");
        File::create(&input_wav).unwrap();
        let telemetry_path = temp_dir.join("capture.telemetry.json");
        fs::write(
            &telemetry_path,
            concat!(
                "{\n",
                "  \"restart_count\": 0,\n",
                "  \"degradation_events\": [\n",
                "    {\"source\":\"queue_full_drops\",\"count\":2},\n",
                "    {\"source\":\"missing_format_description\",\"count\":1}\n",
                "  ]\n",
                "}\n"
            ),
        )
        .unwrap();

        let mut config = TranscribeConfig::default();
        config.live_chunked = true;
        config.input_wav = input_wav.clone();

        let events = collect_live_capture_continuity_events(&config);
        let codes = events.iter().map(|event| event.code).collect::<Vec<_>>();
        assert!(codes.contains(&LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE));
        assert!(codes.contains(&LIVE_CAPTURE_CALLBACK_CONTRACT_DEGRADED_CODE));
        assert!(!codes.contains(&LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE));

        let _ = fs::remove_file(input_wav);
        let _ = fs::remove_file(telemetry_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn continuity_events_apply_to_live_stream_mode_without_live_chunked_flag() {
        let temp_dir = write_temp_dir("recordit-live-stream-continuity-sources");
        let input_wav = temp_dir.join("capture.wav");
        File::create(&input_wav).unwrap();
        let telemetry_path = temp_dir.join("capture.telemetry.json");
        fs::write(&telemetry_path, "{\"restart_count\":1}").unwrap();

        let mut config = TranscribeConfig::default();
        config.live_stream = true;
        config.input_wav = input_wav.clone();

        let events = collect_live_capture_continuity_events(&config);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].code, LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE);
        assert!(events[0].detail.contains("restart(s)"));

        let _ = fs::remove_file(input_wav);
        let _ = fs::remove_file(telemetry_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn runtime_output_events_convert_to_live_transcript_contract_events() {
        let config = TranscribeConfig::default();
        let runtime_events = vec![
            RuntimeOutputEvent::AsrCompleted {
                emit_seq: 1,
                result: RuntimeAsrResult {
                    job: RuntimeAsrJobSpec {
                        emit_seq: 1,
                        job_class: RuntimeAsrJobClass::Partial,
                        channel: "microphone".to_string(),
                        segment_id: "seg-a".to_string(),
                        segment_ord: 1,
                        window_ord: 1,
                        start_ms: 0,
                        end_ms: 500,
                    },
                    transcript_text: "partial text".to_string(),
                },
            },
            RuntimeOutputEvent::AsrCompleted {
                emit_seq: 2,
                result: RuntimeAsrResult {
                    job: RuntimeAsrJobSpec {
                        emit_seq: 2,
                        job_class: RuntimeAsrJobClass::Final,
                        channel: "system-audio".to_string(),
                        segment_id: "seg-b".to_string(),
                        segment_ord: 2,
                        window_ord: 2,
                        start_ms: 500,
                        end_ms: 1_000,
                    },
                    transcript_text: "final text".to_string(),
                },
            },
            RuntimeOutputEvent::AsrCompleted {
                emit_seq: 3,
                result: RuntimeAsrResult {
                    job: RuntimeAsrJobSpec {
                        emit_seq: 3,
                        job_class: RuntimeAsrJobClass::Reconcile,
                        channel: "microphone".to_string(),
                        segment_id: "seg-c".to_string(),
                        segment_ord: 3,
                        window_ord: 3,
                        start_ms: 1_000,
                        end_ms: 1_400,
                    },
                    transcript_text: "reconciled text".to_string(),
                },
            },
        ];

        let events = transcript_events_from_runtime_output_events(&config, &runtime_events);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, "partial");
        assert_eq!(events[0].channel, config.speaker_labels.mic);
        assert_eq!(events[1].event_type, "final");
        assert_eq!(events[1].channel, config.speaker_labels.system);
        assert_eq!(events[2].event_type, "reconciled_final");
        assert_eq!(events[2].segment_id, "seg-c-reconciled");
        assert_eq!(events[2].source_final_segment_id.as_deref(), Some("seg-c"));
    }

    #[test]
    fn live_stream_chunk_queue_telemetry_maps_runtime_and_pool_counts() {
        let mut config = TranscribeConfig::default();
        config.live_stream = true;
        config.chunk_queue_cap = 3;

        let runtime_summary = LiveRuntimeSummary {
            final_phase: LiveRuntimePhase::Shutdown,
            ready_for_transcripts: true,
            transition_count: 4,
            capture_chunks_seen: 20,
            capture_events_seen: 2,
            asr_jobs_queued: 7,
            asr_results_emitted: 6,
            pending_jobs: 0,
            pending_final_jobs: 0,
            shutdown_abandoned_jobs: 0,
            shutdown_abandoned_final_jobs: 0,
        };
        let pool = LiveAsrPoolTelemetry {
            prewarm_ok: true,
            submitted: 7,
            enqueued: 6,
            dropped_queue_full: 1,
            processed: 6,
            succeeded: 6,
            failed: 1,
            retry_attempts: 0,
            temp_audio_deleted: 0,
            temp_audio_retained: 0,
        };

        let telemetry = live_stream_chunk_queue_telemetry(&config, &runtime_summary, &pool);
        assert!(telemetry.enabled);
        assert_eq!(telemetry.max_queue, 3);
        assert_eq!(telemetry.submitted, 7);
        assert_eq!(telemetry.enqueued, 6);
        assert_eq!(telemetry.dropped_oldest, 1);
        assert_eq!(telemetry.processed, 6);
        assert_eq!(telemetry.pending, 0);
        assert!(telemetry.drain_completed);
    }

    #[test]
    fn continuity_telemetry_candidates_prefer_out_wav_with_input_fallback() {
        let mut config = TranscribeConfig::default();
        config.live_chunked = true;
        config.input_wav = PathBuf::from("artifacts/live-input.wav");
        config.out_wav = PathBuf::from("artifacts/live-output.wav");

        let candidates = live_capture_telemetry_path_candidates(&config);
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates[0]
                .to_string_lossy()
                .ends_with("artifacts/live-output.telemetry.json")
        );
        assert!(
            candidates[1]
                .to_string_lossy()
                .ends_with("artifacts/live-input.telemetry.json")
        );
    }

    #[test]
    fn incremental_vad_tracker_requires_min_speech_before_opening_segment() {
        let mut tracker = IncrementalVadTracker::new("mic", 10, 0.5, 200, 200);
        let levels = [0.8, 0.2, 0.7, 0.8, 0.1, 0.1];
        for (idx, level) in levels.into_iter().enumerate() {
            tracker.observe(idx, level);
        }
        let boundaries = tracker.finish(levels.len());
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].channel, "mic");
        assert_eq!(boundaries[0].start_ms, 200);
        assert_eq!(boundaries[0].end_ms, 400);
    }

    #[test]
    fn incremental_vad_tracker_marks_open_tail_with_shutdown_flush_source() {
        let mut tracker = IncrementalVadTracker::new("mic", 10, 0.5, 200, 200);
        let levels = [0.8, 0.8, 0.7, 0.7];
        for (idx, level) in levels.into_iter().enumerate() {
            tracker.observe(idx, level);
        }
        let boundaries = tracker.finish(levels.len());
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].channel, "mic");
        assert_eq!(boundaries[0].start_ms, 0);
        assert_eq!(boundaries[0].end_ms, 400);
        assert_eq!(boundaries[0].source, "shutdown_flush");
    }

    #[test]
    fn per_channel_vad_boundaries_track_channels_independently() {
        let boundaries = detect_per_channel_vad_boundaries(
            &[
                ("mic".to_string(), vec![0.7, 0.8, 0.1, 0.1, 0.0, 0.0]),
                ("system".to_string(), vec![0.0, 0.0, 0.6, 0.7, 0.1, 0.1]),
            ],
            10,
            0.5,
            200,
            200,
        );
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0].channel, "mic");
        assert_eq!(boundaries[0].start_ms, 0);
        assert_eq!(boundaries[0].end_ms, 200);
        assert_eq!(boundaries[1].channel, "system");
        assert_eq!(boundaries[1].start_ms, 200);
        assert_eq!(boundaries[1].end_ms, 400);
    }

    #[test]
    fn merge_channel_vad_boundaries_merges_overlaps_and_promotes_shutdown_flush_source() {
        let merged = merge_channel_vad_boundaries(
            &[
                ChannelVadBoundary {
                    channel: "mic".to_string(),
                    start_ms: 100,
                    end_ms: 400,
                    source: "energy_threshold",
                },
                ChannelVadBoundary {
                    channel: "system".to_string(),
                    start_ms: 250,
                    end_ms: 500,
                    source: "shutdown_flush",
                },
                ChannelVadBoundary {
                    channel: "mic".to_string(),
                    start_ms: 700,
                    end_ms: 900,
                    source: "energy_threshold",
                },
                ChannelVadBoundary {
                    channel: "system".to_string(),
                    start_ms: 850,
                    end_ms: 1_000,
                    source: "energy_threshold",
                },
            ],
            1_000,
            2_000,
        );

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, 0);
        assert_eq!(merged[0].start_ms, 100);
        assert_eq!(merged[0].end_ms, 500);
        assert_eq!(merged[0].source, "shutdown_flush");
        assert_eq!(merged[1].id, 1);
        assert_eq!(merged[1].start_ms, 700);
        assert_eq!(merged[1].end_ms, 1_000);
        assert_eq!(merged[1].source, "energy_threshold");
    }

    #[test]
    fn vad_detector_emits_boundary_for_energy_region() {
        let mut levels = vec![0.0; 2_000];
        for level in &mut levels[400..1_200] {
            *level = 0.9;
        }

        let boundaries = detect_vad_boundaries(&levels, 1_000, 0.5, 100, 100);
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].start_ms, 400);
        assert_eq!(boundaries[0].end_ms, 1_200);
        assert_eq!(boundaries[0].source, "energy_threshold");
    }

    #[test]
    fn vad_detector_marks_trailing_speech_with_shutdown_flush_source() {
        let boundaries = detect_vad_boundaries(&vec![0.9; 500], 1_000, 0.5, 100, 100);
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].start_ms, 0);
        assert_eq!(boundaries[0].end_ms, 500);
        assert_eq!(boundaries[0].source, "shutdown_flush");
    }

    #[test]
    fn vad_detector_falls_back_when_no_speech_crosses_threshold() {
        let boundaries = detect_vad_boundaries(&vec![0.01; 500], 1_000, 0.5, 100, 100);
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].start_ms, 0);
        assert_eq!(boundaries[0].end_ms, 500);
        assert_eq!(boundaries[0].source, "fallback_full_audio");
    }

    #[test]
    fn merged_events_sort_deterministically_and_keep_partial_before_final() {
        let events = vec![
            TranscriptEvent {
                event_type: "final",
                channel: "system".to_string(),
                segment_id: "system-0".to_string(),
                start_ms: 10,
                end_ms: 20,
                text: "sys-final".to_string(),
                source_final_segment_id: None,
            },
            TranscriptEvent {
                event_type: "partial",
                channel: "mic".to_string(),
                segment_id: "mic-0".to_string(),
                start_ms: 10,
                end_ms: 20,
                text: "mic-partial".to_string(),
                source_final_segment_id: None,
            },
            TranscriptEvent {
                event_type: "partial",
                channel: "system".to_string(),
                segment_id: "system-0".to_string(),
                start_ms: 10,
                end_ms: 20,
                text: "sys-partial".to_string(),
                source_final_segment_id: None,
            },
            TranscriptEvent {
                event_type: "final",
                channel: "mic".to_string(),
                segment_id: "mic-0".to_string(),
                start_ms: 10,
                end_ms: 20,
                text: "mic-final".to_string(),
                source_final_segment_id: None,
            },
        ];
        let ordered = merge_transcript_events(events);
        let ordered_tuples: Vec<(&str, &str)> = ordered
            .iter()
            .map(|event| (event.event_type, event.channel.as_str()))
            .collect();
        assert_eq!(
            ordered_tuples,
            vec![
                ("partial", "mic"),
                ("partial", "system"),
                ("final", "mic"),
                ("final", "system"),
            ]
        );
    }

    #[test]
    fn tty_terminal_render_actions_overwrite_partials_before_stable_lines() {
        let actions = build_terminal_render_actions(
            &[
                TranscriptEvent {
                    event_type: "partial",
                    channel: "mic".to_string(),
                    segment_id: "mic-segment-0000".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    text: "hello".to_string(),
                    source_final_segment_id: None,
                },
                TranscriptEvent {
                    event_type: "partial",
                    channel: "mic".to_string(),
                    segment_id: "mic-segment-0000".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    text: "hello".to_string(),
                    source_final_segment_id: None,
                },
                TranscriptEvent {
                    event_type: "final",
                    channel: "mic".to_string(),
                    segment_id: "mic-segment-0000".to_string(),
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "hello world".to_string(),
                    source_final_segment_id: None,
                },
            ],
            TerminalRenderMode::InteractiveTty,
        );

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].kind, TerminalRenderActionKind::PartialOverwrite);
        assert_eq!(actions[0].line, "[00:00.000-00:00.500] mic ~ hello");
        assert_eq!(actions[1].kind, TerminalRenderActionKind::StableLine);
        assert_eq!(actions[1].line, "[00:00.000-00:01.000] mic: hello world");
    }

    #[test]
    fn non_tty_terminal_render_actions_skip_partials_and_keep_stable_order() {
        let actions = build_terminal_render_actions(
            &merge_transcript_events(vec![
                TranscriptEvent {
                    event_type: "llm_final",
                    channel: "mic".to_string(),
                    segment_id: "mic-0-llm".to_string(),
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "hello there".to_string(),
                    source_final_segment_id: Some("mic-0".to_string()),
                },
                TranscriptEvent {
                    event_type: "partial",
                    channel: "mic".to_string(),
                    segment_id: "mic-0".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    text: "hello".to_string(),
                    source_final_segment_id: None,
                },
                TranscriptEvent {
                    event_type: "reconciled_final",
                    channel: "system".to_string(),
                    segment_id: "system-0-reconciled".to_string(),
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "world reconciled".to_string(),
                    source_final_segment_id: Some("system-0".to_string()),
                },
                TranscriptEvent {
                    event_type: "final",
                    channel: "mic".to_string(),
                    segment_id: "mic-0".to_string(),
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "hello world".to_string(),
                    source_final_segment_id: None,
                },
            ]),
            TerminalRenderMode::DeterministicNonTty,
        );

        let stable_lines = actions
            .iter()
            .map(|action| (action.kind, action.line.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            stable_lines,
            vec![
                (
                    TerminalRenderActionKind::StableLine,
                    "[00:00.000-00:01.000] mic: hello world",
                ),
                (
                    TerminalRenderActionKind::StableLine,
                    "[00:00.000-00:01.000] system: world reconciled [reconciled_final]",
                ),
                (
                    TerminalRenderActionKind::StableLine,
                    "[00:00.000-00:01.000] mic: hello there [llm_final]",
                ),
            ]
        );
    }

    #[test]
    fn live_terminal_render_actions_emit_non_tty_stable_fallback_for_live_mode() {
        let mut config = TranscribeConfig::default();
        config.live_chunked = true;

        let actions = live_terminal_render_actions(
            &config,
            &merge_transcript_events(vec![
                TranscriptEvent {
                    event_type: "partial",
                    channel: "mic".to_string(),
                    segment_id: "mic-0".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    text: "hello".to_string(),
                    source_final_segment_id: None,
                },
                TranscriptEvent {
                    event_type: "final",
                    channel: "mic".to_string(),
                    segment_id: "mic-0".to_string(),
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "hello world".to_string(),
                    source_final_segment_id: None,
                },
            ]),
            TerminalRenderMode::DeterministicNonTty,
        );

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, TerminalRenderActionKind::StableLine);
        assert_eq!(actions[0].line, "[00:00.000-00:01.000] mic: hello world");
    }

    #[test]
    fn live_terminal_render_actions_skip_non_live_modes() {
        let config = TranscribeConfig::default();
        let actions = live_terminal_render_actions(
            &config,
            &[TranscriptEvent {
                event_type: "final",
                channel: "mic".to_string(),
                segment_id: "mic-0".to_string(),
                start_ms: 0,
                end_ms: 1_000,
                text: "hello world".to_string(),
                source_final_segment_id: None,
            }],
            TerminalRenderMode::DeterministicNonTty,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn live_close_summary_lines_use_contract_field_order() {
        let mut config = TranscribeConfig::default();
        config.duration_sec = 42;
        config.out_wav = PathBuf::from("artifacts/run.wav");
        config.out_jsonl = PathBuf::from("artifacts/run.jsonl");
        config.out_manifest = PathBuf::from("artifacts/run.manifest.json");

        let report = LiveRunReport {
            generated_at_utc: "2026-03-01T00:00:00Z".to_string(),
            backend_id: "whispercpp",
            resolved_model_path: PathBuf::from("models/test.bin"),
            resolved_model_source: "test".to_string(),
            channel_mode: ChannelMode::Separate,
            active_channel_mode: ChannelMode::Separate,
            transcript_text: "hello".to_string(),
            channel_transcripts: vec![super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "hello".to_string(),
            }],
            vad_boundaries: vec![VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 1_000,
                source: "energy_threshold",
            }],
            events: vec![
                TranscriptEvent {
                    event_type: "partial",
                    channel: "mic".to_string(),
                    segment_id: "mic-0".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    text: "hello".to_string(),
                    source_final_segment_id: None,
                },
                final_event("mic-0", "mic", "hello world"),
                TranscriptEvent {
                    event_type: "reconciled_final",
                    channel: "mic".to_string(),
                    segment_id: "mic-0-reconciled".to_string(),
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "hello world".to_string(),
                    source_final_segment_id: Some("mic-0".to_string()),
                },
            ],
            degradation_events: vec![ModeDegradationEvent {
                code: "queue_pressure",
                detail: "drop-oldest path".to_string(),
            }],
            trust_notices: vec![super::TrustNotice {
                code: "chunk_queue_backpressure".to_string(),
                severity: "warn".to_string(),
                cause: "queue pressure".to_string(),
                impact: "latency".to_string(),
                guidance: "raise cap".to_string(),
            }],
            lifecycle: sample_lifecycle(),
            reconciliation: ReconciliationMatrix::none(),
            asr_worker_pool: LiveAsrPoolTelemetry {
                prewarm_ok: true,
                submitted: 3,
                enqueued: 2,
                dropped_queue_full: 1,
                processed: 2,
                succeeded: 1,
                failed: 2,
                retry_attempts: 0,
                temp_audio_deleted: 1,
                temp_audio_retained: 1,
            },
            final_buffering: FinalBufferingTelemetry::default(),
            chunk_queue: LiveChunkQueueTelemetry::disabled(&config),
            cleanup_queue: CleanupQueueTelemetry::disabled(&config),
            benchmark: BenchmarkSummary {
                run_count: 1,
                wall_ms_p50: 1.0,
                wall_ms_p95: 1.0,
                partial_slo_met: true,
                final_slo_met: true,
            },
            benchmark_summary_csv: PathBuf::from("artifacts/summary.csv"),
            benchmark_runs_csv: PathBuf::from("artifacts/runs.csv"),
        };

        let lines = build_live_close_summary_lines(&config, &report);
        let field_order = lines
            .iter()
            .map(|line| line.split('=').next().unwrap_or(""))
            .collect::<Vec<_>>();
        assert_eq!(
            field_order,
            vec![
                "session_status",
                "duration_sec",
                "channel_mode_requested",
                "channel_mode_active",
                "transcript_events",
                "chunk_queue",
                "chunk_lag",
                "trust_notices",
                "degradation_events",
                "cleanup_queue",
                "artifacts",
            ]
        );
        assert!(lines[0].contains("degraded"));
    }

    #[test]
    fn startup_banner_lines_use_contract_field_order() {
        let mut config = TranscribeConfig::default();
        config.live_stream = true;
        config.duration_sec = 42;
        config.input_wav = PathBuf::from("artifacts/live.input.wav");
        config.out_wav = PathBuf::from("artifacts/run.wav");
        config.out_jsonl = PathBuf::from("artifacts/run.jsonl");
        config.out_manifest = PathBuf::from("artifacts/run.manifest.json");

        let lines = build_startup_banner_lines(&config);
        let field_order = lines
            .iter()
            .map(|line| line.split('=').next().unwrap_or(""))
            .collect::<Vec<_>>();
        assert_eq!(
            field_order,
            vec![
                "runtime_mode",
                "runtime_mode_taxonomy",
                "runtime_mode_selector",
                "runtime_mode_status",
                "channel_mode_requested",
                "duration_sec",
                "input_wav",
                "artifacts",
            ]
        );
        assert_eq!(lines[0], "runtime_mode=live-stream");
        assert_eq!(lines[3], "runtime_mode_status=implemented");
        assert!(lines[7].starts_with("artifacts=out_wav:"));
        assert!(lines[7].contains(" out_jsonl:"));
        assert!(lines[7].contains(" out_manifest:"));
        assert!(lines[7].contains("run.wav"));
        assert!(lines[7].contains("run.jsonl"));
        assert!(lines[7].contains("run.manifest.json"));
    }

    #[test]
    fn remediation_hints_are_deterministic_and_deduplicated() {
        let config = TranscribeConfig::default();
        let report = LiveRunReport {
            generated_at_utc: "2026-03-01T00:00:00Z".to_string(),
            backend_id: "whispercpp",
            resolved_model_path: PathBuf::from("models/test.bin"),
            resolved_model_source: "test".to_string(),
            channel_mode: ChannelMode::Separate,
            active_channel_mode: ChannelMode::Separate,
            transcript_text: "hello".to_string(),
            channel_transcripts: vec![super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "hello".to_string(),
            }],
            vad_boundaries: vec![VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 1_000,
                source: "energy_threshold",
            }],
            events: vec![final_event("mic-0", "mic", "hello world")],
            degradation_events: vec![ModeDegradationEvent {
                code: "queue_pressure",
                detail: "drop-oldest path".to_string(),
            }],
            trust_notices: vec![
                super::TrustNotice {
                    code: "chunk_queue_backpressure".to_string(),
                    severity: "warn".to_string(),
                    cause: "queue pressure".to_string(),
                    impact: "latency".to_string(),
                    guidance: "raise cap".to_string(),
                },
                super::TrustNotice {
                    code: "cleanup_queue_timeout".to_string(),
                    severity: "warn".to_string(),
                    cause: "cleanup timeout".to_string(),
                    impact: "post-processing lag".to_string(),
                    guidance: "raise cap".to_string(),
                },
                super::TrustNotice {
                    code: "capture_restart".to_string(),
                    severity: "warn".to_string(),
                    cause: "capture interruption".to_string(),
                    impact: "continuity risk".to_string(),
                    guidance: "inspect the runtime manifest trust section".to_string(),
                },
            ],
            lifecycle: sample_lifecycle(),
            reconciliation: ReconciliationMatrix::none(),
            asr_worker_pool: LiveAsrPoolTelemetry {
                prewarm_ok: true,
                submitted: 1,
                enqueued: 1,
                dropped_queue_full: 0,
                processed: 1,
                succeeded: 1,
                failed: 0,
                retry_attempts: 0,
                temp_audio_deleted: 1,
                temp_audio_retained: 0,
            },
            final_buffering: FinalBufferingTelemetry::default(),
            chunk_queue: LiveChunkQueueTelemetry::disabled(&config),
            cleanup_queue: CleanupQueueTelemetry::disabled(&config),
            benchmark: BenchmarkSummary {
                run_count: 1,
                wall_ms_p50: 1.0,
                wall_ms_p95: 1.0,
                partial_slo_met: true,
                final_slo_met: true,
            },
            benchmark_summary_csv: PathBuf::from("artifacts/summary.csv"),
            benchmark_runs_csv: PathBuf::from("artifacts/runs.csv"),
        };

        let hints = top_remediation_hints(&report, 3);
        assert_eq!(
            hints,
            vec![
                "inspect the runtime manifest trust section".to_string(),
                "raise cap".to_string(),
            ]
        );
        assert_eq!(
            remediation_hints_csv(&hints),
            "inspect the runtime manifest trust section | raise cap"
        );
    }

    #[test]
    fn reconstructed_transcript_prefers_reconciled_final_events_when_present() {
        let events = vec![
            TranscriptEvent {
                event_type: "final",
                channel: "mic".to_string(),
                segment_id: "mic-chunk-0000".to_string(),
                start_ms: 0,
                end_ms: 2_000,
                text: "incomplete".to_string(),
                source_final_segment_id: None,
            },
            TranscriptEvent {
                event_type: "reconciled_final",
                channel: "mic".to_string(),
                segment_id: "mic-reconciled".to_string(),
                start_ms: 0,
                end_ms: 2_000,
                text: "complete transcript".to_string(),
                source_final_segment_id: None,
            },
        ];
        let transcript = reconstruct_transcript(&events);
        assert!(transcript.contains("complete transcript"));
        assert!(!transcript.contains("incomplete"));
    }

    #[test]
    fn backlog_drop_scenarios_emit_reconciled_final_events() {
        let live = run_live_chunk_queue(
            vec![
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 0,
                    channel: "mic".to_string(),
                    segment_id: "mic-0".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "mic-live-0".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 0,
                    channel: "system".to_string(),
                    segment_id: "system-0".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "system-live-0".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 1,
                    channel: "mic".to_string(),
                    segment_id: "mic-1".to_string(),
                    start_ms: 1_000,
                    end_ms: 5_000,
                    text: "mic-live-1".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 1,
                    channel: "system".to_string(),
                    segment_id: "system-1".to_string(),
                    start_ms: 1_000,
                    end_ms: 5_000,
                    text: "system-live-1".to_string(),
                    source_final_segment_id: None,
                },
            ],
            1,
            1_000,
        );
        assert!(live.telemetry.dropped_oldest > 0);

        let reconciliation = build_reconciliation_events(
            &[
                super::ChannelTranscriptSummary {
                    role: "mic",
                    label: "mic".to_string(),
                    text: "mic complete transcript".to_string(),
                },
                super::ChannelTranscriptSummary {
                    role: "system",
                    label: "system".to_string(),
                    text: "system complete transcript".to_string(),
                },
            ],
            &[VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 5_000,
                source: "energy_threshold",
            }],
        );

        assert!(!reconciliation.is_empty());
        assert!(
            reconciliation
                .iter()
                .all(|event| event.event_type == "reconciled_final")
        );
        assert!(
            reconciliation
                .iter()
                .all(|event| event.segment_id.ends_with("-reconciled"))
        );
    }

    #[test]
    fn reconciliation_events_include_source_final_segment_lineage() {
        let events = build_reconciliation_events(
            &[super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "mic complete transcript".to_string(),
            }],
            &[VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 5_000,
                source: "energy_threshold",
            }],
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "reconciled_final");
        assert_eq!(
            events[0].source_final_segment_id.as_deref(),
            Some("mic-representative-0")
        );
    }

    #[test]
    fn targeted_reconciliation_limits_output_to_shutdown_flush_boundaries() {
        let events = build_targeted_reconciliation_events(
            &[super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "alpha beta gamma delta epsilon".to_string(),
            }],
            &[
                VadBoundary {
                    id: 2,
                    start_ms: 0,
                    end_ms: 1_200,
                    source: "energy_threshold",
                },
                VadBoundary {
                    id: 7,
                    start_ms: 1_700,
                    end_ms: 2_800,
                    source: "shutdown_flush",
                },
            ],
            &[TranscriptEvent {
                event_type: "final",
                channel: "mic".to_string(),
                segment_id: "mic-segment-0000-0-1200".to_string(),
                start_ms: 0,
                end_ms: 1_200,
                text: "alpha beta".to_string(),
                source_final_segment_id: None,
            }],
            &ReconciliationMatrix {
                required: true,
                applied: false,
                triggers: vec![super::ReconciliationTrigger {
                    code: "shutdown_flush_boundary",
                }],
            },
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "reconciled_final");
        assert_eq!(events[0].start_ms, 1_700);
        assert_eq!(events[0].end_ms, 2_800);
        assert_eq!(
            events[0].source_final_segment_id.as_deref(),
            Some("mic-segment-0001-1700-2800")
        );
    }

    #[test]
    fn targeted_reconciliation_prefers_missing_final_boundaries_under_queue_drop() {
        let events = build_targeted_reconciliation_events(
            &[
                super::ChannelTranscriptSummary {
                    role: "mic",
                    label: "mic".to_string(),
                    text: "mic alpha beta gamma delta".to_string(),
                },
                super::ChannelTranscriptSummary {
                    role: "system",
                    label: "system".to_string(),
                    text: "system alpha beta gamma delta".to_string(),
                },
            ],
            &[
                VadBoundary {
                    id: 0,
                    start_ms: 0,
                    end_ms: 1_200,
                    source: "energy_threshold",
                },
                VadBoundary {
                    id: 1,
                    start_ms: 1_700,
                    end_ms: 2_800,
                    source: "energy_threshold",
                },
            ],
            &[
                TranscriptEvent {
                    event_type: "final",
                    channel: "mic".to_string(),
                    segment_id: "mic-segment-0000-0-1200".to_string(),
                    start_ms: 0,
                    end_ms: 1_200,
                    text: "mic alpha".to_string(),
                    source_final_segment_id: None,
                },
                TranscriptEvent {
                    event_type: "final",
                    channel: "system".to_string(),
                    segment_id: "system-segment-0000-0-1200".to_string(),
                    start_ms: 0,
                    end_ms: 1_200,
                    text: "system alpha".to_string(),
                    source_final_segment_id: None,
                },
            ],
            &ReconciliationMatrix {
                required: true,
                applied: false,
                triggers: vec![super::ReconciliationTrigger {
                    code: "chunk_queue_drop_oldest",
                }],
            },
        );

        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|event| event.event_type == "reconciled_final")
        );
        assert!(events.iter().all(|event| event.start_ms == 1_700));
        let lineage = events
            .iter()
            .map(|event| event.source_final_segment_id.clone().unwrap())
            .collect::<Vec<_>>();
        assert!(lineage.contains(&"mic-segment-0001-1700-2800".to_string()));
        assert!(lineage.contains(&"system-segment-0001-1700-2800".to_string()));
    }

    #[test]
    fn reconciliation_preserves_live_provenance_and_improves_or_preserves_completeness() {
        let live = run_live_chunk_queue(
            vec![
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 0,
                    channel: "mic".to_string(),
                    segment_id: "mic-0".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "mic-live-0".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 0,
                    channel: "system".to_string(),
                    segment_id: "system-0".to_string(),
                    start_ms: 0,
                    end_ms: 4_000,
                    text: "system-live-0".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 1,
                    channel: "mic".to_string(),
                    segment_id: "mic-1".to_string(),
                    start_ms: 1_000,
                    end_ms: 5_000,
                    text: "mic-live-1".to_string(),
                    source_final_segment_id: None,
                },
                AsrWorkItem {
                    class: AsrWorkClass::Final,
                    tick_index: 1,
                    channel: "system".to_string(),
                    segment_id: "system-1".to_string(),
                    start_ms: 1_000,
                    end_ms: 5_000,
                    text: "system-live-1".to_string(),
                    source_final_segment_id: None,
                },
            ],
            1,
            1_000,
        );
        assert!(live.telemetry.dropped_oldest > 0);
        assert!(live.events.iter().any(|event| event.event_type == "final"));

        let baseline_text = reconstruct_transcript(&live.events);
        let mut merged = live.events.clone();
        merged.extend(build_reconciliation_events(
            &[
                super::ChannelTranscriptSummary {
                    role: "mic",
                    label: "mic".to_string(),
                    text: "mic complete transcript".to_string(),
                },
                super::ChannelTranscriptSummary {
                    role: "system",
                    label: "system".to_string(),
                    text: "system complete transcript".to_string(),
                },
            ],
            &[VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 5_000,
                source: "energy_threshold",
            }],
        ));
        let merged = merge_transcript_events(merged);
        let reconciled_text = reconstruct_transcript(&merged);

        assert!(
            merged
                .iter()
                .any(|event| event.event_type == "reconciled_final")
        );
        assert!(merged.iter().any(|event| event.event_type == "final"));
        assert!(
            merged
                .iter()
                .filter(|event| event.event_type == "reconciled_final")
                .all(|event| event.source_final_segment_id.is_some())
        );
        assert!(reconciled_text.len() >= baseline_text.len());
        assert!(reconciled_text.contains("mic complete transcript"));
        assert!(reconciled_text.contains("system complete transcript"));
    }

    #[test]
    fn reconstructed_transcript_uses_timestamped_readability_defaults() {
        let events = vec![
            TranscriptEvent {
                event_type: "partial",
                channel: "mic".to_string(),
                segment_id: "mic-0".to_string(),
                start_ms: 0,
                end_ms: 50,
                text: "hello".to_string(),
                source_final_segment_id: None,
            },
            TranscriptEvent {
                event_type: "final",
                channel: "mic".to_string(),
                segment_id: "mic-0".to_string(),
                start_ms: 0,
                end_ms: 100,
                text: "hello from mic".to_string(),
                source_final_segment_id: None,
            },
            TranscriptEvent {
                event_type: "final",
                channel: "system".to_string(),
                segment_id: "system-0".to_string(),
                start_ms: 0,
                end_ms: 100,
                text: "hello from system".to_string(),
                source_final_segment_id: None,
            },
        ];
        let reconstructed = reconstruct_transcript(&events);
        assert_eq!(
            reconstructed,
            "[00:00.000-00:00.100] mic: hello from mic\n[00:00.000-00:00.100] system: hello from system (overlap<=120ms with mic)"
        );
    }

    #[test]
    fn per_channel_readable_transcript_is_deterministic() {
        let events = vec![
            TranscriptEvent {
                event_type: "final",
                channel: "system".to_string(),
                segment_id: "system-0".to_string(),
                start_ms: 0,
                end_ms: 90,
                text: "system one".to_string(),
                source_final_segment_id: None,
            },
            TranscriptEvent {
                event_type: "final",
                channel: "mic".to_string(),
                segment_id: "mic-0".to_string(),
                start_ms: 0,
                end_ms: 100,
                text: "mic one".to_string(),
                source_final_segment_id: None,
            },
            TranscriptEvent {
                event_type: "final",
                channel: "mic".to_string(),
                segment_id: "mic-1".to_string(),
                start_ms: 200,
                end_ms: 320,
                text: "mic two".to_string(),
                source_final_segment_id: None,
            },
        ];

        let per_channel = reconstruct_transcript_per_channel(&events);
        assert_eq!(per_channel.len(), 2);
        assert_eq!(per_channel[0].channel, "mic");
        assert_eq!(
            per_channel[0].text,
            "[00:00.000-00:00.100] mic one\n[00:00.200-00:00.320] mic two"
        );
        assert_eq!(per_channel[1].channel, "system");
        assert_eq!(per_channel[1].text, "[00:00.000-00:00.090] system one");
    }

    #[test]
    fn mixed_fallback_degrades_to_mixed_for_mono_input() {
        let input_wav = write_test_mono_wav();
        let mut config = TranscribeConfig::default();
        config.input_wav = input_wav.clone();
        config.channel_mode = ChannelMode::MixedFallback;

        let plan = super::runtime_representative::prepare_channel_inputs(&config, "unit-fallback")
            .unwrap();
        assert_eq!(plan.active_mode, ChannelMode::Mixed);
        assert_eq!(plan.inputs.len(), 1);
        assert_eq!(plan.inputs[0].role, "mixed");
        assert_eq!(plan.inputs[0].label, "merged");
        assert_eq!(plan.degradation_events.len(), 1);
        assert_eq!(plan.degradation_events[0].code, "fallback_to_mixed");

        let _ = std::fs::remove_file(input_wav);
    }

    #[test]
    fn out_wav_is_materialized_when_output_differs_from_input() {
        let input_wav = write_test_mono_wav();
        let temp_dir = write_temp_dir("recordit-out-wav-copy");
        let out_wav = temp_dir.join("nested").join("session.wav");

        materialize_out_wav(&input_wav, &out_wav).unwrap();

        assert!(out_wav.exists());
        assert_eq!(fs::read(&input_wav).unwrap(), fs::read(&out_wav).unwrap());

        let _ = fs::remove_file(input_wav);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn out_wav_materialization_is_noop_for_same_path() {
        let input_wav = write_test_mono_wav();
        materialize_out_wav(&input_wav, &input_wav).unwrap();
        assert!(input_wav.exists());
        let _ = fs::remove_file(input_wav);
    }

    #[test]
    fn live_capture_output_path_uses_input_wav_for_live_stream() {
        let mut config = TranscribeConfig::default();
        config.live_stream = true;
        config.live_chunked = true;
        config.input_wav = PathBuf::from("artifacts/live-stream.input.wav");
        config.out_wav = PathBuf::from("artifacts/live-stream.out.wav");

        assert_eq!(
            live_capture_output_path(&config),
            Path::new("artifacts/live-stream.input.wav")
        );
    }

    #[test]
    fn live_capture_materialization_paths_follow_runtime_mode_semantics() {
        let mut live_stream = TranscribeConfig::default();
        live_stream.live_stream = true;
        live_stream.live_chunked = true;
        live_stream.input_wav = PathBuf::from("artifacts/stream.input.wav");
        live_stream.out_wav = PathBuf::from("artifacts/stream.out.wav");

        let (stream_from, stream_to) = live_capture_materialization_paths(&live_stream);
        assert_eq!(stream_from, Path::new("artifacts/stream.input.wav"));
        assert_eq!(stream_to, Path::new("artifacts/stream.out.wav"));

        let mut chunked = TranscribeConfig::default();
        chunked.live_stream = false;
        chunked.live_chunked = true;
        chunked.input_wav = PathBuf::from("artifacts/chunked.input.wav");
        chunked.out_wav = PathBuf::from("artifacts/chunked.out.wav");

        let (chunked_from, chunked_to) = live_capture_materialization_paths(&chunked);
        assert_eq!(chunked_from, Path::new("artifacts/chunked.out.wav"));
        assert_eq!(chunked_to, Path::new("artifacts/chunked.input.wav"));
    }

    #[test]
    fn streaming_capture_session_grows_live_stream_input_wav_during_runtime() {
        let temp_dir = write_temp_dir("recordit-live-stream-input-growth");
        let fixture = temp_dir.join("fixture.wav");
        let input_wav = temp_dir.join("runtime.input.wav");
        write_test_stereo_wav(&fixture, 200, 600);

        let capture_config = LiveCaptureConfig {
            duration_secs: 3,
            output: input_wav.clone(),
            target_rate_hz: 200,
            mismatch_policy: LiveCaptureSampleRateMismatchPolicy::AdaptStreamRate,
            callback_contract_mode: LiveCaptureCallbackMode::Warn,
        };

        let _guard = env_lock().lock().unwrap();
        let original_fixture = env::var("RECORDIT_FAKE_CAPTURE_FIXTURE").ok();
        let original_realtime = env::var("RECORDIT_FAKE_CAPTURE_REALTIME").ok();
        let original_restart = env::var("RECORDIT_FAKE_CAPTURE_RESTART_COUNT").ok();
        unsafe {
            env::set_var("RECORDIT_FAKE_CAPTURE_FIXTURE", &fixture);
            env::set_var("RECORDIT_FAKE_CAPTURE_REALTIME", "1");
            env::set_var("RECORDIT_FAKE_CAPTURE_RESTART_COUNT", "0");
        }

        let worker = std::thread::spawn({
            let config = capture_config.clone();
            move || {
                let mut sink = NoopCaptureSink;
                run_streaming_capture_session(&config, &mut sink)
            }
        });

        let mut observed_growth = false;
        for _ in 0..120 {
            if let Ok(metadata) = fs::metadata(&input_wav) {
                if metadata.len() > 44 {
                    observed_growth = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        let capture_result = worker.join().unwrap().unwrap();
        restore_optional_env("RECORDIT_FAKE_CAPTURE_FIXTURE", original_fixture);
        restore_optional_env("RECORDIT_FAKE_CAPTURE_REALTIME", original_realtime);
        restore_optional_env("RECORDIT_FAKE_CAPTURE_RESTART_COUNT", original_restart);

        assert!(
            observed_growth,
            "expected input WAV growth during active capture"
        );
        assert!(input_wav.is_file());
        assert!(fs::metadata(&input_wav).unwrap().len() > 44);
        assert_eq!(capture_result.progressive_output_path, input_wav);

        let telemetry_path = super::live_capture_telemetry_path(&input_wav);
        let _ = fs::remove_file(telemetry_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn input_wav_semantics_describes_mode_roles() {
        let mut config = TranscribeConfig::default();

        assert_eq!(
            input_wav_semantics(&config),
            "representative offline fixture input path"
        );

        config.live_chunked = true;
        assert_eq!(
            input_wav_semantics(&config),
            "representative live scratch artifact mirrored from canonical out_wav after capture"
        );

        config.live_stream = true;
        assert_eq!(
            input_wav_semantics(&config),
            "progressive live capture scratch artifact (materialized into canonical out_wav on success)"
        );
    }

    #[test]
    fn cleanup_queue_drops_when_full_without_blocking() {
        let mut config = TranscribeConfig::default();
        config.llm_cleanup = true;
        config.llm_endpoint = Some("http://127.0.0.1:9/v1/chat/completions".to_string());
        config.llm_model = Some("dummy".to_string());
        config.llm_max_queue = 1;
        config.llm_timeout_ms = 150;
        config.llm_retries = 0;

        let events = vec![
            final_event("mic-0", "mic", "hello"),
            final_event("system-0", "system", "world"),
            final_event("mixed-0", "merged", "again"),
        ];

        let result = run_cleanup_queue_with(&config, &events, |_client, _request| {
            std::thread::sleep(Duration::from_millis(40));
            CleanupAttemptOutcome {
                status: CleanupTaskStatus::Failed,
                cleaned_text: None,
            }
        });
        let telemetry = result.telemetry;

        assert_eq!(telemetry.submitted, 3);
        assert!(telemetry.dropped_queue_full >= 1);
        assert!(telemetry.enqueued <= telemetry.submitted);
        assert_eq!(telemetry.processed, telemetry.enqueued);
        assert!(telemetry.failed >= telemetry.processed);
        assert!(telemetry.drain_completed);
    }

    #[test]
    fn final_buffering_retries_without_queue_drop() {
        let temp_dir = write_temp_dir("recordit-final-buffering");
        let executor = Arc::new(MockLiveAsrExecutor {
            prewarm_calls: AtomicUsize::new(0),
            transcribe_calls: AtomicUsize::new(0),
            sleep_ms: 20,
        });
        let jobs = (0..4)
            .map(|idx| {
                let audio_path = temp_dir.join(format!("job-{idx}.wav"));
                fs::write(&audio_path, b"tmp").unwrap();
                LiveAsrJob {
                    job_id: idx,
                    class: LiveAsrJobClass::Final,
                    role: if idx % 2 == 0 { "mic" } else { "system" },
                    label: format!("chan-{idx}"),
                    segment_id: format!("seg-{idx}"),
                    audio_path,
                    is_temp_audio: true,
                }
            })
            .collect::<Vec<_>>();

        let (results, telemetry, final_buffering) =
            super::runtime_representative::run_live_asr_pool_with_final_buffering(
                executor.clone(),
                jobs,
                LiveAsrPoolConfig {
                    worker_count: 1,
                    queue_capacity: 1,
                    retries: 0,
                    temp_audio_policy: TempAudioPolicy::RetainOnFailure,
                },
            );

        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|result| result.success()));
        assert_eq!(telemetry.submitted, 4);
        assert_eq!(telemetry.succeeded, 4);
        assert_eq!(telemetry.failed, 0);
        assert_eq!(telemetry.dropped_queue_full, 0);
        assert_eq!(telemetry.temp_audio_deleted, 4);
        assert_eq!(final_buffering.submit_window, 1);
        assert_eq!(final_buffering.deferred_final_submissions, 3);
        assert_eq!(final_buffering.max_pending_final_backlog, 3);
        assert_eq!(executor.prewarm_calls.load(Ordering::Relaxed), 1);
        assert_eq!(executor.transcribe_calls.load(Ordering::Relaxed), 4);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn cleanup_queue_counts_retry_attempts() {
        let mut config = TranscribeConfig::default();
        config.llm_cleanup = true;
        config.llm_endpoint = Some("http://127.0.0.1:9/v1/chat/completions".to_string());
        config.llm_model = Some("dummy".to_string());
        config.llm_max_queue = 2;
        config.llm_timeout_ms = 200;
        config.llm_retries = 2;

        let attempts = Arc::new(AtomicUsize::new(0));
        let attempt_counter = Arc::clone(&attempts);
        let events = vec![final_event("mic-0", "mic", "hello")];

        let result = run_cleanup_queue_with(&config, &events, move |_client, _request| {
            let attempt = attempt_counter.fetch_add(1, Ordering::Relaxed);
            if attempt < 2 {
                CleanupAttemptOutcome {
                    status: CleanupTaskStatus::Failed,
                    cleaned_text: None,
                }
            } else {
                CleanupAttemptOutcome {
                    status: CleanupTaskStatus::Succeeded,
                    cleaned_text: Some("cleaned".to_string()),
                }
            }
        });
        let telemetry = result.telemetry;

        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        assert_eq!(telemetry.retry_attempts, 2);
        assert_eq!(telemetry.succeeded, 1);
        assert_eq!(telemetry.failed, 0);
    }

    #[test]
    fn cleanup_queue_emits_llm_final_with_lineage() {
        let mut config = TranscribeConfig::default();
        config.llm_cleanup = true;
        config.llm_endpoint = Some("http://127.0.0.1:9/v1/chat/completions".to_string());
        config.llm_model = Some("dummy".to_string());
        config.llm_max_queue = 2;
        config.llm_timeout_ms = 200;
        config.llm_retries = 0;

        let events = vec![final_event("mic-0", "mic", "hello there")];
        let result = run_cleanup_queue_with(&config, &events, |_client, _request| {
            CleanupAttemptOutcome {
                status: CleanupTaskStatus::Succeeded,
                cleaned_text: Some("hello there.".to_string()),
            }
        });

        assert_eq!(result.llm_events.len(), 1);
        let llm_event = &result.llm_events[0];
        assert_eq!(llm_event.event_type, "llm_final");
        assert_eq!(llm_event.segment_id, "mic-0-llm");
        assert_eq!(llm_event.source_final_segment_id.as_deref(), Some("mic-0"));
        assert_eq!(llm_event.text, "hello there.");
    }

    #[test]
    fn cleanup_disabled_produces_no_llm_events() {
        let config = TranscribeConfig::default();
        let events = vec![final_event("mic-0", "mic", "hello there")];
        let result = run_cleanup_queue_with(&config, &events, |_client, _request| {
            panic!("cleanup invoker should not run when llm_cleanup is disabled")
        });

        assert!(!result.telemetry.enabled);
        assert_eq!(result.telemetry.submitted, 0);
        assert!(result.llm_events.is_empty());
    }

    #[test]
    fn model_resolution_prefers_cli_override() {
        let _guard = env_lock().lock().unwrap();
        let original_cwd = env::current_dir().unwrap();
        let original_env = env::var("RECORDIT_ASR_MODEL").ok();
        let temp_dir = write_temp_dir("recordit-model-cli");
        let cli_model = temp_dir.join("cli-model.bin");
        let env_model = temp_dir.join("env-model.bin");
        File::create(&cli_model).unwrap();
        File::create(&env_model).unwrap();

        env::set_current_dir(&temp_dir).unwrap();
        unsafe {
            env::set_var("RECORDIT_ASR_MODEL", &env_model);
        }

        let mut config = TranscribeConfig::default();
        config.asr_model = cli_model.clone();
        let resolved = resolve_model_path(&config).unwrap();

        assert_eq!(resolved.path, cli_model);
        assert_eq!(resolved.source, "cli --asr-model");

        restore_env_state(original_cwd, original_env);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn model_resolution_uses_env_override_when_cli_missing() {
        let _guard = env_lock().lock().unwrap();
        let original_cwd = env::current_dir().unwrap();
        let original_env = env::var("RECORDIT_ASR_MODEL").ok();
        let temp_dir = write_temp_dir("recordit-model-env");
        let env_model = temp_dir.join("env-model.bin");
        File::create(&env_model).unwrap();

        env::set_current_dir(&temp_dir).unwrap();
        unsafe {
            env::set_var("RECORDIT_ASR_MODEL", &env_model);
        }

        let config = TranscribeConfig::default();
        let resolved = resolve_model_path(&config).unwrap();

        assert_eq!(resolved.path, env_model);
        assert_eq!(resolved.source, "env RECORDIT_ASR_MODEL");

        restore_env_state(original_cwd, original_env);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn model_resolution_uses_repo_default_when_overrides_missing() {
        let _guard = env_lock().lock().unwrap();
        let original_cwd = env::current_dir().unwrap();
        let original_env = env::var("RECORDIT_ASR_MODEL").ok();
        let temp_dir = write_temp_dir("recordit-model-default");
        let default_model = temp_dir.join("artifacts/bench/models/whispercpp/ggml-tiny.en.bin");
        fs::create_dir_all(default_model.parent().unwrap()).unwrap();
        File::create(&default_model).unwrap();

        env::set_current_dir(&temp_dir).unwrap();
        unsafe {
            env::remove_var("RECORDIT_ASR_MODEL");
        }

        let config = TranscribeConfig::default();
        let resolved = resolve_model_path(&config).unwrap();

        assert_eq!(
            fs::canonicalize(&resolved.path).unwrap(),
            fs::canonicalize(&default_model).unwrap()
        );
        assert_eq!(resolved.source, "repo benchmark default");

        restore_env_state(original_cwd, original_env);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn model_checksum_info_reports_available_for_file_model() {
        let temp_dir = write_temp_dir("recordit-model-checksum-file");
        let model_path = temp_dir.join("model.bin");
        fs::write(&model_path, b"recordit-model").unwrap();

        let checksum = model_checksum_info(Some(&ResolvedModelPath {
            path: model_path,
            source: "test".to_string(),
        }));
        assert_eq!(checksum.status, "available");
        assert_eq!(
            checksum.sha256,
            "282b60bfec2d3545ed794a23d6790b4b8d558b4b89c334b4e0eeb6b446dbcafe"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn model_checksum_info_reports_unavailable_for_directory_model() {
        let temp_dir = write_temp_dir("recordit-model-checksum-dir");
        let checksum = model_checksum_info(Some(&ResolvedModelPath {
            path: temp_dir.clone(),
            source: "test".to_string(),
        }));
        assert_eq!(checksum.status, "unavailable_directory");
        assert_eq!(checksum.sha256, "<unavailable>");
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn model_checksum_info_reports_unresolved_when_model_missing() {
        let checksum = model_checksum_info(None);
        assert_eq!(checksum.status, "unavailable_unresolved");
        assert_eq!(checksum.sha256, "<unavailable>");
    }

    #[test]
    fn lifecycle_telemetry_tracks_expected_phase_order() {
        let mut lifecycle = LiveLifecycleTelemetry::new();
        lifecycle.transition(LiveLifecyclePhase::Warmup, "prep");
        lifecycle.transition(LiveLifecyclePhase::Active, "stream");
        lifecycle.transition(LiveLifecyclePhase::Draining, "flush");
        lifecycle.transition(LiveLifecyclePhase::Shutdown, "done");

        assert_eq!(
            lifecycle
                .transitions
                .iter()
                .map(|transition| transition.phase)
                .collect::<Vec<_>>(),
            vec![
                LiveLifecyclePhase::Warmup,
                LiveLifecyclePhase::Active,
                LiveLifecyclePhase::Draining,
                LiveLifecyclePhase::Shutdown
            ]
        );
        assert_eq!(lifecycle.current_phase, LiveLifecyclePhase::Shutdown);
        assert!(lifecycle.ready_for_transcripts);
        assert!(!lifecycle.transitions[0].phase.ready_for_transcripts());
        assert!(lifecycle.transitions[1].phase.ready_for_transcripts());
    }

    #[test]
    fn runtime_manifest_includes_ordered_event_timeline() {
        let temp_dir = write_temp_dir("recordit-runtime-manifest-events");
        let input_wav = temp_dir.join("input.wav");
        let out_wav = temp_dir.join("session.wav");
        let out_jsonl = temp_dir.join("session.jsonl");
        let out_manifest = temp_dir.join("session.manifest.json");
        File::create(&input_wav).unwrap();
        File::create(&out_wav).unwrap();

        let mut config = TranscribeConfig::default();
        config.input_wav = input_wav.clone();
        config.out_wav = out_wav.clone();
        config.out_jsonl = out_jsonl.clone();
        config.out_manifest = out_manifest.clone();
        let mut chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        chunk_queue.enabled = true;
        chunk_queue.max_queue = 2;
        chunk_queue.submitted = 4;
        chunk_queue.enqueued = 4;
        chunk_queue.processed = 4;
        chunk_queue.high_water = 2;
        chunk_queue.lag_sample_count = 4;
        chunk_queue.lag_p50_ms = 1_000;
        chunk_queue.lag_p95_ms = 2_000;
        chunk_queue.lag_max_ms = 2_000;

        let report = LiveRunReport {
            generated_at_utc: "2026-02-28T00:00:00Z".to_string(),
            backend_id: "whispercpp",
            resolved_model_path: temp_dir.join("model.bin"),
            resolved_model_source: "test".to_string(),
            channel_mode: ChannelMode::Separate,
            active_channel_mode: ChannelMode::Separate,
            transcript_text: "hello".to_string(),
            channel_transcripts: vec![super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "hello".to_string(),
            }],
            vad_boundaries: vec![VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 100,
                source: "energy_threshold",
            }],
            events: vec![final_event("mic-0", "mic", "hello world")],
            degradation_events: Vec::new(),
            trust_notices: Vec::new(),
            lifecycle: sample_lifecycle(),
            reconciliation: ReconciliationMatrix::none(),
            asr_worker_pool: LiveAsrPoolTelemetry {
                prewarm_ok: true,
                submitted: 3,
                enqueued: 2,
                dropped_queue_full: 1,
                processed: 2,
                succeeded: 1,
                failed: 2,
                retry_attempts: 0,
                temp_audio_deleted: 1,
                temp_audio_retained: 1,
            },
            final_buffering: FinalBufferingTelemetry::default(),
            chunk_queue,
            cleanup_queue: CleanupQueueTelemetry::disabled(&config),
            benchmark: BenchmarkSummary {
                run_count: 1,
                wall_ms_p50: 1.0,
                wall_ms_p95: 1.0,
                partial_slo_met: true,
                final_slo_met: true,
            },
            benchmark_summary_csv: temp_dir.join("summary.csv"),
            benchmark_runs_csv: temp_dir.join("runs.csv"),
        };

        write_runtime_manifest(&config, &report).unwrap();
        let manifest = fs::read_to_string(&out_manifest).unwrap();
        assert!(manifest.contains("\"events\": ["));
        assert!(manifest.contains("\"event_type\":\"final\""));
        assert!(manifest.contains("\"segment_id\":\"mic-0\""));
        assert!(
            manifest
                .contains("\"input_wav_semantics\": \"representative offline fixture input path\"")
        );
        assert!(manifest.contains("\"out_wav_materialized\": true"));
        assert!(manifest.contains("\"out_wav_bytes\": 0"));
        assert!(manifest.contains("\"runtime_mode\": \"representative-offline\""));
        assert!(manifest.contains("\"runtime_mode_taxonomy\": \"representative-offline\""));
        assert!(manifest.contains("\"runtime_mode_selector\": \"<default>\""));
        assert!(manifest.contains("\"runtime_mode_status\": \"implemented\""));
        assert!(manifest.contains("\"lifecycle\": {"));
        assert!(manifest.contains("\"current_phase\":\"shutdown\""));
        assert!(manifest.contains("\"phase\":\"warmup\""));
        assert!(manifest.contains("\"phase\":\"active\""));
        assert!(manifest.contains("\"phase\":\"draining\""));
        assert!(manifest.contains("\"phase\":\"shutdown\""));
        assert!(manifest.contains("\"reconciliation\": {"));
        assert!(manifest.contains("\"asr_worker_pool\": {"));
        assert!(manifest.contains("\"temp_audio_deleted\":1"));
        assert!(manifest.contains("\"temp_audio_retained\":1"));
        assert!(manifest.contains("\"terminal_summary\": {"));
        assert!(manifest.contains("\"render_mode\":\"deterministic-non-tty\""));
        assert!(manifest.contains("\"stable_line_policy\":\"final-only\""));
        assert!(manifest.contains("\"stable_line_count\":1"));
        assert!(manifest.contains("\"stable_lines_replayed\":false"));
        assert!(manifest.contains("\"first_emit_timing_ms\": {"));
        assert!(manifest.contains("\"first_any\":100"));
        assert!(manifest.contains("\"first_partial\":null"));
        assert!(manifest.contains("\"first_final\":100"));
        assert!(manifest.contains("\"first_stable\":100"));
        assert!(manifest.contains("\"queue_defer\": {"));
        assert!(manifest.contains("\"deferred_final_submissions\":0"));
        assert!(manifest.contains("\"ordering_metadata\": {"));
        assert!(manifest.contains("\"event_sort_key\":\"start_ms,end_ms,event_type,channel,segment_id,source_final_segment_id,text\""));
        assert!(manifest.contains("\"event_counts\": {"));
        assert!(manifest.contains("\"partial\":0"));
        assert!(manifest.contains("\"final\":1"));
        assert!(manifest.contains("\"llm_final\":0"));
        assert!(manifest.contains("\"reconciled_final\":0"));
        assert!(manifest.contains("\"session_summary\": {"));
        assert!(manifest.contains("\"session_status\":\"ok\""));
        assert!(manifest.contains("\"duration_sec\":10"));
        assert!(manifest.contains("\"transcript_events\":{"));
        assert!(manifest.contains("\"trust_notices\":{\"count\":0"));
        assert!(manifest.contains("\"degradation_events\":{\"count\":0"));
        assert!(manifest.contains("\"artifacts\":{\"out_wav\":"));
        assert!(manifest.contains("\"lag_sample_count\":4"));
        assert!(manifest.contains("\"lag_p50_ms\":1000"));
        assert!(manifest.contains("\"lag_p95_ms\":2000"));
        assert!(manifest.contains("\"lag_max_ms\":2000"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn preflight_manifest_includes_runtime_mode_taxonomy_fields() {
        let temp_dir = write_temp_dir("recordit-preflight-manifest-mode-taxonomy");
        let out_manifest = temp_dir.join("preflight.manifest.json");

        let mut config = TranscribeConfig::default();
        config.out_manifest = out_manifest.clone();
        config.live_chunked = true;
        config.chunk_window_ms = 4_000;
        config.chunk_stride_ms = 1_000;
        config.chunk_queue_cap = 4;

        let report = PreflightReport {
            generated_at_utc: "2026-03-01T00:00:00Z".to_string(),
            checks: vec![PreflightCheck {
                id: "sample",
                status: CheckStatus::Pass,
                detail: "ok".to_string(),
                remediation: None,
            }],
        };

        write_preflight_manifest(&config, &report).unwrap();

        let manifest = fs::read_to_string(out_manifest).unwrap();
        let parsed: Value = serde_json::from_str(&manifest).unwrap();
        let config_obj = parsed.get("config").and_then(Value::as_object).unwrap();
        assert!(manifest.contains("\"input_wav\": \""));
        assert!(manifest.contains("tts_phrase.wav"));
        assert!(manifest.contains(
            "\"input_wav_semantics\": \"representative live scratch artifact mirrored from canonical out_wav after capture\""
        ));
        assert_eq!(
            parsed.get("runtime_mode").and_then(Value::as_str),
            Some("live-chunked")
        );
        assert_eq!(
            parsed.get("runtime_mode_taxonomy").and_then(Value::as_str),
            Some("representative-chunked")
        );
        assert_eq!(
            parsed.get("runtime_mode_selector").and_then(Value::as_str),
            Some("--live-chunked")
        );
        assert_eq!(
            parsed.get("runtime_mode_status").and_then(Value::as_str),
            Some("implemented")
        );
        assert_eq!(
            config_obj.get("runtime_mode").and_then(Value::as_str),
            Some("live-chunked")
        );
        assert_eq!(
            config_obj
                .get("runtime_mode_taxonomy")
                .and_then(Value::as_str),
            Some("representative-chunked")
        );
        assert_eq!(
            config_obj
                .get("runtime_mode_selector")
                .and_then(Value::as_str),
            Some("--live-chunked")
        );
        assert_eq!(
            config_obj
                .get("runtime_mode_status")
                .and_then(Value::as_str),
            Some("implemented")
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn runtime_artifacts_remain_compatible_across_runtime_mode_selectors() {
        let temp_dir = write_temp_dir("recordit-runtime-cross-mode-regression");
        let cases = [
            (
                "representative-offline",
                "representative-offline",
                "<default>",
                false,
                true,
                false,
                false,
                "representative offline fixture input path",
            ),
            (
                "live-chunked",
                "representative-chunked",
                "--live-chunked",
                false,
                true,
                true,
                false,
                "representative live scratch artifact mirrored from canonical out_wav after capture",
            ),
            (
                "live-stream",
                "live-stream",
                "--live-stream",
                true,
                false,
                false,
                true,
                "progressive live capture scratch artifact (materialized into canonical out_wav on success)",
            ),
        ];

        for (
            runtime_mode,
            runtime_taxonomy,
            runtime_selector,
            terminal_live_mode,
            stable_lines_replayed,
            live_chunked,
            live_stream,
            expected_input_wav_semantics,
        ) in cases
        {
            let mode_slug = runtime_mode.replace('-', "_");
            let input_wav = temp_dir.join(format!("{mode_slug}.input.wav"));
            let out_wav = temp_dir.join(format!("{mode_slug}.session.wav"));
            let out_jsonl = temp_dir.join(format!("{mode_slug}.session.jsonl"));
            let out_manifest = temp_dir.join(format!("{mode_slug}.session.manifest.json"));
            File::create(&input_wav).unwrap();
            File::create(&out_wav).unwrap();

            let mut config = TranscribeConfig::default();
            config.input_wav = input_wav;
            config.out_wav = out_wav;
            config.out_jsonl = out_jsonl.clone();
            config.out_manifest = out_manifest.clone();
            config.live_chunked = live_chunked;
            config.live_stream = live_stream;
            config.duration_sec = 10;
            let chunk_queue = if terminal_live_mode {
                LiveChunkQueueTelemetry::enabled(config.chunk_queue_cap)
            } else {
                LiveChunkQueueTelemetry::disabled(&config)
            };

            let report = LiveRunReport {
                generated_at_utc: "2026-03-01T00:00:00Z".to_string(),
                backend_id: "whispercpp",
                resolved_model_path: temp_dir.join(format!("{mode_slug}.model.bin")),
                resolved_model_source: "test".to_string(),
                channel_mode: ChannelMode::Separate,
                active_channel_mode: ChannelMode::Separate,
                transcript_text: format!("hello from {runtime_mode}"),
                channel_transcripts: vec![super::ChannelTranscriptSummary {
                    role: "mic",
                    label: "mic".to_string(),
                    text: format!("hello from {runtime_mode}"),
                }],
                vad_boundaries: vec![VadBoundary {
                    id: 0,
                    start_ms: 0,
                    end_ms: 100,
                    source: "energy_threshold",
                }],
                events: vec![final_event(
                    &format!("{mode_slug}-segment-0"),
                    "mic",
                    "hello world",
                )],
                degradation_events: Vec::new(),
                trust_notices: Vec::new(),
                lifecycle: sample_lifecycle(),
                reconciliation: ReconciliationMatrix::none(),
                asr_worker_pool: LiveAsrPoolTelemetry::default(),
                final_buffering: FinalBufferingTelemetry::default(),
                chunk_queue,
                cleanup_queue: CleanupQueueTelemetry::disabled(&config),
                benchmark: BenchmarkSummary {
                    run_count: 1,
                    wall_ms_p50: 1.0,
                    wall_ms_p95: 1.0,
                    partial_slo_met: true,
                    final_slo_met: true,
                },
                benchmark_summary_csv: temp_dir.join(format!("{mode_slug}.summary.csv")),
                benchmark_runs_csv: temp_dir.join(format!("{mode_slug}.runs.csv")),
            };

            write_runtime_jsonl(&config, &report).unwrap();
            write_runtime_manifest(&config, &report).unwrap();
            replay_timeline(&out_jsonl).unwrap();

            let manifest = fs::read_to_string(out_manifest).unwrap();
            assert!(manifest.contains(&format!("\"runtime_mode\": \"{runtime_mode}\"")));
            assert!(manifest.contains(&format!(
                "\"runtime_mode_taxonomy\": \"{runtime_taxonomy}\""
            )));
            assert!(manifest.contains(&format!(
                "\"runtime_mode_selector\": \"{runtime_selector}\""
            )));
            assert!(manifest.contains("\"runtime_mode_status\": \"implemented\""));
            assert!(manifest.contains(&format!("\"live_mode\":{terminal_live_mode}")));
            assert!(manifest.contains(&format!(
                "\"stable_lines_replayed\":{stable_lines_replayed}"
            )));
            assert!(manifest.contains(&format!(
                "\"input_wav_semantics\": \"{}\"",
                expected_input_wav_semantics
            )));
            assert!(manifest.contains("\"event_type\":\"final\""));
            assert!(manifest.contains("\"session_summary\": {"));
        }

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn runtime_jsonl_chunk_queue_event_includes_lag_metrics() {
        let temp_dir = write_temp_dir("recordit-runtime-jsonl-chunk-lag");
        let input_wav = temp_dir.join("input.wav");
        let out_wav = temp_dir.join("session.wav");
        let out_jsonl = temp_dir.join("session.jsonl");
        File::create(&input_wav).unwrap();
        File::create(&out_wav).unwrap();

        let mut config = TranscribeConfig::default();
        config.input_wav = input_wav.clone();
        config.out_wav = out_wav.clone();
        config.out_jsonl = out_jsonl.clone();

        let mut chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        chunk_queue.enabled = true;
        chunk_queue.max_queue = 2;
        chunk_queue.submitted = 4;
        chunk_queue.enqueued = 4;
        chunk_queue.processed = 4;
        chunk_queue.high_water = 2;
        chunk_queue.lag_sample_count = 4;
        chunk_queue.lag_p50_ms = 1_000;
        chunk_queue.lag_p95_ms = 2_000;
        chunk_queue.lag_max_ms = 2_000;

        let report = LiveRunReport {
            generated_at_utc: "2026-02-28T00:00:00Z".to_string(),
            backend_id: "whispercpp",
            resolved_model_path: temp_dir.join("model.bin"),
            resolved_model_source: "test".to_string(),
            channel_mode: ChannelMode::Separate,
            active_channel_mode: ChannelMode::Separate,
            transcript_text: "hello".to_string(),
            channel_transcripts: vec![super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "hello".to_string(),
            }],
            vad_boundaries: vec![VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 100,
                source: "energy_threshold",
            }],
            events: vec![final_event("mic-0", "mic", "hello world")],
            degradation_events: Vec::new(),
            trust_notices: Vec::new(),
            lifecycle: sample_lifecycle(),
            reconciliation: ReconciliationMatrix::none(),
            asr_worker_pool: LiveAsrPoolTelemetry {
                prewarm_ok: true,
                submitted: 4,
                enqueued: 3,
                dropped_queue_full: 1,
                processed: 3,
                succeeded: 2,
                failed: 2,
                retry_attempts: 1,
                temp_audio_deleted: 2,
                temp_audio_retained: 1,
            },
            final_buffering: FinalBufferingTelemetry::default(),
            chunk_queue,
            cleanup_queue: CleanupQueueTelemetry::disabled(&config),
            benchmark: BenchmarkSummary {
                run_count: 1,
                wall_ms_p50: 1.0,
                wall_ms_p95: 1.0,
                partial_slo_met: true,
                final_slo_met: true,
            },
            benchmark_summary_csv: temp_dir.join("summary.csv"),
            benchmark_runs_csv: temp_dir.join("runs.csv"),
        };

        write_runtime_jsonl(&config, &report).unwrap();
        let jsonl = fs::read_to_string(&out_jsonl).unwrap();
        let reconciliation_line = jsonl
            .lines()
            .find(|line| line.contains("\"event_type\":\"reconciliation_matrix\""))
            .unwrap();
        assert!(reconciliation_line.contains("\"channel\":\"control\""));
        let asr_pool_line = jsonl
            .lines()
            .find(|line| line.contains("\"event_type\":\"asr_worker_pool\""))
            .unwrap();
        assert!(asr_pool_line.contains("\"channel\":\"control\""));
        assert!(asr_pool_line.contains("\"prewarm_ok\":true"));
        assert!(asr_pool_line.contains("\"submitted\":4"));
        assert!(asr_pool_line.contains("\"temp_audio_deleted\":2"));
        assert!(asr_pool_line.contains("\"temp_audio_retained\":1"));
        let chunk_queue_line = jsonl
            .lines()
            .find(|line| line.contains("\"event_type\":\"chunk_queue\""))
            .unwrap();
        assert!(chunk_queue_line.contains("\"lag_sample_count\":4"));
        assert!(chunk_queue_line.contains("\"lag_p50_ms\":1000"));
        assert!(chunk_queue_line.contains("\"lag_p95_ms\":2000"));
        assert!(chunk_queue_line.contains("\"lag_max_ms\":2000"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn runtime_jsonl_emits_lifecycle_phase_events_in_order() {
        let temp_dir = write_temp_dir("recordit-runtime-jsonl-lifecycle");
        let input_wav = temp_dir.join("input.wav");
        let out_wav = temp_dir.join("session.wav");
        let out_jsonl = temp_dir.join("session.jsonl");
        File::create(&input_wav).unwrap();
        File::create(&out_wav).unwrap();

        let mut config = TranscribeConfig::default();
        config.input_wav = input_wav.clone();
        config.out_wav = out_wav.clone();
        config.out_jsonl = out_jsonl.clone();

        let report = LiveRunReport {
            generated_at_utc: "2026-02-28T00:00:00Z".to_string(),
            backend_id: "whispercpp",
            resolved_model_path: temp_dir.join("model.bin"),
            resolved_model_source: "test".to_string(),
            channel_mode: ChannelMode::Separate,
            active_channel_mode: ChannelMode::Separate,
            transcript_text: "hello".to_string(),
            channel_transcripts: vec![super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "hello".to_string(),
            }],
            vad_boundaries: vec![VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 100,
                source: "energy_threshold",
            }],
            events: vec![final_event("mic-0", "mic", "hello world")],
            degradation_events: Vec::new(),
            trust_notices: Vec::new(),
            lifecycle: sample_lifecycle(),
            reconciliation: ReconciliationMatrix::none(),
            asr_worker_pool: LiveAsrPoolTelemetry::default(),
            final_buffering: FinalBufferingTelemetry::default(),
            chunk_queue: LiveChunkQueueTelemetry::disabled(&config),
            cleanup_queue: CleanupQueueTelemetry::disabled(&config),
            benchmark: BenchmarkSummary {
                run_count: 1,
                wall_ms_p50: 1.0,
                wall_ms_p95: 1.0,
                partial_slo_met: true,
                final_slo_met: true,
            },
            benchmark_summary_csv: temp_dir.join("summary.csv"),
            benchmark_runs_csv: temp_dir.join("runs.csv"),
        };

        write_runtime_jsonl(&config, &report).unwrap();
        let lifecycle_lines = fs::read_to_string(&out_jsonl)
            .unwrap()
            .lines()
            .filter(|line| line.contains("\"event_type\":\"lifecycle_phase\""))
            .map(str::to_string)
            .collect::<Vec<_>>();

        assert_eq!(lifecycle_lines.len(), 4);
        assert!(lifecycle_lines[0].contains("\"phase\":\"warmup\""));
        assert!(lifecycle_lines[0].contains("\"ready_for_transcripts\":false"));
        assert!(lifecycle_lines[1].contains("\"phase\":\"active\""));
        assert!(lifecycle_lines[1].contains("\"ready_for_transcripts\":true"));
        assert!(lifecycle_lines[2].contains("\"phase\":\"draining\""));
        assert!(lifecycle_lines[3].contains("\"phase\":\"shutdown\""));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn runtime_jsonl_places_active_phase_before_first_transcript_event() {
        let temp_dir = write_temp_dir("recordit-runtime-jsonl-active-before-transcript");
        let input_wav = temp_dir.join("input.wav");
        let out_wav = temp_dir.join("session.wav");
        let out_jsonl = temp_dir.join("session.jsonl");
        File::create(&input_wav).unwrap();
        File::create(&out_wav).unwrap();

        let mut config = TranscribeConfig::default();
        config.input_wav = input_wav.clone();
        config.out_wav = out_wav.clone();
        config.out_jsonl = out_jsonl.clone();

        let report = LiveRunReport {
            generated_at_utc: "2026-02-28T00:00:00Z".to_string(),
            backend_id: "whispercpp",
            resolved_model_path: temp_dir.join("model.bin"),
            resolved_model_source: "test".to_string(),
            channel_mode: ChannelMode::Separate,
            active_channel_mode: ChannelMode::Separate,
            transcript_text: "hello".to_string(),
            channel_transcripts: vec![super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "hello".to_string(),
            }],
            vad_boundaries: vec![VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 100,
                source: "energy_threshold",
            }],
            events: vec![final_event("mic-0", "mic", "hello world")],
            degradation_events: Vec::new(),
            trust_notices: Vec::new(),
            lifecycle: sample_lifecycle(),
            reconciliation: ReconciliationMatrix::none(),
            asr_worker_pool: LiveAsrPoolTelemetry::default(),
            final_buffering: FinalBufferingTelemetry::default(),
            chunk_queue: LiveChunkQueueTelemetry::disabled(&config),
            cleanup_queue: CleanupQueueTelemetry::disabled(&config),
            benchmark: BenchmarkSummary {
                run_count: 1,
                wall_ms_p50: 1.0,
                wall_ms_p95: 1.0,
                partial_slo_met: true,
                final_slo_met: true,
            },
            benchmark_summary_csv: temp_dir.join("summary.csv"),
            benchmark_runs_csv: temp_dir.join("runs.csv"),
        };

        write_runtime_jsonl(&config, &report).unwrap();
        let lines = fs::read_to_string(&out_jsonl)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let active_idx = lines
            .iter()
            .position(|line| {
                line.contains("\"event_type\":\"lifecycle_phase\"")
                    && line.contains("\"phase\":\"active\"")
            })
            .expect("active lifecycle line missing");
        let first_transcript_idx = lines
            .iter()
            .position(|line| {
                line.contains("\"event_type\":\"partial\"")
                    || line.contains("\"event_type\":\"final\"")
                    || line.contains("\"event_type\":\"reconciled_final\"")
            })
            .expect("transcript line missing");
        assert!(
            active_idx < first_transcript_idx,
            "expected active lifecycle before first transcript event (active_idx={active_idx}, first_transcript_idx={first_transcript_idx})"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn incremental_runtime_sink_emits_stable_transcript_during_active_phase() {
        let temp_dir = write_temp_dir("recordit-live-stream-runtime-jsonl-growth");
        let out_jsonl = temp_dir.join("runtime.jsonl");
        let mut config = TranscribeConfig::default();
        config.out_jsonl = out_jsonl.clone();

        let mut sink = CollectingRuntimeOutputSink::with_incremental_jsonl(
            super::LiveStreamIncrementalJsonlWriter::open(&config).unwrap(),
        );
        RuntimeOutputSink::emit(
            &mut sink,
            RuntimeOutputEvent::Lifecycle {
                emit_seq: 1,
                phase: LiveRuntimePhase::Warmup,
                detail: "warmup".to_string(),
            },
        )
        .unwrap();
        let warmup_size = fs::metadata(&out_jsonl).unwrap().len();
        RuntimeOutputSink::emit(
            &mut sink,
            RuntimeOutputEvent::Lifecycle {
                emit_seq: 2,
                phase: LiveRuntimePhase::Active,
                detail: "active".to_string(),
            },
        )
        .unwrap();
        RuntimeOutputSink::emit(
            &mut sink,
            RuntimeOutputEvent::AsrCompleted {
                emit_seq: 3,
                result: RuntimeAsrResult {
                    job: RuntimeAsrJobSpec {
                        emit_seq: 3,
                        job_class: RuntimeAsrJobClass::Final,
                        channel: "microphone".to_string(),
                        segment_id: "microphone-seg-0001".to_string(),
                        segment_ord: 1,
                        window_ord: 1,
                        start_ms: 200,
                        end_ms: 900,
                    },
                    transcript_text: "stable text".to_string(),
                },
            },
        )
        .unwrap();
        let stable_size = fs::metadata(&out_jsonl).unwrap().len();
        RuntimeOutputSink::emit(
            &mut sink,
            RuntimeOutputEvent::Lifecycle {
                emit_seq: 4,
                phase: LiveRuntimePhase::Draining,
                detail: "draining".to_string(),
            },
        )
        .unwrap();
        sink.finalize_incremental_jsonl().unwrap();

        assert!(
            stable_size > warmup_size,
            "expected JSONL to grow after stable transcript emission"
        );
        let lines = fs::read_to_string(&out_jsonl)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let active_idx = lines
            .iter()
            .position(|line| {
                line.contains("\"event_type\":\"lifecycle_phase\"")
                    && line.contains("\"phase\":\"active\"")
            })
            .unwrap();
        let first_stable_idx = lines
            .iter()
            .position(|line| {
                line.contains("\"event_type\":\"final\"")
                    || line.contains("\"event_type\":\"llm_final\"")
                    || line.contains("\"event_type\":\"reconciled_final\"")
            })
            .unwrap();
        let draining_idx = lines
            .iter()
            .position(|line| {
                line.contains("\"event_type\":\"lifecycle_phase\"")
                    && line.contains("\"phase\":\"draining\"")
            })
            .unwrap();
        assert!(
            active_idx < first_stable_idx && first_stable_idx < draining_idx,
            "stable transcript emit should occur during active phase"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn runtime_jsonl_stream_persists_checkpointed_lines_before_finalize() {
        let temp_dir = write_temp_dir("recordit-runtime-jsonl-incremental");
        let out_jsonl = temp_dir.join("session.jsonl");
        let mut stream = RuntimeJsonlStream::open(&out_jsonl).unwrap();

        stream
            .write_line("{\"event_type\":\"lifecycle_phase\",\"phase\":\"warmup\"}")
            .unwrap();
        stream.checkpoint().unwrap();
        let first_snapshot = fs::read_to_string(&out_jsonl).unwrap();
        assert!(first_snapshot.contains("\"phase\":\"warmup\""));

        stream
            .write_line("{\"event_type\":\"vad_boundary\",\"boundary_id\":0}")
            .unwrap();
        stream.checkpoint().unwrap();
        let second_snapshot = fs::read_to_string(&out_jsonl).unwrap();
        assert!(second_snapshot.contains("\"phase\":\"warmup\""));
        assert!(second_snapshot.contains("\"event_type\":\"vad_boundary\""));

        stream.finalize().unwrap();
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn emit_latest_lifecycle_transition_jsonl_writes_only_latest_and_noops_when_empty() {
        let temp_dir = write_temp_dir("recordit-runtime-jsonl-lifecycle-latest");
        let out_jsonl = temp_dir.join("session.jsonl");
        let mut stream = RuntimeJsonlStream::open(&out_jsonl).unwrap();
        let mut lifecycle = LiveLifecycleTelemetry::new();

        emit_latest_lifecycle_transition_jsonl(&mut stream, &lifecycle).unwrap();
        assert_eq!(fs::read_to_string(&out_jsonl).unwrap(), "");

        lifecycle.transition(LiveLifecyclePhase::Active, "capture loop active");
        emit_latest_lifecycle_transition_jsonl(&mut stream, &lifecycle).unwrap();
        lifecycle.transition(
            LiveLifecyclePhase::Draining,
            "draining queue and reconciliation",
        );
        emit_latest_lifecycle_transition_jsonl(&mut stream, &lifecycle).unwrap();
        stream.finalize().unwrap();

        let lifecycle_lines = fs::read_to_string(&out_jsonl)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(lifecycle_lines.len(), 2);
        assert!(lifecycle_lines[0].contains("\"phase\":\"active\""));
        assert!(lifecycle_lines[0].contains("\"transition_index\":0"));
        assert!(lifecycle_lines[1].contains("\"phase\":\"draining\""));
        assert!(lifecycle_lines[1].contains("\"transition_index\":1"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn near_live_artifacts_remain_schema_compatible_for_replay() {
        let temp_dir = write_temp_dir("recordit-near-live-schema-replay");
        let input_wav = temp_dir.join("input.wav");
        let out_wav = temp_dir.join("session.wav");
        let out_jsonl = temp_dir.join("session.jsonl");
        let out_manifest = temp_dir.join("session.manifest.json");
        File::create(&input_wav).unwrap();
        File::create(&out_wav).unwrap();

        let mut config = TranscribeConfig::default();
        config.input_wav = input_wav.clone();
        config.out_wav = out_wav.clone();
        config.out_jsonl = out_jsonl.clone();
        config.out_manifest = out_manifest.clone();

        let mut chunk_queue = LiveChunkQueueTelemetry::disabled(&config);
        chunk_queue.enabled = true;
        chunk_queue.max_queue = 2;
        chunk_queue.submitted = 3;
        chunk_queue.enqueued = 3;
        chunk_queue.processed = 3;
        chunk_queue.high_water = 2;
        chunk_queue.lag_sample_count = 3;
        chunk_queue.lag_p50_ms = 1_000;
        chunk_queue.lag_p95_ms = 2_000;
        chunk_queue.lag_max_ms = 2_000;

        let report = LiveRunReport {
            generated_at_utc: "2026-02-28T00:00:00Z".to_string(),
            backend_id: "whispercpp",
            resolved_model_path: temp_dir.join("model.bin"),
            resolved_model_source: "test".to_string(),
            channel_mode: ChannelMode::Separate,
            active_channel_mode: ChannelMode::Separate,
            transcript_text: "hello world".to_string(),
            channel_transcripts: vec![super::ChannelTranscriptSummary {
                role: "mic",
                label: "mic".to_string(),
                text: "hello world".to_string(),
            }],
            vad_boundaries: vec![VadBoundary {
                id: 0,
                start_ms: 0,
                end_ms: 2_000,
                source: "energy_threshold",
            }],
            events: vec![
                TranscriptEvent {
                    event_type: "partial",
                    channel: "mic".to_string(),
                    segment_id: "mic-chunk-0000-0-4000".to_string(),
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "hello".to_string(),
                    source_final_segment_id: None,
                },
                TranscriptEvent {
                    event_type: "final",
                    channel: "mic".to_string(),
                    segment_id: "mic-chunk-0000-0-4000".to_string(),
                    start_ms: 0,
                    end_ms: 2_000,
                    text: "hello world".to_string(),
                    source_final_segment_id: None,
                },
                TranscriptEvent {
                    event_type: "reconciled_final",
                    channel: "mic".to_string(),
                    segment_id: "mic-reconciled-0000".to_string(),
                    start_ms: 0,
                    end_ms: 2_000,
                    text: "hello world reconciled".to_string(),
                    source_final_segment_id: Some("mic-chunk-0000-0-4000".to_string()),
                },
            ],
            degradation_events: Vec::new(),
            trust_notices: Vec::new(),
            lifecycle: sample_lifecycle(),
            reconciliation: ReconciliationMatrix::none(),
            asr_worker_pool: LiveAsrPoolTelemetry::default(),
            final_buffering: FinalBufferingTelemetry::default(),
            chunk_queue,
            cleanup_queue: CleanupQueueTelemetry::disabled(&config),
            benchmark: BenchmarkSummary {
                run_count: 1,
                wall_ms_p50: 1.0,
                wall_ms_p95: 1.0,
                partial_slo_met: true,
                final_slo_met: true,
            },
            benchmark_summary_csv: temp_dir.join("summary.csv"),
            benchmark_runs_csv: temp_dir.join("runs.csv"),
        };

        write_runtime_jsonl(&config, &report).unwrap();
        write_runtime_manifest(&config, &report).unwrap();

        let jsonl = fs::read_to_string(&out_jsonl).unwrap();
        assert!(jsonl.lines().any(|line| {
            line.contains("\"event_type\":\"partial\"")
                && line.contains("\"segment_id\":\"mic-chunk-0000-0-4000\"")
                && line.contains("\"start_ms\":0")
                && line.contains("\"end_ms\":1000")
                && line.contains("\"text\":\"hello\"")
        }));
        assert!(jsonl.lines().any(|line| {
            line.contains("\"event_type\":\"reconciled_final\"")
                && line.contains("\"segment_id\":\"mic-reconciled-0000\"")
                && line.contains("\"source_final_segment_id\":\"mic-chunk-0000-0-4000\"")
        }));
        assert!(
            jsonl
                .lines()
                .any(|line| line.contains("\"event_type\":\"chunk_queue\""))
        );

        let manifest = fs::read_to_string(&out_manifest).unwrap();
        assert!(manifest.contains("\"events\": ["));
        assert!(manifest.contains("\"event_type\":\"reconciled_final\""));
        assert!(manifest.contains("\"source_final_segment_id\":\"mic-chunk-0000-0-4000\""));
        assert!(manifest.contains("\"chunk_queue\": {"));
        assert!(manifest.contains("\"jsonl_path\":"));

        replay_timeline(&out_jsonl).unwrap();

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn model_resolution_fails_fast_for_missing_explicit_cli_model() {
        let _guard = env_lock().lock().unwrap();
        let original_cwd = env::current_dir().unwrap();
        let original_env = env::var("RECORDIT_ASR_MODEL").ok();
        let temp_dir = write_temp_dir("recordit-model-explicit-missing");
        let default_model = temp_dir.join("artifacts/bench/models/whispercpp/ggml-tiny.en.bin");
        fs::create_dir_all(default_model.parent().unwrap()).unwrap();
        File::create(&default_model).unwrap();

        env::set_current_dir(&temp_dir).unwrap();
        unsafe {
            env::remove_var("RECORDIT_ASR_MODEL");
        }

        let mut config = TranscribeConfig::default();
        config.asr_model = temp_dir.join("does-not-exist.bin");
        let err = resolve_model_path(&config).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("explicit `--asr-model` path does not exist"));
        assert!(message.contains("omit `--asr-model` to allow default resolution"));

        restore_env_state(original_cwd, original_env);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn bundled_backend_program_resolution_prefers_resources_bin() {
        let temp_dir = write_temp_dir("recordit-bundled-backend-program");
        let exe_path = temp_dir
            .join("SequoiaTranscribe.app")
            .join("Contents")
            .join("MacOS")
            .join("SequoiaTranscribe");
        fs::create_dir_all(exe_path.parent().unwrap()).unwrap();
        File::create(&exe_path).unwrap();

        let resources_bin = temp_dir
            .join("SequoiaTranscribe.app")
            .join("Contents")
            .join("Resources")
            .join("bin");
        fs::create_dir_all(&resources_bin).unwrap();
        let resources_helper = resources_bin.join("whisper-cli");
        File::create(&resources_helper).unwrap();

        let helpers_dir = temp_dir
            .join("SequoiaTranscribe.app")
            .join("Contents")
            .join("Helpers");
        fs::create_dir_all(&helpers_dir).unwrap();
        File::create(helpers_dir.join("whisper-cli")).unwrap();

        let resolved = bundled_backend_program_from_exe(AsrBackend::WhisperCpp, &exe_path).unwrap();
        assert_eq!(resolved, resources_helper.to_string_lossy().to_string());

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn resolve_backend_program_prefers_env_override() {
        let _guard = env_lock().lock().unwrap();
        let original_env = env::var("RECORDIT_WHISPERCPP_CLI_PATH").ok();
        unsafe {
            env::set_var("RECORDIT_WHISPERCPP_CLI_PATH", "/tmp/custom-whisper-cli");
        }

        let temp_dir = write_temp_dir("recordit-backend-program-env");
        let model_path = temp_dir.join("ggml.bin");
        File::create(&model_path).unwrap();

        let resolved = resolve_backend_program(AsrBackend::WhisperCpp, &model_path);
        assert_eq!(resolved, "/tmp/custom-whisper-cli");

        restore_optional_env("RECORDIT_WHISPERCPP_CLI_PATH", original_env);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn resolve_backend_program_uses_model_sibling_helper() {
        let _guard = env_lock().lock().unwrap();
        let original_env = env::var("RECORDIT_WHISPERCPP_CLI_PATH").ok();
        unsafe {
            env::remove_var("RECORDIT_WHISPERCPP_CLI_PATH");
        }

        let temp_dir = write_temp_dir("recordit-backend-program-model-sibling");
        let model_dir = temp_dir.join("models");
        fs::create_dir_all(&model_dir).unwrap();
        let model_path = model_dir.join("ggml.bin");
        File::create(&model_path).unwrap();
        let helper_path = model_dir.join("whisper-cli");
        File::create(&helper_path).unwrap();

        let resolved = resolve_backend_program(AsrBackend::WhisperCpp, &model_path);
        assert_eq!(resolved, helper_path.to_string_lossy().to_string());

        restore_optional_env("RECORDIT_WHISPERCPP_CLI_PATH", original_env);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn resolve_backend_program_uses_model_bin_helper_fallback() {
        let _guard = env_lock().lock().unwrap();
        let original_env = env::var("RECORDIT_WHISPERCPP_CLI_PATH").ok();
        unsafe {
            env::remove_var("RECORDIT_WHISPERCPP_CLI_PATH");
        }

        let temp_dir = write_temp_dir("recordit-backend-program-model-bin");
        let model_dir = temp_dir.join("models");
        let model_bin_dir = model_dir.join("bin");
        fs::create_dir_all(&model_bin_dir).unwrap();
        let model_path = model_dir.join("ggml.bin");
        File::create(&model_path).unwrap();
        let helper_path = model_bin_dir.join("whisper-cli");
        File::create(&helper_path).unwrap();

        let resolved = resolve_backend_program(AsrBackend::WhisperCpp, &model_path);
        assert_eq!(resolved, helper_path.to_string_lossy().to_string());

        restore_optional_env("RECORDIT_WHISPERCPP_CLI_PATH", original_env);
        let _ = fs::remove_dir_all(temp_dir);
    }

    fn final_event(segment_id: &str, channel: &str, text: &str) -> TranscriptEvent {
        TranscriptEvent {
            event_type: "final",
            channel: channel.to_string(),
            segment_id: segment_id.to_string(),
            start_ms: 0,
            end_ms: 100,
            text: text.to_string(),
            source_final_segment_id: None,
        }
    }

    fn sample_lifecycle() -> LiveLifecycleTelemetry {
        let mut lifecycle = LiveLifecycleTelemetry::new();
        lifecycle.transition(LiveLifecyclePhase::Warmup, "test warmup");
        lifecycle.transition(LiveLifecyclePhase::Active, "test active");
        lifecycle.transition(LiveLifecyclePhase::Draining, "test draining");
        lifecycle.transition(LiveLifecyclePhase::Shutdown, "test shutdown");
        lifecycle
    }

    fn write_test_mono_wav() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("recordit-mono-{stamp}.wav"));
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for _ in 0..320 {
            writer.write_sample::<i16>(0).unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    fn write_test_stereo_wav(path: &Path, sample_rate_hz: u32, frame_count: usize) {
        let spec = WavSpec {
            channels: 2,
            sample_rate: sample_rate_hz,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(path, spec).unwrap();
        for idx in 0..frame_count {
            let sample = ((idx % 48) as f32 / 24.0) - 1.0;
            writer.write_sample(sample).unwrap(); // microphone
            writer.write_sample(-sample).unwrap(); // system audio
        }
        writer.finalize().unwrap();
    }

    fn write_temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("{prefix}-{stamp}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn restore_env_state(original_cwd: PathBuf, original_env: Option<String>) {
        env::set_current_dir(original_cwd).unwrap();
        restore_optional_env("RECORDIT_ASR_MODEL", original_env);
    }

    fn restore_optional_env(name: &str, original_value: Option<String>) {
        match original_value {
            Some(value) => unsafe {
                env::set_var(name, value);
            },
            None => unsafe {
                env::remove_var(name);
            },
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
}
