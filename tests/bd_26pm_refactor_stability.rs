use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const ENTRYPOINT_PATH: &str = "src/bin/transcribe_live.rs";
const APP_ENTRYPOINT_PATH: &str = "src/bin/transcribe_live/app.rs";
const MATRIX_CSV: &str = "artifacts/validation/bd-1qfx/matrix.csv";

#[derive(Debug, Clone)]
struct ScenarioExpectation {
    scenario: String,
    kind: String,
    runtime_mode: String,
    runtime_mode_taxonomy: String,
    runtime_mode_selector: String,
    runtime_mode_status: String,
    channel_mode_requested: String,
    channel_mode_active: String,
    jsonl_event_types: BTreeSet<String>,
    lifecycle_phases: Vec<String>,
    partial_count: usize,
    final_count: usize,
    llm_final_count: usize,
    reconciled_final_count: usize,
    trust_notice_count: usize,
    out_wav_materialized: bool,
    manifest_path: PathBuf,
    jsonl_path: PathBuf,
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_text(rel_path: &str) -> String {
    let path = project_root().join(rel_path);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

fn parse_bool(raw: &str) -> Result<bool, Box<dyn Error>> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("invalid bool `{raw}`").into()),
    }
}

fn parse_usize(raw: &str, field: &str) -> Result<usize, Box<dyn Error>> {
    raw.trim()
        .parse::<usize>()
        .map_err(|err| format!("invalid `{field}` value `{raw}`: {err}").into())
}

fn parse_pipe_set(raw: &str) -> BTreeSet<String> {
    raw.split('|')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_pipe_vec(raw: &str) -> Vec<String> {
    raw.split('|')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn load_matrix_expectations() -> Result<Vec<ScenarioExpectation>, Box<dyn Error>> {
    let csv_path = project_root().join(MATRIX_CSV);
    let csv = fs::read_to_string(&csv_path)?;

    let mut expectations = Vec::new();
    for (line_idx, line) in csv.lines().enumerate().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let fields = trimmed.split(',').collect::<Vec<_>>();
        if fields.len() != 23 {
            return Err(format!(
                "unexpected matrix.csv field count at line {}: expected 23, got {}",
                line_idx + 1,
                fields.len()
            )
            .into());
        }

        expectations.push(ScenarioExpectation {
            scenario: fields[0].to_owned(),
            kind: fields[1].to_owned(),
            runtime_mode: fields[2].to_owned(),
            runtime_mode_taxonomy: fields[3].to_owned(),
            runtime_mode_selector: fields[4].to_owned(),
            runtime_mode_status: fields[5].to_owned(),
            channel_mode_requested: fields[6].to_owned(),
            channel_mode_active: fields[7].to_owned(),
            jsonl_event_types: parse_pipe_set(fields[8]),
            lifecycle_phases: parse_pipe_vec(fields[9]),
            partial_count: parse_usize(fields[10], "partial_count")?,
            final_count: parse_usize(fields[11], "final_count")?,
            llm_final_count: parse_usize(fields[12], "llm_final_count")?,
            reconciled_final_count: parse_usize(fields[13], "reconciled_final_count")?,
            trust_notice_count: parse_usize(fields[14], "trust_notice_count")?,
            out_wav_materialized: parse_bool(fields[20])?,
            manifest_path: project_root().join(fields[21]),
            jsonl_path: project_root().join(fields[22]),
        });
    }

    Ok(expectations)
}

fn read_json(path: &Path) -> Value {
    let body = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed reading JSON {}: {err}", path.display()));
    serde_json::from_str(&body)
        .unwrap_or_else(|err| panic!("failed parsing JSON {}: {err}", path.display()))
}

fn extract_json_string_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let bytes = line.as_bytes();
    let mut index = line.find(&needle)? + needle.len();

    while index < bytes.len() && (bytes[index] as char).is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index)? != &b':' {
        return None;
    }
    index += 1;
    while index < bytes.len() && (bytes[index] as char).is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index)? != &b'"' {
        return None;
    }
    index += 1;

    let start = index;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            return Some(line[start..index].to_owned());
        }
        index += 1;
    }
    None
}

fn jsonl_event_types(path: &Path) -> BTreeSet<String> {
    let mut kinds = BTreeSet::new();
    for line in fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed reading JSONL {}: {err}", path.display()))
        .lines()
    {
        if let Some(event_type) = extract_json_string_field(line, "event_type") {
            kinds.insert(event_type);
        }
    }
    kinds
}

fn jsonl_event_count(path: &Path, event_type: &str) -> usize {
    let mut count = 0usize;
    for line in fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed reading JSONL {}: {err}", path.display()))
        .lines()
    {
        if extract_json_string_field(line, "event_type").as_deref() == Some(event_type) {
            count += 1;
        }
    }
    count
}

fn jsonl_lifecycle_sequence(path: &Path) -> Vec<String> {
    let mut phases = Vec::new();
    for line in fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed reading JSONL {}: {err}", path.display()))
        .lines()
    {
        if extract_json_string_field(line, "event_type").as_deref() != Some("lifecycle_phase") {
            continue;
        }
        let Some(phase) = extract_json_string_field(line, "phase") else {
            continue;
        };
        if phases.last() != Some(&phase) {
            phases.push(phase);
        }
    }
    phases
}

