use screencapturekit::prelude::*;
use std::env;
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant};

const HELP_TEXT: &str = "\
transcribe-live

Define and validate the live transcription CLI contract for the next phase of recordit.

Usage:
  transcribe-live [options]

Options:
  --duration-sec <seconds>        Capture duration in seconds (default: 10)
  --out-wav <path>                Output WAV artifact path (default: artifacts/transcribe-live.wav)
  --out-jsonl <path>              Output JSONL transcript path (default: artifacts/transcribe-live.jsonl)
  --out-manifest <path>           Output session manifest path (default: artifacts/transcribe-live.manifest.json)
  --sample-rate <hz>              Capture sample rate in Hz (default: 48000)
  --asr-backend <backend>         ASR backend: whisper-rs | moonshine (default: whisper-rs)
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
  --transcribe-channels <mode>    Channel mode: separate | mixed (default: separate)
  --speaker-labels <mic,system>   Comma-separated labels for the two channels (default: mic,system)
  --preflight                     Run structured preflight diagnostics and write manifest
  -h, --help                      Show this help text

Examples:
  transcribe-live --asr-model models/moonshine/base.onnx
  transcribe-live --asr-backend moonshine --asr-model models/moonshine/base.onnx --transcribe-channels mixed
  transcribe-live --asr-model models/ggml-base.en.bin --llm-cleanup --llm-endpoint http://127.0.0.1:8080/v1/chat/completions --llm-model llama3.2:3b
  transcribe-live --preflight --asr-model models/ggml-base.en.bin
";

#[derive(Debug, Clone, Copy)]
enum AsrBackend {
    WhisperRs,
    Moonshine,
}

impl Display for AsrBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WhisperRs => f.write_str("whisper-rs"),
            Self::Moonshine => f.write_str("moonshine"),
        }
    }
}

