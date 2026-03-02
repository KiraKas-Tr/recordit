use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(path: &Path) -> Value {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read JSON file {}: {err}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse JSON file {}: {err}", path.display()))
}

fn require_string_vec(value: &Value, context: &str) -> Vec<String> {
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

fn assert_opt_str(actual: Option<&Value>, expected: &Value, context: &str) {
    if expected.is_null() {
        assert!(
            actual.is_none() || actual.is_some_and(Value::is_null),
            "{context}: expected null/missing, got {:?}",
            actual
        );
        return;
    }

    let expected_str = expected
        .as_str()
        .unwrap_or_else(|| panic!("{context}: expected string or null"));
    let actual_str = actual
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{context}: expected present string, got {:?}", actual));
    assert_eq!(
        actual_str, expected_str,
        "{context}: string mismatch (actual={actual_str}, expected={expected_str})"
    );
}

fn assert_opt_u64(actual: Option<&Value>, expected: &Value, context: &str) {
    if expected.is_null() {
        assert!(
            actual.is_none() || actual.is_some_and(Value::is_null),
            "{context}: expected null/missing, got {:?}",
            actual
        );
        return;
    }

    let expected_u64 = expected
        .as_u64()
        .unwrap_or_else(|| panic!("{context}: expected u64 or null"));
    let actual_u64 = actual
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("{context}: expected present u64, got {:?}", actual));
    assert_eq!(
        actual_u64, expected_u64,
        "{context}: numeric mismatch (actual={actual_u64}, expected={expected_u64})"
    );
}