#[test]
fn extracted_runtime_boundaries_are_declared_and_delegated_from_entrypoint() {
    let root = project_root();
    for rel in [
        "src/bin/transcribe_live/cli_parse.rs",
        "src/bin/transcribe_live/artifacts.rs",
        "src/bin/transcribe_live/asr_backend.rs",
        "src/bin/transcribe_live/runtime_representative.rs",
        "src/bin/transcribe_live/runtime_live_stream.rs",
    ] {
        let path = root.join(rel);
        assert!(
            path.is_file(),
            "missing extracted boundary file {}",
            path.display()
        );
    }

    let entrypoint = read_text(ENTRYPOINT_PATH);
    let app_entrypoint = read_text(APP_ENTRYPOINT_PATH);

    for snippet in [
        "#[path = \"transcribe_live/app.rs\"]",
        "mod app;",
        "app::main()",
    ] {
        assert!(
            entrypoint.contains(snippet),
            "entrypoint missing expected thin-root snippet: {snippet}"
        );
    }

    for snippet in [
        "mod cli_parse;",
        "mod artifacts;",
        "mod asr_backend;",
        "mod runtime_representative;",
        "mod runtime_live_stream;",
        "runtime_representative::run_representative_offline_pipeline(config)",
        "runtime_representative::run_representative_chunked_pipeline(config)",
        "runtime_live_stream::run_live_stream_pipeline(config)",
    ] {
        assert!(
            app_entrypoint.contains(snippet),
            "entrypoint missing expected extracted-boundary delegation snippet: {snippet}"
        );
    }

    for legacy_symbol in [
        "struct LiveStreamRuntimeExecution",
        "struct LiveTerminalStream",
        "fn _run_standard_pipeline_moved",
    ] {
        assert!(
            !entrypoint.contains(legacy_symbol),
            "thin root entrypoint still contains pre-extraction symbol `{legacy_symbol}`"
        );
        assert!(
            !app_entrypoint.contains(legacy_symbol),
            "entrypoint still contains pre-extraction symbol `{legacy_symbol}`"
        );
    }
}

#[test]
fn frozen_baseline_semantics_hold_after_runtime_module_extraction() {
    let expectations =
        load_matrix_expectations().expect("failed to load frozen matrix expectations");
    assert!(
        !expectations.is_empty(),
        "matrix expectations should not be empty"
    );

    for expectation in expectations {
        assert!(
            expectation.manifest_path.is_file(),
            "{}: missing manifest {}",
            expectation.scenario,
            expectation.manifest_path.display()
        );
        assert!(
            expectation.jsonl_path.is_file(),
            "{}: missing jsonl {}",
            expectation.scenario,
            expectation.jsonl_path.display()
        );

        let manifest = read_json(&expectation.manifest_path);
        let object = manifest
            .as_object()
            .unwrap_or_else(|| panic!("{}: manifest must be a JSON object", expectation.scenario));

        assert_eq!(
            object.get("kind").and_then(Value::as_str),
            Some(expectation.kind.as_str()),
            "{}: kind drift",
            expectation.scenario
        );
        assert_eq!(
            object.get("runtime_mode").and_then(Value::as_str),
            Some(expectation.runtime_mode.as_str()),
            "{}: runtime_mode drift",
            expectation.scenario
        );
        assert_eq!(
            object.get("runtime_mode_taxonomy").and_then(Value::as_str),
            Some(expectation.runtime_mode_taxonomy.as_str()),
            "{}: runtime_mode_taxonomy drift",
            expectation.scenario
        );
        assert_eq!(
            object.get("runtime_mode_selector").and_then(Value::as_str),
            Some(expectation.runtime_mode_selector.as_str()),
            "{}: runtime_mode_selector drift",
            expectation.scenario
        );
        assert_eq!(
            object.get("runtime_mode_status").and_then(Value::as_str),
            Some(expectation.runtime_mode_status.as_str()),
            "{}: runtime_mode_status drift",
            expectation.scenario
        );
        assert_eq!(
            object.get("channel_mode_requested").and_then(Value::as_str),
            Some(expectation.channel_mode_requested.as_str()),
            "{}: channel_mode_requested drift",
            expectation.scenario
        );
        assert_eq!(
            object.get("channel_mode").and_then(Value::as_str),
            Some(expectation.channel_mode_active.as_str()),
            "{}: channel_mode (active) drift",
            expectation.scenario
        );
        assert_eq!(
            object.get("out_wav_materialized").and_then(Value::as_bool),
            Some(expectation.out_wav_materialized),
            "{}: out_wav_materialized drift",
            expectation.scenario
        );

        let actual_event_types = jsonl_event_types(&expectation.jsonl_path);
        assert_eq!(
            actual_event_types, expectation.jsonl_event_types,
            "{}: JSONL event-family drift",
            expectation.scenario
        );

        assert_eq!(
            jsonl_event_count(&expectation.jsonl_path, "partial"),
            expectation.partial_count,
            "{}: partial count drift",
            expectation.scenario
        );
        assert_eq!(
            jsonl_event_count(&expectation.jsonl_path, "final"),
            expectation.final_count,
            "{}: final count drift",
            expectation.scenario
        );
        assert_eq!(
            jsonl_event_count(&expectation.jsonl_path, "llm_final"),
            expectation.llm_final_count,
            "{}: llm_final count drift",
            expectation.scenario
        );
        assert_eq!(
            jsonl_event_count(&expectation.jsonl_path, "reconciled_final"),
            expectation.reconciled_final_count,
            "{}: reconciled_final count drift",
            expectation.scenario
        );
        assert_eq!(
            jsonl_event_count(&expectation.jsonl_path, "trust_notice"),
            expectation.trust_notice_count,
            "{}: trust_notice count drift",
            expectation.scenario
        );

        let lifecycle = jsonl_lifecycle_sequence(&expectation.jsonl_path);
        assert_eq!(
            lifecycle, expectation.lifecycle_phases,
            "{}: lifecycle sequence drift",
            expectation.scenario
        );
    }
}
