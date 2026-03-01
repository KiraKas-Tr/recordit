use crate::rt_transport::TransportStatsSnapshot;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureStream {
    Microphone,
    SystemAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureChunkKind {
    Microphone,
    SystemAudio,
}

impl CaptureChunkKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::SystemAudio => "system-audio",
        }
    }
}

impl CaptureStream {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::SystemAudio => "system-audio",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleRateMismatchPolicy {
    Strict,
    AdaptStreamRate,
}

impl SampleRateMismatchPolicy {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "strict" => Some(Self::Strict),
            "adapt-stream-rate" => Some(Self::AdaptStreamRate),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::AdaptStreamRate => "adapt-stream-rate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureRecoveryAction {
    DropSampleContinue,
    RestartStream,
    AdaptOutputRate,
    FailFastReconfigure,
}

impl CaptureRecoveryAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DropSampleContinue => "DropSampleContinue",
            Self::RestartStream => "RestartStream",
            Self::AdaptOutputRate => "AdaptOutputRate",
            Self::FailFastReconfigure => "FailFastReconfigure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureEventCode {
    StreamInterruption,
    SlotMissDrops,
    FillFailures,
    QueueFullDrops,
    RecycleFailures,
    MissingAudioBufferList,
    MissingFirstAudioBuffer,
    MissingFormatDescription,
    MissingSampleRate,
    NonFloatPcm,
    ChunkTooLarge,
}

impl CaptureEventCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StreamInterruption => "stream_interruption",
            Self::SlotMissDrops => "slot_miss_drops",
            Self::FillFailures => "fill_failures",
            Self::QueueFullDrops => "queue_full_drops",
            Self::RecycleFailures => "recycle_failures",
            Self::MissingAudioBufferList => "missing_audio_buffer_list",
            Self::MissingFirstAudioBuffer => "missing_first_audio_buffer",
            Self::MissingFormatDescription => "missing_format_description",
            Self::MissingSampleRate => "missing_sample_rate",
            Self::NonFloatPcm => "non_float_pcm",
            Self::ChunkTooLarge => "chunk_too_large",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CaptureChunk {
    pub stream: CaptureStream,
    pub pts_seconds: f64,
    pub sample_rate_hz: u32,
    pub mono_samples: Vec<f32>,
}

#[derive(Debug, Clone)]
pub enum CaptureMessage {
    Chunk(CaptureChunk),
    Event(CaptureEvent),
    Finished(CaptureSummary),
}

#[derive(Debug, Clone)]
pub struct CaptureChunkSummary {
    pub kind: CaptureChunkKind,
    pub pts_seconds: f64,
    pub sample_rate_hz: u32,
    pub frame_count: usize,
}

