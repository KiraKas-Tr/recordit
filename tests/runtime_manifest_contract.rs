use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

#[derive(Clone, Copy)]
struct ManifestBaseline {
    name: &'static str,
    rel_path: &'static str,
}

const MANIFEST_BASELINES: &[ManifestBaseline] = &[
    ManifestBaseline {
        name: "representative-offline",
        rel_path: "artifacts/validation/bd-1qfx/representative-offline.runtime.manifest.json",
    },
    ManifestBaseline {
        name: "representative-chunked",
        rel_path: "artifacts/validation/bd-1qfx/representative-chunked.runtime.manifest.json",
    },
    ManifestBaseline {
        name: "live-stream-cold",
        rel_path: "artifacts/validation/bd-1qfx/live-stream-cold.runtime.manifest.json",
    },
    ManifestBaseline {
        name: "live-stream-warm",
        rel_path: "artifacts/validation/bd-1qfx/live-stream-warm.runtime.manifest.json",
    },
    ManifestBaseline {
        name: "live-stream-backlog",
        rel_path: "artifacts/validation/bd-1qfx/live-stream-backlog.runtime.manifest.json",
    },
];

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn baseline_path(rel_path: &str) -> PathBuf {
    project_root().join(rel_path)
}

fn parse_manifest(path: &Path) -> Value {
    let body = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed reading manifest {}: {err}", path.display()));
    serde_json::from_str::<Value>(&body)
        .unwrap_or_else(|err| panic!("failed parsing manifest {}: {err}", path.display()))
}

fn object_field<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> &'a Map<String, Value> {
    obj.get(key)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{context}: missing object field `{key}`"))
}

fn bool_field(obj: &Map<String, Value>, key: &str, context: &str) -> bool {
    obj.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("{context}: missing bool field `{key}`"))
}

fn int_field(obj: &Map<String, Value>, key: &str, context: &str) -> i64 {
    obj.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("{context}: missing integer field `{key}`"))
}

fn str_field<'a>(obj: &'a Map<String, Value>, key: &str, context: &str) -> &'a str {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{context}: missing string field `{key}`"))
}

fn normalize_artifact_path(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        project_root().join(path)
    }
}

fn assert_keys(obj: &Map<String, Value>, keys: &[&str], context: &str) {
    for key in keys {
        assert!(
            obj.contains_key(*key),
            "{context}: expected key `{key}` to exist"
        );
    }
}