fn manifest_codes(manifest: &Value, path: &[&str], field: &str) -> Vec<String> {
    let notices = path
        .iter()
        .fold(Some(manifest), |current, segment| current?.get(segment))
        .and_then(Value::as_array);
    let entries: &[Value] = notices.map(Vec::as_slice).unwrap_or(&[]);
    let mut codes: Vec<String> = entries
        .iter()
        .filter_map(|entry| entry.get(field).and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    codes.sort();
    codes
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read JSONL {}: {err}", path.display()));
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

#[test]
fn frozen_matrix_rows_match_committed_artifacts() {
    let root = project_root();
    let matrix_path = root.join("artifacts/validation/bd-1qfx.golden-artifact-matrix.json");
    let matrix = read_json(&matrix_path);

    let rows = matrix
        .get("matrix")
        .and_then(Value::as_array)
        .expect("matrix should contain a non-empty `matrix` array");
    assert!(!rows.is_empty(), "baseline matrix is empty");

    for row in rows {
        let row_id = row
            .get("id")
            .and_then(Value::as_str)
            .expect("row must contain id");
        let manifest_rel = row
            .get("manifest_path")
            .and_then(Value::as_str)
            .expect("row must contain manifest_path");
        let jsonl_rel = row
            .get("jsonl_path")
            .and_then(Value::as_str)
            .expect("row must contain jsonl_path");

        let manifest_path = root.join(manifest_rel);
        let jsonl_path = root.join(jsonl_rel);
        assert!(
            manifest_path.is_file(),
            "{row_id}: missing manifest {}",
            manifest_path.display()
        );
        assert!(
            jsonl_path.is_file(),
            "{row_id}: missing jsonl {}",
            jsonl_path.display()
        );

        let expected = row
            .get("expected")
            .expect("row must contain expected object");
        let manifest = read_json(&manifest_path);
        let jsonl_rows = read_jsonl(&jsonl_path);

        assert_opt_str(
            manifest.get("runtime_mode"),
            expected
                .get("runtime_mode")
                .expect("expected.runtime_mode missing"),
            &format!("{row_id}: runtime_mode"),
        );
        assert_opt_str(
            manifest.get("runtime_mode_taxonomy"),
            expected
                .get("runtime_mode_taxonomy")
                .expect("expected.runtime_mode_taxonomy missing"),
            &format!("{row_id}: runtime_mode_taxonomy"),
        );
        assert_opt_str(
            manifest.get("runtime_mode_selector"),
            expected
                .get("runtime_mode_selector")
                .expect("expected.runtime_mode_selector missing"),
            &format!("{row_id}: runtime_mode_selector"),
        );
        assert_opt_str(
            manifest
                .get("session_summary")
                .and_then(|summary| summary.get("session_status")),
            expected
                .get("session_status")
                .expect("expected.session_status missing"),
            &format!("{row_id}: session_status"),
        );
        assert_opt_u64(
            manifest
                .get("first_emit_timing_ms")
                .and_then(|timing| timing.get("first_stable")),
            expected
                .get("first_stable_timing_ms")
                .expect("expected.first_stable_timing_ms missing"),
            &format!("{row_id}: first_stable_timing_ms"),
        );

        let expected_event_counts = expected
            .get("event_counts")
            .and_then(Value::as_object)
            .expect("expected.event_counts must be object");
        for (key, expected_value) in expected_event_counts {
            let actual_value = manifest
                .get("event_counts")
                .and_then(|counts| counts.get(key))
                .and_then(Value::as_u64)
                .unwrap_or_else(|| panic!("{row_id}: missing manifest.event_counts.{key}"));
            let expected_u64 = expected_value
                .as_u64()
                .unwrap_or_else(|| panic!("{row_id}: expected event_count {key} must be u64"));
            assert_eq!(
                actual_value, expected_u64,
                "{row_id}: event count mismatch for {key}"
            );
        }

        let mut expected_trust_codes = require_string_vec(
            expected
                .get("trust_codes")
                .expect("expected.trust_codes missing"),
            &format!("{row_id}: expected.trust_codes"),
        );
        expected_trust_codes.sort();
        let actual_trust_codes = manifest_codes(&manifest, &["trust", "notices"], "code");
        assert_eq!(
            actual_trust_codes, expected_trust_codes,
            "{row_id}: trust code mismatch"
        );

        let mut expected_degradation_codes = require_string_vec(
            expected
                .get("degradation_codes")
                .expect("expected.degradation_codes missing"),
            &format!("{row_id}: expected.degradation_codes"),
        );
        expected_degradation_codes.sort();
        let actual_degradation_codes = manifest_codes(&manifest, &["degradation_events"], "code");
        assert_eq!(
            actual_degradation_codes, expected_degradation_codes,
            "{row_id}: degradation code mismatch"
        );

        let mut actual_event_types = BTreeSet::new();
        let mut actual_lifecycle = Vec::new();
        let mut seen_lifecycle = HashSet::new();

        for event in &jsonl_rows {
            if let Some(event_type) = event.get("event_type").and_then(Value::as_str) {
                actual_event_types.insert(event_type.to_string());
                if event_type == "lifecycle_phase"
                    && let Some(phase) = event.get("phase").and_then(Value::as_str)
                    && seen_lifecycle.insert(phase.to_string())
                {
                    actual_lifecycle.push(phase.to_string());
                }
            }
        }

        let expected_event_types = require_string_vec(
            expected
                .get("jsonl_event_types")
                .expect("expected.jsonl_event_types missing"),
            &format!("{row_id}: expected.jsonl_event_types"),
        )
        .into_iter()
        .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_event_types, expected_event_types,
            "{row_id}: JSONL event-type family mismatch"
        );

        let expected_lifecycle = require_string_vec(
            expected
                .get("lifecycle_phases")
                .expect("expected.lifecycle_phases missing"),
            &format!("{row_id}: expected.lifecycle_phases"),
        );
        assert_eq!(
            actual_lifecycle, expected_lifecycle,
            "{row_id}: lifecycle-phase sequence mismatch"
        );
    }
}

#[test]
fn trust_and_degradation_code_examples_remain_available() {
    let root = project_root();
    let matrix_path = root.join("artifacts/validation/bd-1qfx.golden-artifact-matrix.json");
    let matrix = read_json(&matrix_path);

    let examples = matrix
        .get("trust_degradation_code_examples")
        .and_then(Value::as_array)
        .expect("matrix must include trust_degradation_code_examples");
    assert!(!examples.is_empty(), "trust/degradation examples are empty");

    for example in examples {
        let id = example
            .get("id")
            .and_then(Value::as_str)
            .expect("example must include id");
        let manifest_rel = example
            .get("manifest_path")
            .and_then(Value::as_str)
            .expect("example must include manifest_path");
        let jsonl_rel = example
            .get("jsonl_path")
            .and_then(Value::as_str)
            .expect("example must include jsonl_path");

        let manifest_path = root.join(manifest_rel);
        let jsonl_path = root.join(jsonl_rel);
        assert!(
            manifest_path.is_file(),
            "{id}: missing manifest {}",
            manifest_path.display()
        );
        assert!(
            jsonl_path.is_file(),
            "{id}: missing jsonl {}",
            jsonl_path.display()
        );

        let manifest = read_json(&manifest_path);
        let mut expected_trust = require_string_vec(
            example
                .get("trust_codes")
                .expect("example.trust_codes missing"),
            &format!("{id}: expected trust_codes"),
        );
        expected_trust.sort();
        let mut expected_degradation = require_string_vec(
            example
                .get("degradation_codes")
                .expect("example.degradation_codes missing"),
            &format!("{id}: expected degradation_codes"),
        );
        expected_degradation.sort();

        let actual_trust = manifest_codes(&manifest, &["trust", "notices"], "code");
        let actual_degradation = manifest_codes(&manifest, &["degradation_events"], "code");
        assert_eq!(actual_trust, expected_trust, "{id}: trust code mismatch");
        assert_eq!(
            actual_degradation, expected_degradation,
            "{id}: degradation code mismatch"
        );

        let supplemental = example
            .get("note")
            .and_then(Value::as_str)
            .map(|note| note.contains("Supplemental"))
            .unwrap_or(false);
        if supplemental {
            continue;
        }

        let jsonl_rows = read_jsonl(&jsonl_path);
        let actual_jsonl_codes: BTreeSet<String> = jsonl_rows
            .iter()
            .filter_map(|event| {
                let event_type = event.get("event_type").and_then(Value::as_str)?;
                if event_type == "trust_notice" || event_type == "mode_degradation" {
                    event
                        .get("code")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .collect();
        let expected_jsonl_codes: BTreeSet<String> = expected_trust
            .into_iter()
            .chain(expected_degradation.into_iter())
            .collect();
        assert_eq!(
            actual_jsonl_codes, expected_jsonl_codes,
            "{id}: JSONL trust/degradation code mismatch"
        );
    }
}