#[derive(Debug, Clone)]
pub struct CaptureEvent {
    pub generated_unix: u64,
    pub code: CaptureEventCode,
    pub count: u64,
    pub recovery_action: CaptureRecoveryAction,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct CaptureDegradationEvent {
    pub generated_unix: u64,
    pub stage: String,
    pub source: String,
    pub count: u64,
    pub recovery_action: CaptureRecoveryAction,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResampleSummary {
    pub resampled_chunks: usize,
    pub input_frames: usize,
    pub output_frames: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureResampleSummary {
    pub resampled_chunks: usize,
    pub input_frames: usize,
    pub output_frames: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallbackContractSummary {
    pub missing_audio_buffer_list: u64,
    pub missing_first_audio_buffer: u64,
    pub missing_format_description: u64,
    pub missing_sample_rate: u64,
    pub non_float_pcm: u64,
    pub chunk_too_large: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureCallbackAuditSummary {
    pub missing_audio_buffer_list: u64,
    pub missing_first_audio_buffer: u64,
    pub missing_format_description: u64,
    pub missing_sample_rate: u64,
    pub non_float_pcm: u64,
    pub chunk_too_large: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureTransportSummary {
    pub capacity: u64,
    pub ready_depth_high_water: u64,
    pub in_flight: u64,
    pub enqueued: u64,
    pub dequeued: u64,
    pub slot_miss_drops: u64,
    pub fill_failures: u64,
    pub queue_full_drops: u64,
    pub recycle_failures: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureSampleRatePolicySummary {
    pub mismatch_policy: String,
    pub target_rate_hz: u32,
    pub output_rate_hz: u32,
    pub mic_input_rate_hz: u32,
    pub system_input_rate_hz: u32,
    pub mic_resample: CaptureResampleSummary,
    pub system_resample: CaptureResampleSummary,
}

#[derive(Debug, Clone)]
pub struct CaptureRunSummary {
    pub output_wav_path: String,
    pub duration_secs: u64,
    pub target_rate_hz: u32,
    pub output_rate_hz: u32,
    pub mic_chunks: usize,
    pub system_chunks: usize,
    pub output_frames: usize,
    pub restart_count: u64,
    pub transport: CaptureTransportSummary,
    pub callback_audit: CaptureCallbackAuditSummary,
    pub sample_rate_policy: CaptureSampleRatePolicySummary,
    pub degradation_events: Vec<CaptureDegradationEvent>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureStreamSummary {
    pub input_rate_hz: u32,
    pub chunk_count: usize,
    pub resample: ResampleSummary,
}

#[derive(Debug, Clone)]
pub struct CaptureSummary {
    pub generated_unix: u64,
    pub output_wav_path: PathBuf,
    pub duration_secs: u64,
    pub target_rate_hz: u32,
    pub output_rate_hz: u32,
    pub mismatch_policy: SampleRateMismatchPolicy,
    pub microphone: CaptureStreamSummary,
    pub system_audio: CaptureStreamSummary,
    pub output_frames: usize,
    pub restart_count: usize,
    pub transport: TransportStatsSnapshot,
    pub callback_contract: CallbackContractSummary,
    pub degradation_events: Vec<CaptureEvent>,
}

pub trait CaptureSink {
    fn on_chunk(&mut self, chunk: CaptureChunk) -> Result<(), String>;
    fn on_event(&mut self, event: CaptureEvent) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct StreamingCaptureResult {
    pub summary: CaptureSummary,
    pub progressive_output_path: PathBuf,
}

impl CaptureSummary {
    pub fn degraded(&self) -> bool {
        !self.degradation_events.is_empty()
    }
}

pub fn capture_telemetry_path_for_output(output: &Path) -> PathBuf {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("capture");
    parent.join(format!("{stem}.telemetry.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingSink {
        chunks: Vec<CaptureChunk>,
        events: Vec<CaptureEvent>,
        reject_chunks: bool,
        reject_events: bool,
    }

    impl CaptureSink for RecordingSink {
        fn on_chunk(&mut self, chunk: CaptureChunk) -> Result<(), String> {
            if self.reject_chunks {
                return Err("chunk rejected by sink".to_string());
            }
            self.chunks.push(chunk);
            Ok(())
        }

        fn on_event(&mut self, event: CaptureEvent) -> Result<(), String> {
            if self.reject_events {
                return Err("event rejected by sink".to_string());
            }
            self.events.push(event);
            Ok(())
        }
    }

    #[test]
    fn sample_rate_policy_parser_accepts_known_values() {
        assert_eq!(
            SampleRateMismatchPolicy::parse("strict"),
            Some(SampleRateMismatchPolicy::Strict)
        );
        assert_eq!(
            SampleRateMismatchPolicy::parse("adapt-stream-rate"),
            Some(SampleRateMismatchPolicy::AdaptStreamRate)
        );
        assert_eq!(SampleRateMismatchPolicy::parse("unknown"), None);
    }

    #[test]
    fn capture_event_code_strings_are_stable() {
        assert_eq!(
            CaptureEventCode::StreamInterruption.as_str(),
            "stream_interruption"
        );
        assert_eq!(
            CaptureEventCode::QueueFullDrops.as_str(),
            "queue_full_drops"
        );
        assert_eq!(
            CaptureEventCode::MissingSampleRate.as_str(),
            "missing_sample_rate"
        );
    }

    #[test]
    fn capture_summary_degraded_reflects_event_presence() {
        let clean = CaptureSummary {
            generated_unix: 0,
            output_wav_path: PathBuf::from("capture.wav"),
            duration_secs: 10,
            target_rate_hz: 48_000,
            output_rate_hz: 48_000,
            mismatch_policy: SampleRateMismatchPolicy::AdaptStreamRate,
            microphone: CaptureStreamSummary::default(),
            system_audio: CaptureStreamSummary::default(),
            output_frames: 0,
            restart_count: 0,
            transport: TransportStatsSnapshot::default(),
            callback_contract: CallbackContractSummary::default(),
            degradation_events: Vec::new(),
        };
        assert!(!clean.degraded());

        let degraded = CaptureSummary {
            degradation_events: vec![CaptureEvent {
                generated_unix: 1,
                code: CaptureEventCode::StreamInterruption,
                count: 1,
                recovery_action: CaptureRecoveryAction::RestartStream,
                detail: "restart attempted".to_string(),
            }],
            ..clean
        };
        assert!(degraded.degraded());
    }

    #[test]
    fn capture_sink_contract_records_chunks_and_events() {
        let mut sink = RecordingSink::default();
        let chunk = CaptureChunk {
            stream: CaptureStream::Microphone,
            pts_seconds: 1.25,
            sample_rate_hz: 16_000,
            mono_samples: vec![0.2, 0.3, 0.4],
        };
        let event = CaptureEvent {
            generated_unix: 10,
            code: CaptureEventCode::QueueFullDrops,
            count: 2,
            recovery_action: CaptureRecoveryAction::DropSampleContinue,
            detail: "dropped chunk due to full queue".to_string(),
        };

        sink.on_chunk(chunk.clone())
            .expect("capture sink should accept chunk");
        sink.on_event(event.clone())
            .expect("capture sink should accept event");

        assert_eq!(sink.chunks.len(), 1);
        assert_eq!(sink.events.len(), 1);
        assert_eq!(sink.chunks[0].stream, CaptureStream::Microphone);
        assert_eq!(sink.events[0].code, CaptureEventCode::QueueFullDrops);
    }

    #[test]
    fn capture_message_variants_match_payload_contract() {
        let chunk = CaptureChunk {
            stream: CaptureStream::SystemAudio,
            pts_seconds: 0.5,
            sample_rate_hz: 48_000,
            mono_samples: vec![0.0, 0.1],
        };
        let event = CaptureEvent {
            generated_unix: 22,
            code: CaptureEventCode::StreamInterruption,
            count: 1,
            recovery_action: CaptureRecoveryAction::RestartStream,
            detail: "capture interrupted".to_string(),
        };
        let summary = CaptureSummary {
            generated_unix: 42,
            output_wav_path: PathBuf::from("progressive.wav"),
            duration_secs: 5,
            target_rate_hz: 48_000,
            output_rate_hz: 48_000,
            mismatch_policy: SampleRateMismatchPolicy::AdaptStreamRate,
            microphone: CaptureStreamSummary::default(),
            system_audio: CaptureStreamSummary::default(),
            output_frames: 2,
            restart_count: 0,
            transport: TransportStatsSnapshot::default(),
            callback_contract: CallbackContractSummary::default(),
            degradation_events: Vec::new(),
        };

        let chunk_message = CaptureMessage::Chunk(chunk.clone());
        match chunk_message {
            CaptureMessage::Chunk(value) => assert_eq!(value.sample_rate_hz, 48_000),
            _ => panic!("expected chunk message"),
        }

        let event_message = CaptureMessage::Event(event.clone());
        match event_message {
            CaptureMessage::Event(value) => {
                assert_eq!(value.recovery_action, CaptureRecoveryAction::RestartStream)
            }
            _ => panic!("expected event message"),
        }

        let finished_message = CaptureMessage::Finished(summary.clone());
        match finished_message {
            CaptureMessage::Finished(value) => {
                assert_eq!(value.output_wav_path, summary.output_wav_path)
            }
            _ => panic!("expected finished message"),
        }

        let result = StreamingCaptureResult {
            summary,
            progressive_output_path: PathBuf::from("progressive.wav"),
        };
        assert_eq!(
            result.progressive_output_path.to_string_lossy(),
            "progressive.wav"
        );
    }

    #[test]
    fn capture_sink_errors_are_string_typed() {
        let mut sink = RecordingSink {
            reject_chunks: true,
            reject_events: true,
            ..RecordingSink::default()
        };
        let chunk_err = sink
            .on_chunk(CaptureChunk {
                stream: CaptureStream::Microphone,
                pts_seconds: 0.0,
                sample_rate_hz: 16_000,
                mono_samples: vec![0.0],
            })
            .expect_err("chunk should be rejected");
        let event_err = sink
            .on_event(CaptureEvent {
                generated_unix: 0,
                code: CaptureEventCode::FillFailures,
                count: 1,
                recovery_action: CaptureRecoveryAction::DropSampleContinue,
                detail: "failed to fill chunk".to_string(),
            })
            .expect_err("event should be rejected");

        assert!(chunk_err.contains("chunk rejected"));
        assert!(event_err.contains("event rejected"));
    }
}
