use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

fn read_json(path: &Path) -> Value {
    let raw = read_text(path);
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse {} as JSON: {err}", path.display()))
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    let raw = read_text(path);
    raw.lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            if line.trim().is_empty() {
                None
            } else {
                Some(serde_json::from_str::<Value>(line).unwrap_or_else(|err| {
                    panic!(
                        "failed to parse JSONL {} line {}: {err}",
                        path.display(),
                        idx + 1
                    )
                }))
            }
        })
        .collect()
}

fn collect_unique_lifecycle_phases(rows: &[Value]) -> Vec<String> {
    let mut phases = Vec::new();
    for row in rows {
        let Some(event_type) = row.get("event_type").and_then(Value::as_str) else {
            continue;
        };
        if event_type != "lifecycle_phase" {
            continue;
        }
        let Some(phase) = row.get("phase").and_then(Value::as_str) else {
            continue;
        };
        if !phases.iter().any(|existing| existing == phase) {
            phases.push(phase.to_string());
        }
    }
    phases
}

fn assert_expected_string(actual: Option<&Value>, expected: &Value, context: &str) {
    if expected.is_null() {
        assert!(
            actual.is_none() || actual.is_some_and(Value::is_null),
            "{context}: expected null/missing, got {actual:?}"
        );
        return;
    }

    let expected_str = expected
        .as_str()
        .unwrap_or_else(|| panic!("{context}: expected string or null"));
    let actual_str = actual
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{context}: expected present string"));
    assert_eq!(
        actual_str, expected_str,
        "{context}: string mismatch (actual={actual_str}, expected={expected_str})"
    );
}

#[test]
fn transcribe_live_entrypoint_keeps_delegation_seams() {
    let root = project_root();
    let entrypoint = read_text(&root.join("src/bin/transcribe_live.rs"));
    let app = read_text(&root.join("src/bin/transcribe_live/app.rs"));

    for required in [
        "#[path = \"transcribe_live/app.rs\"]",
        "mod app;",
        "app::main()",
    ] {
        assert!(
            entrypoint.contains(required),
            "entrypoint is missing module seam declaration: {required}"
        );
    }

    for forbidden in [
        "mod asr_backend;",
        "mod cli_parse;",
        "mod runtime_representative;",
        "mod runtime_live_stream;",
        "mod artifacts;",
    ] {
        assert!(
            !entrypoint.contains(forbidden),
            "thin root entrypoint should not contain implementation seam `{forbidden}`"
        );
    }

    for required in [
        "mod asr_backend;",
        "mod cli_parse;",
        "mod runtime_representative;",
        "mod runtime_live_stream;",
        "mod artifacts;",
    ] {
        assert!(
            app.contains(required),
            "app entrypoint is missing module seam declaration: {required}"
        );
    }

    for delegation in [
        "cli_parse::parse_args()",
        "cli_parse::parse_args_from(args)",
        "runtime_representative::run_representative_offline_pipeline(config)",
        "runtime_representative::run_representative_chunked_pipeline(config)",
        "runtime_live_stream::run_live_stream_pipeline(config)",
    ] {
        assert!(
            app.contains(delegation),
            "app delegation seam missing: {delegation}"
        );
    }

    for extracted_symbol in [
        "trait AsrAdapter",
        "struct AsrRequest<'a>",
        "fn resolve_backend_program(backend: AsrBackend, model_path: &Path) -> String",
        "fn run_standard_pipeline(config: &TranscribeConfig) -> Result<LiveRunReport, CliError>",
    ] {
        assert!(
            !app.contains(extracted_symbol),
            "entrypoint still contains extracted backend implementation symbol: {extracted_symbol}"
        );
    }
}

