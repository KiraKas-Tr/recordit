//! Typed contract model boundaries for Phase 2 JSON/manifest migration.
//! This module centralizes schema vocabulary so runtime logic can evolve
//! independently from on-disk artifact contract keys.
#![allow(dead_code)]

pub(crate) mod runtime_jsonl {
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use std::fmt;

    pub(crate) const EVENT_TYPE_VAD_BOUNDARY: &str = "vad_boundary";
    pub(crate) const EVENT_TYPE_MODE_DEGRADATION: &str = "mode_degradation";
    pub(crate) const EVENT_TYPE_TRUST_NOTICE: &str = "trust_notice";
    pub(crate) const EVENT_TYPE_LIFECYCLE_PHASE: &str = "lifecycle_phase";
    pub(crate) const EVENT_TYPE_RECONCILIATION_MATRIX: &str = "reconciliation_matrix";
    pub(crate) const EVENT_TYPE_ASR_WORKER_POOL: &str = "asr_worker_pool";
    pub(crate) const EVENT_TYPE_CHUNK_QUEUE: &str = "chunk_queue";
    pub(crate) const EVENT_TYPE_CLEANUP_QUEUE: &str = "cleanup_queue";
    pub(crate) const EVENT_TYPE_PARTIAL: &str = "partial";
    pub(crate) const EVENT_TYPE_STABLE_PARTIAL: &str = "stable_partial";
    pub(crate) const EVENT_TYPE_FINAL: &str = "final";
    pub(crate) const EVENT_TYPE_LLM_FINAL: &str = "llm_final";
    pub(crate) const EVENT_TYPE_RECONCILED_FINAL: &str = "reconciled_final";

    pub(crate) const RUNTIME_JSONL_EVENT_TYPES: &[&str] = &[
        EVENT_TYPE_VAD_BOUNDARY,
        EVENT_TYPE_MODE_DEGRADATION,
        EVENT_TYPE_TRUST_NOTICE,
        EVENT_TYPE_LIFECYCLE_PHASE,
        EVENT_TYPE_RECONCILIATION_MATRIX,
        EVENT_TYPE_ASR_WORKER_POOL,
        EVENT_TYPE_CHUNK_QUEUE,
        EVENT_TYPE_CLEANUP_QUEUE,
        EVENT_TYPE_PARTIAL,
        EVENT_TYPE_STABLE_PARTIAL,
        EVENT_TYPE_FINAL,
        EVENT_TYPE_LLM_FINAL,
        EVENT_TYPE_RECONCILED_FINAL,
    ];

    pub(crate) const TRANSCRIPT_EVENT_TYPES: &[&str] = &[
        EVENT_TYPE_PARTIAL,
        EVENT_TYPE_STABLE_PARTIAL,
        EVENT_TYPE_FINAL,
        EVENT_TYPE_LLM_FINAL,
        EVENT_TYPE_RECONCILED_FINAL,
    ];

    pub(crate) const VAD_BOUNDARY_KEYS: &[&str] = &[
        "event_type",
        "channel",
        "boundary_id",
        "start_ms",
        "end_ms",
        "source",
        "vad_backend",
        "vad_threshold",
    ];

    pub(crate) const MODE_DEGRADATION_KEYS: &[&str] = &[
        "event_type",
        "channel",
        "requested_mode",
        "active_mode",
        "code",
        "detail",
    ];

    pub(crate) const TRUST_NOTICE_KEYS: &[&str] = &[
        "event_type",
        "channel",
        "code",
        "severity",
        "cause",
        "impact",
        "guidance",
    ];

    pub(crate) const LIFECYCLE_PHASE_KEYS: &[&str] = &[
        "event_type",
        "channel",
        "phase",
        "transition_index",
        "entered_at_utc",
        "ready_for_transcripts",
        "detail",
    ];

    pub(crate) const RECONCILIATION_MATRIX_KEYS: &[&str] = &[
        "event_type",
        "channel",
        "required",
        "applied",
        "trigger_count",
        "trigger_codes",
    ];

