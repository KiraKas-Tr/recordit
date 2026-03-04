use std::collections::{BTreeSet, HashSet};
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
        .unwrap_or_else(|err| panic!("failed to parse JSON file {}: {err}", path.display()))
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    let raw = read_text(path);
    raw.lines()
        .enumerate()
        .filter_map(|(line_no, line)| {
            if line.trim().is_empty() {
                return None;
            }
            Some(serde_json::from_str::<Value>(line).unwrap_or_else(|err| {
                panic!(
                    "failed to parse JSONL {} line {}: {err}",
                    path.display(),
                    line_no + 1
                )
            }))
        })
        .collect()
}

fn require_strings(value: &Value, context: &str) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{context}: expected array"))
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .unwrap_or_else(|| panic!("{context}: expected string entries"))
                .to_string()
        })
        .collect()
}

#[test]
fn transcribe_live_declares_expected_module_seams() {
    let root = project_root();
    let transcribe_live = read_text(&root.join("src/bin/transcribe_live.rs"));
    let transcribe_live_app = read_text(&root.join("src/bin/transcribe_live/app.rs"));

    let expected_root_decls = [
        "#[path = \"transcribe_live/app.rs\"]",
        "mod app;",
        "app::main()",
    ];

    for decl in expected_root_decls {
        assert!(
            transcribe_live.contains(decl),
            "missing thin-entrypoint declaration in transcribe_live.rs: {decl}"
        );
    }

    let expected_app_decls = [
        "mod cli_parse;",
        "mod asr_backend;",
        "mod artifacts;",
        "mod cleanup;",
        "mod preflight;",
        "mod reporting;",
        "mod reconciliation;",
        "mod runtime_representative;",
        "mod runtime_live_stream;",
    ];

    for decl in expected_app_decls {
        assert!(
            transcribe_live_app.contains(decl),
            "missing module declaration in transcribe_live/app.rs: {decl}"
        );
    }

    for module_file in [
        "src/bin/transcribe_live/cli_parse.rs",
        "src/bin/transcribe_live/asr_backend.rs",
        "src/bin/transcribe_live/artifacts.rs",
        "src/bin/transcribe_live/cleanup.rs",
        "src/bin/transcribe_live/preflight.rs",
        "src/bin/transcribe_live/reporting.rs",
        "src/bin/transcribe_live/reconciliation.rs",
        "src/bin/transcribe_live/runtime_representative.rs",
        "src/bin/transcribe_live/runtime_live_stream.rs",
    ] {
        assert!(
            root.join(module_file).is_file(),
            "expected extracted module file to exist: {module_file}"
        );
    }
}

#[test]
fn transcribe_live_keeps_thin_wrapper_delegation_to_extracted_modules() {
    let root = project_root();
    let transcribe_live = read_text(&root.join("src/bin/transcribe_live/app.rs"));

    let expected_calls = [
        "cli_parse::parse_args()",
        "cli_parse::parse_args_from(args)",
        "runtime_representative::run_representative_offline_pipeline(config)",
        "runtime_representative::run_representative_chunked_pipeline(config)",
        "runtime_live_stream::run_live_stream_pipeline(config)",
        "cleanup::run_cleanup_queue(config, events)",
        "cleanup::run_cleanup_queue_with(config, events, invoke_cleanup)",
        "cleanup::cleanup_content_from_response(stdout)",
        "reconciliation::build_targeted_reconciliation_events(",
        "reconciliation::build_reconciliation_matrix(vad_boundaries, degradation_events)",
        "reporting::print_live_report(config, report, concise_only)",
        "artifacts::write_runtime_jsonl(config, report)",
        "artifacts::write_runtime_manifest(config, report)",
        "artifacts::write_preflight_manifest(config, report)",
    ];

    for call in expected_calls {
        assert!(
            transcribe_live.contains(call),
            "expected delegation call missing from transcribe_live/app.rs: {call}"
        );
    }
}