#[test]
fn extracted_module_files_expose_expected_boundaries() {
    let root = project_root();

    let asr_backend = read_text(&root.join("src/bin/transcribe_live/asr_backend.rs"));
    for needle in [
        "pub(super) fn resolve_backend_program",
        "pub(super) fn validate_model_path_for_backend",
        "pub(super) struct PooledAsrExecutor",
    ] {
        assert!(
            asr_backend.contains(needle),
            "asr_backend boundary missing expected symbol: {needle}"
        );
    }

    let cli_parse = read_text(&root.join("src/bin/transcribe_live/cli_parse.rs"));
    for needle in [
        "pub(super) fn parse_args()",
        "pub(super) fn parse_args_from",
        "config.validate()?",
    ] {
        assert!(
            cli_parse.contains(needle),
            "cli_parse boundary missing expected symbol: {needle}"
        );
    }

    let runtime_representative =
        read_text(&root.join("src/bin/transcribe_live/runtime_representative.rs"));
    for needle in [
        "pub(super) fn run_representative_offline_pipeline",
        "pub(super) fn run_representative_chunked_pipeline",
        "pub(super) fn run_standard_pipeline",
    ] {
        assert!(
            runtime_representative.contains(needle),
            "runtime_representative boundary missing expected symbol: {needle}"
        );
    }

    let runtime_live_stream =
        read_text(&root.join("src/bin/transcribe_live/runtime_live_stream.rs"));
    for needle in [
        "struct LiveStreamRuntimeExecution",
        "pub(super) fn run_live_stream_pipeline",
    ] {
        assert!(
            runtime_live_stream.contains(needle),
            "runtime_live_stream boundary missing expected symbol: {needle}"
        );
    }

    let artifacts = read_text(&root.join("src/bin/transcribe_live/artifacts.rs"));
    assert!(
        artifacts.contains("pub(super) fn write_runtime_jsonl"),
        "artifacts boundary missing write_runtime_jsonl"
    );
    assert!(
        artifacts.contains("pub(super) fn write_runtime_manifest"),
        "artifacts boundary missing write_runtime_manifest"
    );
}

#[test]
fn frozen_matrix_semantics_hold_after_modularization() {
    let root = project_root();
    let matrix = read_json(&root.join("artifacts/validation/bd-1qfx.golden-artifact-matrix.json"));

    let rows = matrix
        .get("matrix")
        .and_then(Value::as_array)
        .expect("matrix should contain array field `matrix`");
    assert!(!rows.is_empty(), "frozen matrix rows should not be empty");

    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .expect("row must include id");
        let expected = row
            .get("expected")
            .expect("row must include expected object");

        let manifest_rel = row
            .get("manifest_path")
            .and_then(Value::as_str)
            .expect("row must include manifest_path");
        let jsonl_rel = row
            .get("jsonl_path")
            .and_then(Value::as_str)
            .expect("row must include jsonl_path");

        let manifest = read_json(&root.join(manifest_rel));
        let jsonl_rows = read_jsonl(&root.join(jsonl_rel));

        assert_expected_string(
            manifest.get("runtime_mode"),
            expected
                .get("runtime_mode")
                .expect("expected.runtime_mode missing"),
            &format!("{id}: runtime_mode"),
        );
        assert_expected_string(
            manifest.get("runtime_mode_taxonomy"),
            expected
                .get("runtime_mode_taxonomy")
                .expect("expected.runtime_mode_taxonomy missing"),
            &format!("{id}: runtime_mode_taxonomy"),
        );
        assert_expected_string(
            manifest.get("runtime_mode_selector"),
            expected
                .get("runtime_mode_selector")
                .expect("expected.runtime_mode_selector missing"),
            &format!("{id}: runtime_mode_selector"),
        );

        let expected_lifecycle: Vec<String> = expected
            .get("lifecycle_phases")
            .and_then(Value::as_array)
            .expect("expected.lifecycle_phases missing")
            .iter()
            .map(|phase| {
                phase
                    .as_str()
                    .unwrap_or_else(|| panic!("{id}: lifecycle phase should be string"))
                    .to_string()
            })
            .collect();
        let actual_lifecycle = collect_unique_lifecycle_phases(&jsonl_rows);
        assert_eq!(
            actual_lifecycle, expected_lifecycle,
            "{id}: lifecycle phase order drift"
        );

        let expected_event_types: BTreeSet<String> = expected
            .get("jsonl_event_types")
            .and_then(Value::as_array)
            .expect("expected.jsonl_event_types missing")
            .iter()
            .map(|event| {
                event
                    .as_str()
                    .unwrap_or_else(|| panic!("{id}: jsonl event type should be string"))
                    .to_string()
            })
            .collect();
        let actual_event_types: BTreeSet<String> = jsonl_rows
            .iter()
            .filter_map(|row| row.get("event_type").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        assert_eq!(
            actual_event_types, expected_event_types,
            "{id}: JSONL event family drift"
        );
    }
}