    pub(crate) const ASR_WORKER_POOL_KEYS: &[&str] = &[
        "event_type",
        "channel",
        "prewarm_ok",
        "submitted",
        "enqueued",
        "dropped_queue_full",
        "processed",
        "succeeded",
        "failed",
        "retry_attempts",
        "temp_audio_deleted",
        "temp_audio_retained",
    ];

    pub(crate) const CHUNK_QUEUE_KEYS: &[&str] = &[
        "event_type",
        "channel",
        "enabled",
        "max_queue",
        "submitted",
        "enqueued",
        "dropped_oldest",
        "processed",
        "pending",
        "high_water",
        "drain_completed",
        "lag_sample_count",
        "lag_p50_ms",
        "lag_p95_ms",
        "lag_max_ms",
    ];

    pub(crate) const CLEANUP_QUEUE_KEYS: &[&str] = &[
        "event_type",
        "channel",
        "enabled",
        "max_queue",
        "timeout_ms",
        "retries",
        "submitted",
        "enqueued",
        "dropped_queue_full",
        "processed",
        "succeeded",
        "timed_out",
        "failed",
        "retry_attempts",
        "pending",
        "drain_budget_ms",
        "drain_completed",
    ];

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct VadBoundaryEventModel {
        pub(crate) channel: String,
        pub(crate) boundary_id: usize,
        pub(crate) start_ms: u64,
        pub(crate) end_ms: u64,
        pub(crate) source: String,
        pub(crate) vad_backend: String,
        pub(crate) vad_threshold: f64,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct ModeDegradationEventModel {
        pub(crate) channel: String,
        pub(crate) requested_mode: String,
        pub(crate) active_mode: String,
        pub(crate) code: String,
        pub(crate) detail: String,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct TrustNoticeEventModel {
        pub(crate) channel: String,
        pub(crate) code: String,
        pub(crate) severity: String,
        pub(crate) cause: String,
        pub(crate) impact: String,
        pub(crate) guidance: String,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct LifecyclePhaseEventModel {
        pub(crate) channel: String,
        pub(crate) phase: String,
        pub(crate) transition_index: usize,
        pub(crate) entered_at_utc: String,
        pub(crate) ready_for_transcripts: bool,
        pub(crate) detail: String,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct ReconciliationMatrixEventModel {
        pub(crate) channel: String,
        pub(crate) required: bool,
        pub(crate) applied: bool,
        pub(crate) trigger_count: usize,
        pub(crate) trigger_codes: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct AsrWorkerPoolEventModel {
        pub(crate) channel: String,
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

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct ChunkQueueEventModel {
        pub(crate) channel: String,
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
        pub(crate) lag_p50_ms: usize,
        pub(crate) lag_p95_ms: usize,
        pub(crate) lag_max_ms: usize,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct CleanupQueueEventModel {
        pub(crate) channel: String,
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

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(crate) struct TranscriptArtifactEventModel {
        #[serde(default = "default_transcript_channel")]
        pub(crate) channel: String,
        pub(crate) segment_id: String,
        pub(crate) source_final_segment_id: Option<String>,
        pub(crate) start_ms: u64,
        pub(crate) end_ms: u64,
        #[serde(default)]
        pub(crate) text: String,
        #[serde(default)]
        pub(crate) asr_backend: String,
        #[serde(default)]
        pub(crate) vad_boundary_count: usize,
    }

    fn default_transcript_channel() -> String {
        "merged".to_string()
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "event_type")]
    pub(crate) enum RuntimeJsonlEvent {
        #[serde(rename = "vad_boundary")]
        VadBoundary(VadBoundaryEventModel),
        #[serde(rename = "mode_degradation")]
        ModeDegradation(ModeDegradationEventModel),
        #[serde(rename = "trust_notice")]
        TrustNotice(TrustNoticeEventModel),
        #[serde(rename = "lifecycle_phase")]
        LifecyclePhase(LifecyclePhaseEventModel),
        #[serde(rename = "reconciliation_matrix")]
        ReconciliationMatrix(ReconciliationMatrixEventModel),
        #[serde(rename = "asr_worker_pool")]
        AsrWorkerPool(AsrWorkerPoolEventModel),
        #[serde(rename = "chunk_queue")]
        ChunkQueue(ChunkQueueEventModel),
        #[serde(rename = "cleanup_queue")]
        CleanupQueue(CleanupQueueEventModel),
        #[serde(rename = "partial")]
        Partial(TranscriptArtifactEventModel),
        #[serde(rename = "stable_partial")]
        StablePartial(TranscriptArtifactEventModel),
        #[serde(rename = "final")]
        Final(TranscriptArtifactEventModel),
        #[serde(rename = "llm_final")]
        LlmFinal(TranscriptArtifactEventModel),
        #[serde(rename = "reconciled_final")]
        ReconciledFinal(TranscriptArtifactEventModel),
    }

    impl RuntimeJsonlEvent {
        pub(crate) fn event_type(&self) -> &'static str {
            match self {
                Self::VadBoundary(_) => EVENT_TYPE_VAD_BOUNDARY,
                Self::ModeDegradation(_) => EVENT_TYPE_MODE_DEGRADATION,
                Self::TrustNotice(_) => EVENT_TYPE_TRUST_NOTICE,
                Self::LifecyclePhase(_) => EVENT_TYPE_LIFECYCLE_PHASE,
                Self::ReconciliationMatrix(_) => EVENT_TYPE_RECONCILIATION_MATRIX,
                Self::AsrWorkerPool(_) => EVENT_TYPE_ASR_WORKER_POOL,
                Self::ChunkQueue(_) => EVENT_TYPE_CHUNK_QUEUE,
                Self::CleanupQueue(_) => EVENT_TYPE_CLEANUP_QUEUE,
                Self::Partial(_) => EVENT_TYPE_PARTIAL,
                Self::StablePartial(_) => EVENT_TYPE_STABLE_PARTIAL,
                Self::Final(_) => EVENT_TYPE_FINAL,
                Self::LlmFinal(_) => EVENT_TYPE_LLM_FINAL,
                Self::ReconciledFinal(_) => EVENT_TYPE_RECONCILED_FINAL,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct RuntimeJsonlParseError {
        detail: String,
    }

    impl RuntimeJsonlParseError {
        fn new(detail: impl Into<String>) -> Self {
            Self {
                detail: detail.into(),
            }
        }
    }

    impl fmt::Display for RuntimeJsonlParseError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.detail)
        }
    }

    impl std::error::Error for RuntimeJsonlParseError {}

    pub(crate) fn parse_runtime_jsonl_event_line(
        line: &str,
    ) -> Result<RuntimeJsonlEvent, RuntimeJsonlParseError> {
        let value: Value = serde_json::from_str(line)
            .map_err(|err| RuntimeJsonlParseError::new(format!("invalid json line: {err}")))?;
        parse_runtime_jsonl_event_value(value)
    }

    fn parse_runtime_jsonl_event_value(
        value: Value,
    ) -> Result<RuntimeJsonlEvent, RuntimeJsonlParseError> {
        let event_type = value
            .as_object()
            .and_then(|obj| obj.get("event_type"))
            .and_then(Value::as_str)
            .ok_or_else(|| RuntimeJsonlParseError::new("missing string field `event_type`"))?
            .to_string();
        if !RUNTIME_JSONL_EVENT_TYPES.contains(&event_type.as_str()) {
            return Err(RuntimeJsonlParseError::new(format!(
                "unknown event_type `{event_type}`"
            )));
        }
        serde_json::from_value::<RuntimeJsonlEvent>(value.clone()).map_err(|err| {
            RuntimeJsonlParseError::new(format!(
                "event_type `{event_type}` payload mismatch: {err}"
            ))
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::collections::BTreeMap;
        use std::fs;
        use std::path::PathBuf;

        fn frozen_runtime_jsonl_fixtures() -> Vec<PathBuf> {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            vec![
                root.join("artifacts/bench/gate_v1_acceptance/20260301T130355Z/cold/runtime.jsonl"),
                root.join("artifacts/bench/gate_v1_acceptance/20260301T130355Z/warm/runtime.jsonl"),
                root.join("artifacts/bench/gate_backlog_pressure/20260302T074649Z/runtime.jsonl"),
            ]
        }

        fn replay_event_debug(event: &RuntimeJsonlEvent) -> String {
            let as_value = serde_json::to_value(event).unwrap_or(Value::Null);
            let channel = as_value
                .get("channel")
                .and_then(Value::as_str)
                .unwrap_or("<none>");
            let segment_id = as_value
                .get("segment_id")
                .and_then(Value::as_str)
                .unwrap_or("<none>");
            let start_ms = as_value
                .get("start_ms")
                .and_then(Value::as_u64)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<none>".to_string());
            let end_ms = as_value
                .get("end_ms")
                .and_then(Value::as_u64)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<none>".to_string());
            format!(
                "event_type={} channel={} segment_id={} start_ms={} end_ms={}",
                event.event_type(),
                channel,
                segment_id,
                start_ms,
                end_ms
            )
        }

        #[test]
        fn runtime_jsonl_event_roundtrip_parity_across_all_variants() {
            let fixtures = vec![
                (
                    EVENT_TYPE_VAD_BOUNDARY,
                    r#"{"event_type":"vad_boundary","channel":"merged","boundary_id":1,"start_ms":0,"end_ms":250,"source":"live_runtime","vad_backend":"silero","vad_threshold":0.125}"#,
                ),
                (
                    EVENT_TYPE_MODE_DEGRADATION,
                    r#"{"event_type":"mode_degradation","channel":"control","requested_mode":"mixed-fallback","active_mode":"mixed","code":"fallback_to_mixed","detail":"mono input"}"#,
                ),
                (
                    EVENT_TYPE_TRUST_NOTICE,
                    r#"{"event_type":"trust_notice","channel":"control","code":"mode_degradation","severity":"warn","cause":"mono input","impact":"channel attribution reduced","guidance":"use stereo input"}"#,
                ),
                (
                    EVENT_TYPE_LIFECYCLE_PHASE,
                    r#"{"event_type":"lifecycle_phase","channel":"control","phase":"active","transition_index":1,"entered_at_utc":"2026-03-04T00:00:00Z","ready_for_transcripts":true,"detail":"stream started"}"#,
                ),
                (
                    EVENT_TYPE_RECONCILIATION_MATRIX,
                    r#"{"event_type":"reconciliation_matrix","channel":"control","required":true,"applied":true,"trigger_count":1,"trigger_codes":["queue_drop"]}"#,
                ),
                (
                    EVENT_TYPE_ASR_WORKER_POOL,
                    r#"{"event_type":"asr_worker_pool","channel":"control","prewarm_ok":true,"submitted":5,"enqueued":5,"dropped_queue_full":0,"processed":5,"succeeded":5,"failed":0,"retry_attempts":0,"temp_audio_deleted":5,"temp_audio_retained":0}"#,
                ),
                (
                    EVENT_TYPE_CHUNK_QUEUE,
                    r#"{"event_type":"chunk_queue","channel":"control","enabled":true,"max_queue":2,"submitted":12,"enqueued":12,"dropped_oldest":1,"processed":11,"pending":0,"high_water":2,"drain_completed":true,"lag_sample_count":4,"lag_p50_ms":30,"lag_p95_ms":80,"lag_max_ms":120}"#,
                ),
                (
                    EVENT_TYPE_CLEANUP_QUEUE,
                    r#"{"event_type":"cleanup_queue","channel":"control","enabled":true,"max_queue":4,"timeout_ms":2500,"retries":2,"submitted":6,"enqueued":6,"dropped_queue_full":0,"processed":6,"succeeded":5,"timed_out":0,"failed":1,"retry_attempts":1,"pending":0,"drain_budget_ms":1200,"drain_completed":true}"#,
                ),
                (
                    EVENT_TYPE_PARTIAL,
                    r#"{"event_type":"partial","channel":"mic","segment_id":"mic-chunk-0000","source_final_segment_id":null,"start_ms":0,"end_ms":500,"text":"hello","asr_backend":"whispercpp","vad_boundary_count":1}"#,
                ),
                (
                    EVENT_TYPE_STABLE_PARTIAL,
                    r#"{"event_type":"stable_partial","channel":"mic","segment_id":"mic-chunk-0000","source_final_segment_id":null,"start_ms":0,"end_ms":700,"text":"hello there","asr_backend":"whispercpp","vad_boundary_count":1}"#,
                ),
                (
                    EVENT_TYPE_FINAL,
                    r#"{"event_type":"final","channel":"mic","segment_id":"mic-chunk-0001","source_final_segment_id":null,"start_ms":500,"end_ms":1500,"text":"hello world","asr_backend":"whispercpp","vad_boundary_count":2}"#,
                ),
                (
                    EVENT_TYPE_LLM_FINAL,
                    r#"{"event_type":"llm_final","channel":"system","segment_id":"system-chunk-0001","source_final_segment_id":"system-chunk-0000","start_ms":500,"end_ms":1500,"text":"hello world cleaned","asr_backend":"whispercpp","vad_boundary_count":2}"#,
                ),
                (
                    EVENT_TYPE_RECONCILED_FINAL,
                    r#"{"event_type":"reconciled_final","channel":"merged","segment_id":"merged-reconciled-0001","source_final_segment_id":"merged-final-0001","start_ms":500,"end_ms":1500,"text":"hello world (reconciled)","asr_backend":"whispercpp","vad_boundary_count":2}"#,
                ),
            ];

            for (expected_type, fixture) in fixtures {
                let parsed = parse_runtime_jsonl_event_line(fixture).expect("fixture parses");
                assert_eq!(parsed.event_type(), expected_type);
                let roundtrip = serde_json::to_string(&parsed).expect("serialize event");
                let reparsed =
                    parse_runtime_jsonl_event_line(&roundtrip).expect("roundtrip reparses");
                assert_eq!(reparsed, parsed);
            }
        }

        #[test]
        fn runtime_jsonl_unknown_event_type_diagnostic_is_explicit() {
            let err = parse_runtime_jsonl_event_line(
                r#"{"event_type":"mystery_control_event","channel":"control"}"#,
            )
            .expect_err("unknown event_type must fail");
            assert!(err
                .to_string()
                .contains("unknown event_type `mystery_control_event`"));
        }

        #[test]
        fn runtime_jsonl_variant_payload_mismatch_diagnostic_is_explicit() {
            let err = parse_runtime_jsonl_event_line(
                r#"{"event_type":"partial","channel":"mic","segment_id":"seg-1"}"#,
            )
            .expect_err("missing transcript fields must fail");
            let rendered = err.to_string();
            assert!(rendered.contains("event_type `partial` payload mismatch"));
            assert!(rendered.contains("missing field"));
        }

        #[test]
        fn runtime_jsonl_unknown_fields_preserve_replay_compatibility() {
            let parsed = parse_runtime_jsonl_event_line(
                r#"{"event_type":"final","channel":"mic","segment_id":"mic-seg-0001","source_final_segment_id":null,"start_ms":0,"end_ms":500,"text":"hello","asr_backend":"whispercpp","vad_boundary_count":1,"extra_field":"ignored"}"#,
            )
            .expect("unknown payload fields should not break typed replay compatibility");
            assert!(
                matches!(parsed, RuntimeJsonlEvent::Final(_)),
                "expected final event, got {}",
                parsed.event_type()
            );
            if let RuntimeJsonlEvent::Final(payload) = parsed {
                assert_eq!(payload.channel, "mic");
                assert_eq!(payload.segment_id, "mic-seg-0001");
                assert_eq!(payload.text, "hello");
            }
        }

        #[test]
        fn runtime_jsonl_frozen_fixtures_parse_with_typed_boundary_and_context() {
            for fixture in frozen_runtime_jsonl_fixtures() {
                assert!(
                    fixture.is_file(),
                    "missing frozen runtime fixture {}",
                    fixture.display()
                );
                let raw = fs::read_to_string(&fixture).expect("failed to read frozen fixture");
                let mut per_type = BTreeMap::<String, usize>::new();

                for (line_index, line) in raw.lines().enumerate() {
                    let parsed = parse_runtime_jsonl_event_line(line);
                    assert!(
                        parsed.is_ok(),
                        "{} line {} failed typed parse: {}\nrow={}",
                        fixture.display(),
                        line_index + 1,
                        parsed
                            .as_ref()
                            .err()
                            .map(std::string::ToString::to_string)
                            .unwrap_or_else(|| "<unknown parse failure>".to_string()),
                        line
                    );
                    let parsed = parsed.expect("typed parse should succeed after assertion");
                    let event_type = parsed.event_type().to_string();
                    *per_type.entry(event_type.clone()).or_insert(0) += 1;

                    let encoded = serde_json::to_string(&parsed);
                    assert!(
                        encoded.is_ok(),
                        "{} line {} ({}) failed encode",
                        fixture.display(),
                        line_index + 1,
                        event_type
                    );
                    let encoded = encoded.expect("typed encode should succeed after assertion");
                    let reparsed = parse_runtime_jsonl_event_line(&encoded);
                    assert!(
                        reparsed.is_ok(),
                        "{} line {} ({}) failed reparse after roundtrip: {}",
                        fixture.display(),
                        line_index + 1,
                        event_type,
                        reparsed
                            .as_ref()
                            .err()
                            .map(std::string::ToString::to_string)
                            .unwrap_or_else(|| "<unknown reparse failure>".to_string())
                    );
                    let reparsed = reparsed.expect("typed reparse should succeed after assertion");
                    assert_eq!(
                        reparsed,
                        parsed,
                        "{} line {} ({}) changed after typed roundtrip\nbefore={}\nafter={}",
                        fixture.display(),
                        line_index + 1,
                        event_type,
                        replay_event_debug(&parsed),
                        replay_event_debug(&reparsed)
                    );
                }

                let mut required_types = vec![
                    EVENT_TYPE_LIFECYCLE_PHASE,
                    EVENT_TYPE_PARTIAL,
                    EVENT_TYPE_FINAL,
                    EVENT_TYPE_CHUNK_QUEUE,
                ];
                required_types.sort_unstable();
                let mut missing = Vec::new();
                for required in required_types {
                    if !per_type.contains_key(required) {
                        missing.push(required.to_string());
                    }
                }
                assert!(
                    missing.is_empty(),
                    "{} missing expected event types. missing={:?} observed={:?}",
                    fixture.display(),
                    missing,
                    per_type
                );
            }
        }
    }
}
pub(crate) mod runtime_manifest {
    pub(crate) const KIND_RUNTIME_MANIFEST: &str = "transcribe-live-runtime";
    pub(crate) const KIND_PREFLIGHT_MANIFEST: &str = "transcribe-live-preflight";

    pub(crate) const RUNTIME_TOP_LEVEL_KEYS: &[&str] = &[
        "schema_version",
        "kind",
        "generated_at_utc",
        "asr_backend",
        "asr_model",
        "asr_model_source",
        "asr_model_checksum_sha256",
        "asr_model_checksum_status",
        "input_wav",
        "input_wav_semantics",
        "out_wav",
        "out_wav_semantics",
        "out_wav_materialized",
        "out_wav_bytes",
        "channel_mode",
        "channel_mode_requested",
        "runtime_mode",
        "runtime_mode_taxonomy",
        "runtime_mode_selector",
        "runtime_mode_status",
        "live_config",
        "lifecycle",
        "speaker_labels",
        "event_channels",
        "vad",
        "transcript",
        "readability_defaults",
        "transcript_per_channel",
        "terminal_summary",
        "first_emit_timing_ms",
        "queue_defer",
        "ordering_metadata",
        "events",
        "benchmark",
        "reconciliation",
        "asr_worker_pool",
        "chunk_queue",
        "cleanup_queue",
        "degradation_events",
        "trust",
        "event_counts",
        "session_summary",
        "jsonl_path",
    ];

    pub(crate) const SESSION_SUMMARY_KEYS: &[&str] = &[
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
    ];

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct RuntimeManifestHeaderModel {
        pub(crate) schema_version: String,
        pub(crate) kind: String,
        pub(crate) generated_at_utc: String,
        pub(crate) runtime_mode: String,
        pub(crate) runtime_mode_taxonomy: String,
        pub(crate) runtime_mode_selector: String,
        pub(crate) runtime_mode_status: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct RuntimeManifestArtifactsModel {
        pub(crate) out_wav: String,
        pub(crate) out_jsonl: String,
        pub(crate) out_manifest: String,
    }
}
