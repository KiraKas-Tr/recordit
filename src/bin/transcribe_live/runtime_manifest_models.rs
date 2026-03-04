#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeManifest {
    pub(crate) schema_version: String,
    pub(crate) kind: String,
    pub(crate) generated_at_utc: String,
    pub(crate) asr_backend: String,
    pub(crate) asr_model: String,
    pub(crate) asr_model_source: String,
    pub(crate) asr_model_checksum_sha256: String,
    pub(crate) asr_model_checksum_status: String,
    pub(crate) input_wav: String,
    pub(crate) input_wav_semantics: String,
    pub(crate) out_wav: String,
    pub(crate) out_wav_semantics: String,
    pub(crate) out_wav_materialized: bool,
    pub(crate) out_wav_bytes: u64,
    pub(crate) channel_mode: String,
    pub(crate) channel_mode_requested: String,
    pub(crate) runtime_mode: String,
    pub(crate) runtime_mode_taxonomy: String,
    pub(crate) runtime_mode_selector: String,
    pub(crate) runtime_mode_status: String,
    pub(crate) live_config: RuntimeLiveConfig,
    pub(crate) lifecycle: RuntimeLifecycle,
    pub(crate) speaker_labels: Vec<String>,
    pub(crate) event_channels: Vec<String>,
    pub(crate) vad: RuntimeVad,
    pub(crate) transcript: RuntimeTranscript,
    pub(crate) readability_defaults: RuntimeReadabilityDefaults,
    pub(crate) transcript_per_channel: Vec<RuntimeTranscriptPerChannel>,
    pub(crate) terminal_summary: RuntimeTerminalSummary,
    pub(crate) first_emit_timing_ms: RuntimeFirstEmitTiming,
    pub(crate) queue_defer: RuntimeQueueDefer,
    pub(crate) ordering_metadata: RuntimeOrderingMetadata,
    pub(crate) events: Vec<RuntimeTranscriptEvent>,
    pub(crate) benchmark: RuntimeBenchmark,
    pub(crate) reconciliation: RuntimeReconciliation,
    pub(crate) asr_worker_pool: RuntimeAsrWorkerPool,
    pub(crate) chunk_queue: RuntimeChunkQueue,
    pub(crate) cleanup_queue: RuntimeCleanupQueue,
    pub(crate) degradation_events: Vec<RuntimeDegradationEvent>,
    pub(crate) trust: RuntimeTrust,
    pub(crate) event_counts: RuntimeEventCounts,
    pub(crate) session_summary: RuntimeSessionSummary,
    pub(crate) jsonl_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeLiveConfig {
    pub(crate) live_chunked: bool,
    pub(crate) chunk_window_ms: u64,
    pub(crate) chunk_stride_ms: u64,
    pub(crate) chunk_queue_cap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeLifecycle {
    pub(crate) current_phase: String,
    pub(crate) ready_for_transcripts: bool,
    pub(crate) transitions: Vec<RuntimeLifecycleTransition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeLifecycleTransition {
    pub(crate) phase: String,
    pub(crate) transition_index: usize,
    pub(crate) entered_at_utc: String,
    pub(crate) ready_for_transcripts: bool,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeVad {
    pub(crate) backend: String,
    pub(crate) threshold: f32,
    pub(crate) min_speech_ms: u64,
    pub(crate) min_silence_ms: u64,
    pub(crate) boundary_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeTranscript {
    pub(crate) segment_id: String,
    pub(crate) start_ms: u64,
    pub(crate) end_ms: u64,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeReadabilityDefaults {
    pub(crate) merged_line_format: String,
    pub(crate) near_overlap_window_ms: u64,
    pub(crate) near_overlap_annotation: String,
    pub(crate) ordering: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeTranscriptPerChannel {
    pub(crate) channel: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeTerminalSummary {
    pub(crate) live_mode: bool,
    pub(crate) render_mode: String,
    pub(crate) stable_line_policy: String,
    pub(crate) stable_line_count: usize,
    pub(crate) stable_lines_replayed: bool,
    pub(crate) stable_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeFirstEmitTiming {
    pub(crate) first_any: Option<u64>,
    pub(crate) first_partial: Option<u64>,
    pub(crate) first_final: Option<u64>,
    pub(crate) first_stable: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeQueueDefer {
    pub(crate) submit_window: usize,
    pub(crate) deferred_final_submissions: usize,
    pub(crate) max_pending_final_backlog: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeOrderingMetadata {
    pub(crate) event_sort_key: String,
    pub(crate) stable_line_sort_key: String,
    pub(crate) stable_line_event_types: Vec<String>,
    pub(crate) event_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeTranscriptEvent {
    pub(crate) event_type: String,
    pub(crate) channel: String,
    pub(crate) segment_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_final_segment_id: Option<String>,
    pub(crate) start_ms: u64,
    pub(crate) end_ms: u64,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeBenchmark {
    pub(crate) run_count: usize,
    pub(crate) wall_ms_p50: f64,
    pub(crate) wall_ms_p95: f64,
    pub(crate) partial_slo_met: bool,
    pub(crate) final_slo_met: bool,
    pub(crate) summary_csv: String,
    pub(crate) runs_csv: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeReconciliation {
    pub(crate) required: bool,
    pub(crate) applied: bool,
    pub(crate) trigger_count: usize,
    pub(crate) trigger_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeAsrWorkerPool {
    pub(crate) prewarm_ok: bool,
    pub(crate) submitted: usize,
    pub(crate) enqueued: usize,
    pub(crate) dropped_queue_full: usize,
    pub(crate) processed: usize,
    pub(crate) succeeded: usize,
    pub(crate) failed: usize,
    pub(crate) retry_attempts: usize,
    pub(crate) temp_audio_deleted: usize,
    pub(crate) temp_audio_retained: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeChunkQueue {
    pub(crate) enabled: bool,
    pub(crate) max_queue: usize,
    pub(crate) submitted: usize,
    pub(crate) enqueued: usize,
    pub(crate) dropped_oldest: usize,
    pub(crate) processed: usize,
    pub(crate) pending: usize,
    pub(crate) high_water: usize,
    pub(crate) drain_completed: bool,
    pub(crate) lag_sample_count: usize,
    pub(crate) lag_p50_ms: u64,
    pub(crate) lag_p95_ms: u64,
    pub(crate) lag_max_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeCleanupQueue {
    pub(crate) enabled: bool,
    pub(crate) max_queue: usize,
    pub(crate) timeout_ms: u64,
    pub(crate) retries: usize,
    pub(crate) submitted: usize,
    pub(crate) enqueued: usize,
    pub(crate) dropped_queue_full: usize,
    pub(crate) processed: usize,
    pub(crate) succeeded: usize,
    pub(crate) timed_out: usize,
    pub(crate) failed: usize,
    pub(crate) retry_attempts: usize,
    pub(crate) pending: usize,
    pub(crate) drain_budget_ms: u64,
    pub(crate) drain_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeDegradationEvent {
    pub(crate) code: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeTrust {
    pub(crate) degraded_mode_active: bool,
    pub(crate) notice_count: usize,
    pub(crate) notices: Vec<RuntimeTrustNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeTrustNotice {
    pub(crate) code: String,
    pub(crate) severity: String,
    pub(crate) cause: String,
    pub(crate) impact: String,
    pub(crate) guidance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeEventCounts {
    pub(crate) vad_boundary: usize,
    pub(crate) transcript: usize,
    pub(crate) partial: usize,
    #[serde(rename = "final")]
    pub(crate) final_count: usize,
    pub(crate) llm_final: usize,
    pub(crate) reconciled_final: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeSessionSummary {
    pub(crate) session_status: String,
    pub(crate) duration_sec: u64,
    pub(crate) channel_mode_requested: String,
    pub(crate) channel_mode_active: String,
    pub(crate) transcript_events: RuntimeSessionTranscriptEvents,
    pub(crate) chunk_queue: RuntimeSessionChunkQueue,
    pub(crate) chunk_lag: RuntimeSessionChunkLag,
    pub(crate) trust_notices: RuntimeSessionCodeSummary,
    pub(crate) degradation_events: RuntimeSessionCodeSummary,
    pub(crate) cleanup_queue: RuntimeSessionCleanupQueue,
    pub(crate) artifacts: RuntimeSessionArtifacts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeSessionTranscriptEvents {
    pub(crate) partial: usize,
    #[serde(rename = "final")]
    pub(crate) final_count: usize,
    pub(crate) llm_final: usize,
    pub(crate) reconciled_final: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeSessionChunkQueue {
    pub(crate) submitted: usize,
    pub(crate) enqueued: usize,
    pub(crate) dropped_oldest: usize,
    pub(crate) processed: usize,
    pub(crate) pending: usize,
    pub(crate) high_water: usize,
    pub(crate) drain_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeSessionChunkLag {
    pub(crate) lag_sample_count: usize,
    pub(crate) lag_p50_ms: u64,
    pub(crate) lag_p95_ms: u64,
    pub(crate) lag_max_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeSessionCodeSummary {
    pub(crate) count: usize,
    pub(crate) top_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeSessionCleanupQueue {
    pub(crate) enabled: bool,
    pub(crate) submitted: usize,
    pub(crate) enqueued: usize,
    pub(crate) dropped_queue_full: usize,
    pub(crate) processed: usize,
    pub(crate) succeeded: usize,
    pub(crate) timed_out: usize,
    pub(crate) failed: usize,
    pub(crate) retry_attempts: usize,
    pub(crate) pending: usize,
    pub(crate) drain_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeSessionArtifacts {
    pub(crate) out_wav: String,
    pub(crate) out_jsonl: String,
    pub(crate) out_manifest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreflightManifest {
    pub(crate) schema_version: String,
    pub(crate) kind: String,
    pub(crate) generated_at_utc: String,
    pub(crate) overall_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_mode_taxonomy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_mode_selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_mode_status: Option<String>,
    pub(crate) config: PreflightConfig,
    pub(crate) checks: Vec<PreflightCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreflightConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) input_wav: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) input_wav_semantics: Option<String>,
    pub(crate) out_wav: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) out_wav_semantics: Option<String>,
    pub(crate) out_jsonl: String,
    pub(crate) out_manifest: String,
    pub(crate) asr_backend: String,
    pub(crate) asr_model_requested: String,
    pub(crate) asr_model_resolved: String,
    pub(crate) asr_model_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) asr_model_checksum_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) asr_model_checksum_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_mode_taxonomy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_mode_selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_mode_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) live_chunked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) chunk_window_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) chunk_stride_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) chunk_queue_cap: Option<usize>,
    pub(crate) sample_rate_hz: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreflightCheck {
    pub(crate) id: String,
    pub(crate) status: String,
    pub(crate) detail: String,
    pub(crate) remediation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestDecodeError {
    pub(crate) manifest_kind: Option<String>,
    pub(crate) detail: String,
}

impl fmt::Display for ManifestDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.manifest_kind {
            Some(kind) => write!(f, "manifest kind `{kind}` decode error: {}", self.detail),
            None => write!(f, "manifest decode error: {}", self.detail),
        }
    }
}

impl std::error::Error for ManifestDecodeError {}

fn manifest_kind_hint(input: &str) -> Option<String> {
    serde_json::from_str::<Value>(input).ok().and_then(|value| {
        value
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

pub(crate) fn decode_runtime_manifest(input: &str) -> Result<RuntimeManifest, ManifestDecodeError> {
    serde_json::from_str::<RuntimeManifest>(input).map_err(|error| ManifestDecodeError {
        manifest_kind: manifest_kind_hint(input),
        detail: error.to_string(),
    })
}

pub(crate) fn decode_preflight_manifest(
    input: &str,
) -> Result<PreflightManifest, ManifestDecodeError> {
    serde_json::from_str::<PreflightManifest>(input).map_err(|error| ManifestDecodeError {
        manifest_kind: manifest_kind_hint(input),
        detail: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn project_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn runtime_manifest_models_roundtrip_frozen_fixtures_without_schema_drift() {
        let fixtures = [
            "artifacts/validation/bd-1qfx/representative-offline.runtime.manifest.json",
            "artifacts/validation/bd-1qfx/representative-chunked.runtime.manifest.json",
            "artifacts/validation/bd-1qfx/live-stream-cold.runtime.manifest.json",
            "artifacts/validation/bd-1qfx/live-stream-warm.runtime.manifest.json",
            "artifacts/validation/bd-1qfx/live-stream-backlog.runtime.manifest.json",
        ];

        for rel_path in fixtures {
            let path = project_root().join(rel_path);
            let raw = std::fs::read_to_string(&path).expect("failed reading runtime fixture");
            let original: Value = serde_json::from_str(&raw).expect("invalid runtime fixture json");
            let typed = decode_runtime_manifest(&raw).expect("typed runtime decode failed");
            let roundtrip = serde_json::to_value(&typed).expect("typed runtime encode failed");

            assert_eq!(
                roundtrip,
                original,
                "runtime manifest schema drift for {}",
                path.display()
            );
        }
    }

    #[test]
    fn preflight_manifest_models_roundtrip_fixture_without_schema_drift() {
        let path = project_root().join("artifacts/validation/bd-2p6.preflight.manifest.json");
        let raw = std::fs::read_to_string(&path).expect("failed reading preflight fixture");
        let original: Value = serde_json::from_str(&raw).expect("invalid preflight fixture json");
        let typed = decode_preflight_manifest(&raw).expect("typed preflight decode failed");
        let roundtrip = serde_json::to_value(&typed).expect("typed preflight encode failed");

        assert_eq!(
            roundtrip,
            original,
            "preflight manifest schema drift for {}",
            path.display()
        );
    }

    #[test]
    fn manifest_decode_errors_report_kind_and_field_context() {
        let invalid = r#"{"schema_version":"1","kind":"transcribe-live-runtime"}"#;
        let error = decode_runtime_manifest(invalid).expect_err("expected decode failure");

        assert_eq!(
            error.manifest_kind.as_deref(),
            Some("transcribe-live-runtime")
        );
        assert!(
            error.detail.contains("missing field") || error.detail.contains("unknown field"),
            "unexpected decode error detail: {}",
            error.detail
        );
    }
}
