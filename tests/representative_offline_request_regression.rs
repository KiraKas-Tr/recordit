use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn transcribe_live_bin() -> PathBuf {
    if let Ok(path) = env::var("CARGO_BIN_EXE_transcribe-live") {
        return PathBuf::from(path);
    }
    project_root().join("target/debug/transcribe-live")
}

fn run_transcribe_live(args: &[String]) -> Output {
    Command::new(transcribe_live_bin())
        .args(args)
        .output()
        .expect("failed to execute transcribe-live")
}

fn parse_json(path: &Path) -> Value {
    let raw = fs::read_to_string(path).expect("failed to read JSON file");
    serde_json::from_str(&raw).expect("failed to parse JSON file")
}

fn parse_jsonl(path: &Path) -> Vec<Value> {
    let raw = fs::read_to_string(path).expect("failed to read JSONL file");
    raw.lines()
        .filter_map(|line| {
            if line.trim().is_empty() {
                return None;
            }
            Some(serde_json::from_str::<Value>(line).expect("failed to parse JSONL row"))
        })
        .collect()
}

fn temp_output_root(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("recordit-{prefix}-{nanos}"))
}

fn fixture_model_path() -> PathBuf {
    project_root().join("artifacts/bench/models/whispercpp/ggml-tiny.en.bin")
}

fn fixture_input_wav() -> PathBuf {
    project_root().join("artifacts/bench/corpus/gate_a/tts_phrase.wav")
}

fn run_representative_case(live_chunked: bool) -> (PathBuf, Value, Vec<Value>) {
    let output_root = temp_output_root(if live_chunked {
        "rep-chunked-regression"
    } else {
        "rep-offline-regression"
    });
    fs::create_dir_all(&output_root).expect("failed to create output root");

    let input_wav = fixture_input_wav();
    let model = fixture_model_path();
    assert!(
        input_wav.is_file(),
        "missing fixture input {}",
        input_wav.display()
    );
    assert!(model.is_file(), "missing fixture model {}", model.display());

    let out_wav = output_root.join("session.wav");
    let out_jsonl = output_root.join("session.runtime.jsonl");
    let out_manifest = output_root.join("session.manifest.json");

    let mut args = vec![
        "--input-wav".to_string(),
        input_wav.display().to_string(),
        "--asr-model".to_string(),
        model.display().to_string(),
        "--out-wav".to_string(),
        out_wav.display().to_string(),
        "--out-jsonl".to_string(),
        out_jsonl.display().to_string(),
        "--out-manifest".to_string(),
        out_manifest.display().to_string(),
        "--benchmark-runs".to_string(),
        "1".to_string(),
    ];
    if live_chunked {
        args.push("--live-chunked".to_string());
    }

    let output = run_transcribe_live(&args);
    assert!(
        output.status.success(),
        "transcribe-live run failed (live_chunked={}): stderr={}",
        live_chunked,
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        out_manifest.is_file(),
        "missing manifest {}",
        out_manifest.display()
    );
    assert!(out_jsonl.is_file(), "missing jsonl {}", out_jsonl.display());
    assert!(out_wav.is_file(), "missing out wav {}", out_wav.display());

    (
        output_root,
        parse_json(&out_manifest),
        parse_jsonl(&out_jsonl),
    )
}

fn final_text_by_channel(manifest: &Value) -> BTreeMap<String, String> {
    let mut finals = BTreeMap::new();
    let events = manifest
        .get("events")
        .and_then(Value::as_array)
        .expect("manifest.events should be an array");

    for event in events {
        let event_type = event
            .get("event_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type != "final" {
            continue;
        }
        let channel = event
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let text = event
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        finals.insert(channel, text);
    }

    finals
}

fn event_type_set(jsonl: &[Value]) -> std::collections::BTreeSet<String> {
    jsonl
        .iter()
        .filter_map(|row| row.get("event_type").and_then(Value::as_str))
        .map(|value| value.to_string())
        .collect()
}