impl AsrBackend {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "whisper-rs" => Ok(Self::WhisperRs),
            "moonshine" => Ok(Self::Moonshine),
            _ => Err(CliError::new(format!(
                "unsupported --asr-backend `{value}`; expected `whisper-rs` or `moonshine`"
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

#[derive(Debug, Clone, Copy)]
enum ChannelMode {
    Separate,
    Mixed,
}

impl Display for ChannelMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Separate => f.write_str("separate"),
            Self::Mixed => f.write_str("mixed"),
        }
    }
}

impl ChannelMode {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "separate" => Ok(Self::Separate),
            "mixed" => Ok(Self::Mixed),
            _ => Err(CliError::new(format!(
                "unsupported --transcribe-channels `{value}`; expected `separate` or `mixed`"
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

#[derive(Debug, Clone)]
struct TranscribeConfig {
    duration_sec: u64,
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
    channel_mode: ChannelMode,
    speaker_labels: SpeakerLabels,
    preflight: bool,
}

impl Default for TranscribeConfig {
    fn default() -> Self {
        Self {
            duration_sec: 10,
            out_wav: PathBuf::from("artifacts/transcribe-live.wav"),
            out_jsonl: PathBuf::from("artifacts/transcribe-live.jsonl"),
            out_manifest: PathBuf::from("artifacts/transcribe-live.manifest.json"),
            sample_rate_hz: 48_000,
            asr_backend: AsrBackend::WhisperRs,
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
            channel_mode: ChannelMode::Separate,
            speaker_labels: SpeakerLabels {
                mic: "mic".to_owned(),
                system: "system".to_owned(),
            },
            preflight: false,
        }
    }
}

impl TranscribeConfig {
    fn validate(&self) -> Result<(), CliError> {
        if self.duration_sec == 0 {
            return Err(CliError::new("`--duration-sec` must be greater than zero"));
        }

        if self.sample_rate_hz == 0 {
            return Err(CliError::new("`--sample-rate` must be greater than zero"));
        }

        if !self.preflight && self.asr_model.as_os_str().is_empty() {
            return Err(CliError::new(
                "`--asr-model <path>` is required so the CLI contract stays explicit about local model assets",
            ));
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

        validate_output_path("--out-wav", &self.out_wav)?;
        validate_output_path("--out-jsonl", &self.out_jsonl)?;
        validate_output_path("--out-manifest", &self.out_manifest)?;

        Ok(())
    }

    fn print_summary(&self) {
        println!("Transcribe-live configuration");
        println!("  status: contract validated");
        println!(
            "  runtime: capture/ASR execution not wired yet; this command currently locks the CLI surface for follow-up work"
        );
        println!("  duration_sec: {}", self.duration_sec);
        println!("  sample_rate_hz: {}", self.sample_rate_hz);
        println!("  out_wav: {}", display_path(&self.out_wav));
        println!("  out_jsonl: {}", display_path(&self.out_jsonl));
        println!("  out_manifest: {}", display_path(&self.out_manifest));
        println!("  asr_backend: {}", self.asr_backend);
        println!("  asr_model: {}", display_path(&self.asr_model));
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
        println!("  transcribe_channels: {}", self.channel_mode);
        println!("  speaker_labels: {}", self.speaker_labels);
        println!("  preflight: {}", self.preflight);
        println!();
        println!("Next implementation tasks:");
        println!("  - `bd-1kp` wires VAD + ASR backend execution");
        println!("  - `bd-w4c` adds partial/final JSONL events and replay");
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

fn main() -> ExitCode {
    match parse_args() {
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
                            return ExitCode::from(2);
                        }
                        match report.overall_status() {
                            CheckStatus::Fail => ExitCode::from(2),
                            _ => ExitCode::SUCCESS,
                        }
                    }
                    Err(err) => {
                        eprintln!("error: preflight failed unexpectedly: {err}");
                        ExitCode::from(2)
                    }
                }
            } else {
                config.print_summary();
                ExitCode::SUCCESS
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!();
            eprintln!("Run `transcribe-live --help` to see the supported contract.");
            ExitCode::from(2)
        }
    }
}

fn parse_args() -> Result<ParseOutcome, CliError> {
    let mut config = TranscribeConfig::default();
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(ParseOutcome::Help),
            "--duration-sec" => {
                config.duration_sec =
                    parse_u64(&read_value(&mut args, "--duration-sec")?, "--duration-sec")?;
            }
            "--out-wav" => {
                config.out_wav = PathBuf::from(read_value(&mut args, "--out-wav")?);
            }
            "--out-jsonl" => {
                config.out_jsonl = PathBuf::from(read_value(&mut args, "--out-jsonl")?);
            }
            "--out-manifest" => {
                config.out_manifest = PathBuf::from(read_value(&mut args, "--out-manifest")?);
            }
            "--sample-rate" => {
                config.sample_rate_hz =
                    parse_u32(&read_value(&mut args, "--sample-rate")?, "--sample-rate")?;
            }
            "--asr-backend" => {
                config.asr_backend = AsrBackend::parse(&read_value(&mut args, "--asr-backend")?)?;
            }
            "--asr-model" => {
                config.asr_model = PathBuf::from(read_value(&mut args, "--asr-model")?);
            }
            "--asr-language" => {
                config.asr_language = read_value(&mut args, "--asr-language")?;
            }
            "--asr-threads" => {
                config.asr_threads =
                    parse_usize(&read_value(&mut args, "--asr-threads")?, "--asr-threads")?;
            }
            "--asr-profile" => {
                config.asr_profile = AsrProfile::parse(&read_value(&mut args, "--asr-profile")?)?;
            }
            "--vad-backend" => {
                config.vad_backend = VadBackend::parse(&read_value(&mut args, "--vad-backend")?)?;
            }
            "--vad-threshold" => {
                config.vad_threshold = parse_f32(
                    &read_value(&mut args, "--vad-threshold")?,
                    "--vad-threshold",
                )?;
            }
            "--vad-min-speech-ms" => {
                config.vad_min_speech_ms = parse_u32(
                    &read_value(&mut args, "--vad-min-speech-ms")?,
                    "--vad-min-speech-ms",
                )?;
            }
            "--vad-min-silence-ms" => {
                config.vad_min_silence_ms = parse_u32(
                    &read_value(&mut args, "--vad-min-silence-ms")?,
                    "--vad-min-silence-ms",
                )?;
            }
            "--llm-cleanup" => {
                config.llm_cleanup = true;
            }
            "--llm-endpoint" => {
                config.llm_endpoint = Some(read_value(&mut args, "--llm-endpoint")?);
            }
            "--llm-model" => {
                config.llm_model = Some(read_value(&mut args, "--llm-model")?);
            }
            "--llm-timeout-ms" => {
                config.llm_timeout_ms = parse_u64(
                    &read_value(&mut args, "--llm-timeout-ms")?,
                    "--llm-timeout-ms",
                )?;
            }
            "--llm-max-queue" => {
                config.llm_max_queue = parse_usize(
                    &read_value(&mut args, "--llm-max-queue")?,
                    "--llm-max-queue",
                )?;
            }
            "--transcribe-channels" => {
                config.channel_mode =
                    ChannelMode::parse(&read_value(&mut args, "--transcribe-channels")?)?;
            }
            "--speaker-labels" => {
                config.speaker_labels =
                    SpeakerLabels::parse(&read_value(&mut args, "--speaker-labels")?)?;
            }
            "--preflight" => {
                config.preflight = true;
            }
            _ if arg.starts_with('-') => {
                return Err(CliError::new(format!("unknown option `{arg}`")));
            }
            _ => {
                return Err(CliError::new(format!(
                    "unexpected positional argument `{arg}`; use named flags only"
                )));
            }
        }
    }

    config.validate()?;
    Ok(ParseOutcome::Config(config))
}

fn read_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, CliError> {
    args.next()
        .ok_or_else(|| CliError::new(format!("`{flag}` requires a value")))
}

fn parse_u64(value: &str, flag: &str) -> Result<u64, CliError> {
    value
        .parse::<u64>()
        .map_err(|_| CliError::new(format!("`{flag}` expects an integer, got `{value}`")))
}

fn parse_u32(value: &str, flag: &str) -> Result<u32, CliError> {
    value
        .parse::<u32>()
        .map_err(|_| CliError::new(format!("`{flag}` expects an integer, got `{value}`")))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, CliError> {
    value
        .parse::<usize>()
        .map_err(|_| CliError::new(format!("`{flag}` expects an integer, got `{value}`")))
}

fn parse_f32(value: &str, flag: &str) -> Result<f32, CliError> {
    value.parse::<f32>().map_err(|_| {
        CliError::new(format!(
            "`{flag}` expects a floating-point value, got `{value}`"
        ))
    })
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
    if config.asr_model.as_os_str().is_empty() {
        return PreflightCheck::fail(
            "model_path",
            "no --asr-model path provided",
            "Provide --asr-model /absolute/path/to/model file.",
        );
    }

    if !config.asr_model.exists() {
        return PreflightCheck::fail(
            "model_path",
            format!(
                "model path does not exist: {}",
                display_path(&config.asr_model)
            ),
            "Download or copy the model locally, then pass --asr-model with that path.",
        );
    }

    if !config.asr_model.is_file() {
        return PreflightCheck::fail(
            "model_path",
            format!(
                "model path is not a file: {}",
                display_path(&config.asr_model)
            ),
            "Pass a file path for --asr-model, not a directory.",
        );
    }

    PreflightCheck::pass(
        "model_path",
        format!("model file found: {}", display_path(&config.asr_model)),
    )
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
        AsrBackend::WhisperRs => "whisper-cli",
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

fn write_preflight_manifest(
    config: &TranscribeConfig,
    report: &PreflightReport,
) -> Result<(), CliError> {
    if let Some(parent) = config.out_manifest.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CliError::new(format!(
                "failed to create manifest directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let mut file = File::create(&config.out_manifest).map_err(|err| {
        CliError::new(format!(
            "failed to create manifest {}: {err}",
            display_path(&config.out_manifest)
        ))
    })?;

    writeln!(file, "{{").map_err(io_to_cli)?;
    writeln!(file, "  \"schema_version\": \"1\",").map_err(io_to_cli)?;
    writeln!(file, "  \"kind\": \"transcribe-live-preflight\",").map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"generated_at_utc\": \"{}\",",
        json_escape(&report.generated_at_utc)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"overall_status\": \"{}\",",
        report.overall_status()
    )
    .map_err(io_to_cli)?;
    writeln!(file, "  \"config\": {{").map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"out_wav\": \"{}\",",
        json_escape(&display_path(&config.out_wav))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"out_jsonl\": \"{}\",",
        json_escape(&display_path(&config.out_jsonl))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"out_manifest\": \"{}\",",
        json_escape(&display_path(&config.out_manifest))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"asr_backend\": \"{}\",",
        json_escape(&config.asr_backend.to_string())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"asr_model\": \"{}\",",
        json_escape(&display_path(&config.asr_model))
    )
    .map_err(io_to_cli)?;
    writeln!(file, "    \"sample_rate_hz\": {}", config.sample_rate_hz).map_err(io_to_cli)?;
    writeln!(file, "  }},").map_err(io_to_cli)?;
    writeln!(file, "  \"checks\": [").map_err(io_to_cli)?;

    for (idx, check) in report.checks.iter().enumerate() {
        writeln!(file, "    {{").map_err(io_to_cli)?;
        writeln!(file, "      \"id\": \"{}\",", json_escape(check.id)).map_err(io_to_cli)?;
        writeln!(
            file,
            "      \"status\": \"{}\",",
            json_escape(&check.status.to_string())
        )
        .map_err(io_to_cli)?;
        writeln!(
            file,
            "      \"detail\": \"{}\",",
            json_escape(&check.detail)
        )
        .map_err(io_to_cli)?;
        writeln!(
            file,
            "      \"remediation\": \"{}\"",
            json_escape(check.remediation.as_deref().unwrap_or(""))
        )
        .map_err(io_to_cli)?;
        if idx + 1 == report.checks.len() {
            writeln!(file, "    }}").map_err(io_to_cli)?;
        } else {
            writeln!(file, "    }},").map_err(io_to_cli)?;
        }
    }

    writeln!(file, "  ]").map_err(io_to_cli)?;
    writeln!(file, "}}").map_err(io_to_cli)?;
    Ok(())
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