#[test]
fn extracted_modules_expose_expected_entrypoints() {
    let root = project_root();

    let asr_backend = read_text(&root.join("src/bin/transcribe_live/asr_backend.rs"));
    for symbol in [
        "pub(super) fn resolve_backend_program",
        "pub(super) fn validate_model_path_for_backend",
        "pub(super) fn resolve_model_path",
    ] {
        assert!(
            asr_backend.contains(symbol),
            "missing asr backend entrypoint: {symbol}"
        );
    }

    let artifacts = read_text(&root.join("src/bin/transcribe_live/artifacts.rs"));
    for symbol in [
        "pub(super) fn write_runtime_jsonl",
        "pub(super) fn write_runtime_manifest",
        "pub(super) fn write_preflight_manifest",
    ] {
        assert!(
            artifacts.contains(symbol),
            "missing artifacts entrypoint: {symbol}"
        );
    }

    let representative = read_text(&root.join("src/bin/transcribe_live/runtime_representative.rs"));
    assert!(
        representative.contains("pub(super) fn run_standard_pipeline"),
        "missing representative runtime entrypoint"
    );

    let live_stream = read_text(&root.join("src/bin/transcribe_live/runtime_live_stream.rs"));
    assert!(
        live_stream.contains("pub(super) fn run_live_stream_pipeline"),
        "missing live-stream runtime entrypoint"
    );

    let preflight = read_text(&root.join("src/bin/transcribe_live/preflight.rs"));
    for symbol in [
        "pub(super) fn run_preflight",
        "pub(super) fn run_model_doctor",
        "pub(super) fn print_preflight_report",
        "pub(super) fn print_model_doctor_report",
    ] {
        assert!(
            preflight.contains(symbol),
            "missing preflight entrypoint: {symbol}"
        );
    }

    let cleanup = read_text(&root.join("src/bin/transcribe_live/cleanup.rs"));
    for symbol in [
        "pub(super) fn run_cleanup_queue",
        "pub(super) fn run_cleanup_queue_with",
        "pub(super) fn cleanup_content_from_response",
    ] {
        assert!(
            cleanup.contains(symbol),
            "missing cleanup entrypoint: {symbol}"
        );
    }

    let reporting = read_text(&root.join("src/bin/transcribe_live/reporting.rs"));
    for symbol in [
        "pub(super) fn runtime_failure_breadcrumbs",
        "pub(super) fn top_remediation_hints",
        "pub(super) fn remediation_hints_csv",
        "pub(super) fn build_live_close_summary_lines",
        "pub(super) fn print_live_report",
    ] {
        assert!(
            reporting.contains(symbol),
            "missing reporting entrypoint: {symbol}"
        );
    }

    let reconciliation = read_text(&root.join("src/bin/transcribe_live/reconciliation.rs"));
    for symbol in [
        "pub(super) fn build_reconciliation_events",
        "pub(super) fn build_targeted_reconciliation_events",
        "pub(super) fn build_reconciliation_matrix",
    ] {
        assert!(
            reconciliation.contains(symbol),
            "missing reconciliation entrypoint: {symbol}"
        );
    }
}

#[test]
fn frozen_baseline_matrix_semantics_hold_after_modularization() {
    let root = project_root();
    let matrix = read_json(&root.join("artifacts/validation/bd-1qfx.golden-artifact-matrix.json"));
    let rows = matrix
        .get("matrix")
        .and_then(Value::as_array)
        .expect("matrix should contain a `matrix` array");
    assert!(!rows.is_empty(), "baseline matrix should not be empty");

    for row in rows {
        let row_id = row
            .get("id")
            .and_then(Value::as_str)
            .expect("row.id is required");
        let expected = row.get("expected").expect("row.expected is required");

        let manifest_path = root.join(
            row.get("manifest_path")
                .and_then(Value::as_str)
                .expect("row.manifest_path is required"),
        );
        let jsonl_path = root.join(
            row.get("jsonl_path")
                .and_then(Value::as_str)
                .expect("row.jsonl_path is required"),
        );

        assert!(
            manifest_path.is_file(),
            "{row_id}: missing manifest fixture"
        );
        assert!(jsonl_path.is_file(), "{row_id}: missing JSONL fixture");

        let manifest = read_json(&manifest_path);
        let jsonl_rows = read_jsonl(&jsonl_path);

        for key in [
            "runtime_mode",
            "runtime_mode_taxonomy",
            "runtime_mode_selector",
        ] {
            let expected_value = expected.get(key).and_then(Value::as_str);
            let actual_value = manifest.get(key).and_then(Value::as_str);
            match expected_value {
                Some(expected_str) => {
                    let actual = actual_value
                        .unwrap_or_else(|| panic!("{row_id}: manifest missing `{key}`"));
                    assert_eq!(
                        actual, expected_str,
                        "{row_id}: mismatch for manifest.{key}"
                    );
                }
                None => {
                    assert!(
                        actual_value.is_none(),
                        "{row_id}: expected no `{key}` in manifest but found {:?}",
                        actual_value
                    );
                }
            }
        }

        let expected_event_types = require_strings(
            expected
                .get("jsonl_event_types")
                .expect("expected.jsonl_event_types missing"),
            &format!("{row_id}: expected.jsonl_event_types"),
        )
        .into_iter()
        .collect::<BTreeSet<_>>();

        let mut actual_event_types = BTreeSet::new();
        let mut lifecycle = Vec::new();
        let mut seen_lifecycle = HashSet::new();

        for row in &jsonl_rows {
            if let Some(event_type) = row.get("event_type").and_then(Value::as_str) {
                actual_event_types.insert(event_type.to_string());
                if event_type == "lifecycle_phase"
                    && let Some(phase) = row.get("phase").and_then(Value::as_str)
                    && seen_lifecycle.insert(phase.to_string())
                {
                    lifecycle.push(phase.to_string());
                }
            }
        }

        assert_eq!(
            actual_event_types, expected_event_types,
            "{row_id}: JSONL event family drift"
        );

        let expected_lifecycle = require_strings(
            expected
                .get("lifecycle_phases")
                .expect("expected.lifecycle_phases missing"),
            &format!("{row_id}: expected.lifecycle_phases"),
        );
        assert_eq!(
            lifecycle, expected_lifecycle,
            "{row_id}: lifecycle order drift"
        );
    }
}