#[test]
fn representative_modes_preserve_path_flow_finals_and_queue_contracts_after_request_migration() {
    let (_offline_root, offline_manifest, offline_jsonl) = run_representative_case(false);
    let (_chunked_root, chunked_manifest, chunked_jsonl) = run_representative_case(true);

    assert_eq!(
        offline_manifest
            .get("runtime_mode_taxonomy")
            .and_then(Value::as_str),
        Some("representative-offline")
    );
    assert_eq!(
        chunked_manifest
            .get("runtime_mode_taxonomy")
            .and_then(Value::as_str),
        Some("representative-chunked")
    );

    let offline_finals = final_text_by_channel(&offline_manifest);
    let chunked_finals = final_text_by_channel(&chunked_manifest);
    assert!(
        !offline_finals.is_empty(),
        "offline run should emit final events per channel"
    );
    assert_eq!(
        offline_finals.keys().collect::<Vec<_>>(),
        chunked_finals.keys().collect::<Vec<_>>(),
        "representative channel coverage drifted between offline and chunked modes"
    );
    for (channel, text) in &offline_finals {
        assert!(
            !text.trim().is_empty(),
            "offline final text should be non-empty for channel {channel}"
        );
    }
    for (channel, text) in &chunked_finals {
        assert!(
            !text.trim().is_empty(),
            "chunked final text should be non-empty for channel {channel}"
        );
    }

    for (label, manifest) in [
        ("offline", &offline_manifest),
        ("chunked", &chunked_manifest),
    ] {
        let asr_pool = manifest
            .get("asr_worker_pool")
            .and_then(Value::as_object)
            .expect("manifest.asr_worker_pool should be an object");
        let submitted = asr_pool
            .get("submitted")
            .and_then(Value::as_u64)
            .expect("asr_worker_pool.submitted should be numeric");
        let enqueued = asr_pool
            .get("enqueued")
            .and_then(Value::as_u64)
            .expect("asr_worker_pool.enqueued should be numeric");
        let failed = asr_pool
            .get("failed")
            .and_then(Value::as_u64)
            .expect("asr_worker_pool.failed should be numeric");
        let dropped = asr_pool
            .get("dropped_queue_full")
            .and_then(Value::as_u64)
            .expect("asr_worker_pool.dropped_queue_full should be numeric");

        assert!(
            submitted >= 2,
            "{label}: expected at least two ASR submissions"
        );
        assert_eq!(
            submitted, enqueued,
            "{label}: ASR enqueue drift indicates path-flow request incompatibility"
        );
        assert_eq!(
            failed, 0,
            "{label}: representative run should not fail ASR jobs"
        );
        assert_eq!(
            dropped, 0,
            "{label}: representative ASR queue should not drop submissions"
        );
    }

    let offline_chunk_queue_enabled = offline_manifest
        .get("chunk_queue")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("enabled"))
        .and_then(Value::as_bool)
        .expect("offline manifest chunk_queue.enabled should exist");
    let chunked_chunk_queue = chunked_manifest
        .get("chunk_queue")
        .and_then(Value::as_object)
        .expect("chunked manifest chunk_queue should be object");
    let chunked_enabled = chunked_chunk_queue
        .get("enabled")
        .and_then(Value::as_bool)
        .expect("chunked chunk_queue.enabled should exist");
    let chunked_submitted = chunked_chunk_queue
        .get("submitted")
        .and_then(Value::as_u64)
        .expect("chunked chunk_queue.submitted should be numeric");
    let chunked_processed = chunked_chunk_queue
        .get("processed")
        .and_then(Value::as_u64)
        .expect("chunked chunk_queue.processed should be numeric");
    assert!(!offline_chunk_queue_enabled);
    assert!(chunked_enabled);
    assert!(chunked_submitted >= chunked_processed);

    let offline_types = event_type_set(&offline_jsonl);
    let chunked_types = event_type_set(&chunked_jsonl);
    for required in ["asr_worker_pool", "chunk_queue", "cleanup_queue"] {
        assert!(
            offline_types.contains(required),
            "offline JSONL missing required diagnostics event `{required}`"
        );
        assert!(
            chunked_types.contains(required),
            "chunked JSONL missing required diagnostics event `{required}`"
        );
    }
}

#[test]
fn representative_offline_invalid_model_path_preserves_error_diagnostics() {
    let input_wav = fixture_input_wav();
    assert!(
        input_wav.is_file(),
        "missing fixture input {}",
        input_wav.display()
    );

    let missing_model = temp_output_root("rep-offline-missing-model").join("missing-model.bin");
    let output = run_transcribe_live(&[
        "--input-wav".to_string(),
        input_wav.display().to_string(),
        "--asr-model".to_string(),
        missing_model.display().to_string(),
        "--benchmark-runs".to_string(),
        "1".to_string(),
    ]);

    assert!(
        !output.status.success(),
        "invalid-model representative run should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("explicit `--asr-model` path does not exist"),
        "invalid-model failure should preserve explicit model-path diagnostics, stderr={stderr}"
    );
    assert!(
        stderr.contains(missing_model.to_string_lossy().as_ref()),
        "invalid-model failure should include failing model path in diagnostics, stderr={stderr}"
    );
}