#[test]
fn runtime_manifest_baselines_preserve_stable_top_level_and_nested_keys() {
    let top_level_required = [
        "kind",
        "generated_at_utc",
        "input_wav",
        "out_wav",
        "out_wav_semantics",
        "out_wav_materialized",
        "out_wav_bytes",
        "channel_mode_requested",
        "channel_mode",
        "runtime_mode",
        "runtime_mode_taxonomy",
        "runtime_mode_selector",
        "runtime_mode_status",
        "lifecycle",
        "terminal_summary",
        "first_emit_timing_ms",
        "event_counts",
        "session_summary",
        "trust",
        "degradation_events",
        "reconciliation",
        "asr_worker_pool",
        "chunk_queue",
        "cleanup_queue",
        "events",
        "jsonl_path",
    ];

    let terminal_summary_required = [
        "live_mode",
        "render_mode",
        "stable_line_policy",
        "stable_line_count",
        "stable_lines_replayed",
        "stable_lines",
    ];

    let first_emit_required = ["first_any", "first_partial", "first_final", "first_stable"];

    let session_summary_required = [
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

    let trust_required = ["degraded_mode_active", "notice_count", "notices"];

    for baseline in MANIFEST_BASELINES {
        let path = baseline_path(baseline.rel_path);
        assert!(
            path.is_file(),
            "missing baseline manifest {}",
            path.display()
        );
        let value = parse_manifest(&path);
        let manifest = value
            .as_object()
            .unwrap_or_else(|| panic!("manifest is not object: {}", path.display()));

        let context = format!("baseline={}", baseline.name);
        assert_keys(manifest, &top_level_required, &context);

        let terminal_summary = object_field(manifest, "terminal_summary", &context);
        assert_keys(terminal_summary, &terminal_summary_required, &context);

        let first_emit = object_field(manifest, "first_emit_timing_ms", &context);
        assert_keys(first_emit, &first_emit_required, &context);

        let session_summary = object_field(manifest, "session_summary", &context);
        assert_keys(session_summary, &session_summary_required, &context);

        let trust = object_field(manifest, "trust", &context);
        assert_keys(trust, &trust_required, &context);

        assert!(
            manifest
                .get("degradation_events")
                .and_then(Value::as_array)
                .is_some(),
            "{context}: expected `degradation_events` array"
        );
        assert!(
            manifest.get("events").and_then(Value::as_array).is_some(),
            "{context}: expected `events` array"
        );
    }
}

#[test]
fn runtime_manifest_baselines_preserve_artifact_truth_semantics() {
    for baseline in MANIFEST_BASELINES {
        let path = baseline_path(baseline.rel_path);
        let value = parse_manifest(&path);
        let manifest = value
            .as_object()
            .unwrap_or_else(|| panic!("manifest is not object: {}", path.display()));

        let context = format!("artifact_truth:{}", baseline.name);
        assert_eq!(
            str_field(manifest, "kind", &context),
            "transcribe-live-runtime",
            "{context}: kind must remain transcribe-live-runtime"
        );
        assert!(
            bool_field(manifest, "out_wav_materialized", &context),
            "{context}: expected out_wav_materialized=true"
        );
        assert!(
            int_field(manifest, "out_wav_bytes", &context) > 0,
            "{context}: expected out_wav_bytes>0"
        );

        let out_wav = normalize_artifact_path(str_field(manifest, "out_wav", &context));
        assert!(
            out_wav.is_file(),
            "{context}: out_wav path does not exist: {}",
            out_wav.display()
        );
        let out_wav_size = fs::metadata(&out_wav)
            .unwrap_or_else(|err| {
                panic!(
                    "{context}: failed stat out_wav {}: {err}",
                    out_wav.display()
                )
            })
            .len();
        assert!(out_wav_size > 0, "{context}: out_wav file is empty");

        let jsonl_path = normalize_artifact_path(str_field(manifest, "jsonl_path", &context));
        assert!(
            jsonl_path.is_file(),
            "{context}: jsonl_path does not exist: {}",
            jsonl_path.display()
        );

        let session_summary = object_field(manifest, "session_summary", &context);
        let artifacts = object_field(session_summary, "artifacts", &context);
        let session_out_wav = normalize_artifact_path(str_field(artifacts, "out_wav", &context));
        let session_out_jsonl =
            normalize_artifact_path(str_field(artifacts, "out_jsonl", &context));
        let session_out_manifest =
            normalize_artifact_path(str_field(artifacts, "out_manifest", &context));

        assert_eq!(
            session_out_wav, out_wav,
            "{context}: session_summary.artifacts.out_wav must match top-level out_wav"
        );
        assert_eq!(
            session_out_jsonl, jsonl_path,
            "{context}: session_summary.artifacts.out_jsonl must match top-level jsonl_path"
        );
        assert!(
            session_out_manifest.is_file(),
            "{context}: session_summary.artifacts.out_manifest does not exist: {}",
            session_out_manifest.display()
        );
        let linked_manifest = parse_manifest(&session_out_manifest);
        let linked_obj = linked_manifest.as_object().unwrap_or_else(|| {
            panic!(
                "{context}: session_summary.artifacts.out_manifest is not object JSON: {}",
                session_out_manifest.display()
            )
        });
        assert_eq!(
            str_field(linked_obj, "kind", &context),
            "transcribe-live-runtime",
            "{context}: linked out_manifest kind drifted"
        );
    }
}

#[test]
fn representative_chunked_baseline_preserves_degraded_trust_and_reconciliation_shape() {
    let path =
        baseline_path("artifacts/validation/bd-1qfx/representative-chunked.runtime.manifest.json");
    let value = parse_manifest(&path);
    let manifest = value
        .as_object()
        .unwrap_or_else(|| panic!("manifest is not object: {}", path.display()));
    let context = "representative-chunked-degraded-shape";

    assert_eq!(
        str_field(manifest, "runtime_mode", context),
        "live-chunked",
        "runtime_mode compatibility label drifted"
    );
    assert_eq!(
        str_field(manifest, "runtime_mode_taxonomy", context),
        "representative-chunked",
        "runtime_mode_taxonomy drifted"
    );
    assert_eq!(
        str_field(manifest, "runtime_mode_selector", context),
        "--live-chunked",
        "runtime_mode_selector drifted"
    );
    assert_eq!(
        str_field(manifest, "runtime_mode_status", context),
        "implemented",
        "runtime_mode_status drifted"
    );

    let trust = object_field(manifest, "trust", context);
    let trust_notices = trust
        .get("notices")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{context}: trust.notices missing"));
    let trust_notice_count = int_field(trust, "notice_count", context);
    assert_eq!(
        trust_notice_count as usize,
        trust_notices.len(),
        "{context}: trust.notice_count must match trust.notices length"
    );
    assert!(
        trust_notice_count > 0,
        "{context}: expected degraded representative-chunked trust notices"
    );

    let degradation_events = manifest
        .get("degradation_events")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{context}: degradation_events missing"));
    assert!(
        !degradation_events.is_empty(),
        "{context}: expected degraded representative-chunked degradation events"
    );

    let reconciliation = object_field(manifest, "reconciliation", context);
    assert!(
        bool_field(reconciliation, "required", context),
        "{context}: expected reconciliation.required=true"
    );
    assert!(
        bool_field(reconciliation, "applied", context),
        "{context}: expected reconciliation.applied=true"
    );
}
