use std::collections::{BTreeSet, HashMap};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MATRIX_CSV: &str = "artifacts/validation/bd-1qfx/matrix.csv";
const DEFAULT_DURATION_SEC: u64 = 3;
const LIVE_CHUNK_WINDOW_MS: u64 = 2_000;
const LIVE_CHUNK_STRIDE_MS: u64 = 500;
const LIVE_CHUNK_QUEUE_CAP: u64 = 4;

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
    partial_count: u64,
    final_count: u64,
    llm_final_count: u64,
    reconciled_final_count: u64,
    trust_notice_count: u64,
    trust_codes: BTreeSet<String>,
    degradation_event_count: u64,
    degradation_codes: BTreeSet<String>,
    reconciliation_required: bool,
    reconciliation_applied: bool,
    out_wav_materialized: bool,
    manifest_path: PathBuf,
    jsonl_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
enum RuntimeMode {
    RepresentativeOffline,
    RepresentativeChunked,
    LiveStream,
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn matrix_csv_path() -> PathBuf {
    project_root().join(MATRIX_CSV)
}

fn transcribe_live_bin() -> PathBuf {
    if let Ok(path) = env::var("CARGO_BIN_EXE_transcribe-live") {
        return PathBuf::from(path);
    }
    project_root().join("target/debug/transcribe-live")
}

fn default_model_path() -> PathBuf {
    project_root().join("artifacts/bench/models/whispercpp/ggml-tiny.en.bin")
}

fn default_fixture_path() -> PathBuf {
    project_root().join("artifacts/bench/corpus/gate_c/tts_phrase_stereo.wav")
}

fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = env::temp_dir().join(format!("{prefix}-{nanos}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn parse_bool(raw: &str) -> Result<bool, Box<dyn Error>> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("invalid bool `{raw}`").into()),
    }
}

fn parse_u64(raw: &str, field: &str) -> Result<u64, Box<dyn Error>> {
    raw.trim()
        .parse::<u64>()
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

fn load_matrix_expectations() -> Result<HashMap<String, ScenarioExpectation>, Box<dyn Error>> {
    let csv = fs::read_to_string(matrix_csv_path())?;
    let mut expectations = HashMap::new();

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

        let scenario = fields[0].to_owned();
        let expectation = ScenarioExpectation {
            scenario: scenario.clone(),
            kind: fields[1].to_owned(),
            runtime_mode: fields[2].to_owned(),
            runtime_mode_taxonomy: fields[3].to_owned(),
            runtime_mode_selector: fields[4].to_owned(),
            runtime_mode_status: fields[5].to_owned(),
            channel_mode_requested: fields[6].to_owned(),
            channel_mode_active: fields[7].to_owned(),
            jsonl_event_types: parse_pipe_set(fields[8]),
            lifecycle_phases: parse_pipe_vec(fields[9]),
            partial_count: parse_u64(fields[10], "partial_count")?,
            final_count: parse_u64(fields[11], "final_count")?,
            llm_final_count: parse_u64(fields[12], "llm_final_count")?,
            reconciled_final_count: parse_u64(fields[13], "reconciled_final_count")?,
            trust_notice_count: parse_u64(fields[14], "trust_notice_count")?,
            trust_codes: parse_pipe_set(fields[15]),
            degradation_event_count: parse_u64(fields[16], "degradation_event_count")?,
            degradation_codes: parse_pipe_set(fields[17]),
            reconciliation_required: parse_bool(fields[18])?,
            reconciliation_applied: parse_bool(fields[19])?,
            out_wav_materialized: parse_bool(fields[20])?,
            manifest_path: project_root().join(fields[21]),
            jsonl_path: project_root().join(fields[22]),
        };

        expectations.insert(scenario, expectation);
    }

    Ok(expectations)
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

fn extract_json_bool_field(line: &str, key: &str) -> Option<bool> {
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

    let tail = &line[index..];
    if tail.starts_with("true") {
        Some(true)
    } else if tail.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn jsonl_event_types(path: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut kinds = BTreeSet::new();
    for line in fs::read_to_string(path)?.lines() {
        if let Some(event_type) = extract_json_string_field(line, "event_type") {
            kinds.insert(event_type);
        }
    }
    Ok(kinds)
}

fn jsonl_event_count(path: &Path, event_type: &str) -> Result<u64, Box<dyn Error>> {
    let mut count = 0_u64;
    for line in fs::read_to_string(path)?.lines() {
        if extract_json_string_field(line, "event_type").as_deref() == Some(event_type) {
            count += 1;
        }
    }
    Ok(count)
}

fn jsonl_codes_for_event(
    path: &Path,
    event_type: &str,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut codes = BTreeSet::new();
    for line in fs::read_to_string(path)?.lines() {
        if extract_json_string_field(line, "event_type").as_deref() != Some(event_type) {
            continue;
        }
        if let Some(code) = extract_json_string_field(line, "code") {
            codes.insert(code);
        }
    }
    Ok(codes)
}

fn jsonl_lifecycle_phases(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut phases = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        if extract_json_string_field(line, "event_type").as_deref() != Some("lifecycle_phase") {
            continue;
        }
        if let Some(phase) = extract_json_string_field(line, "phase") {
            phases.push(phase);
        }
    }
    Ok(phases)
}

fn assert_semantics_match(expectation: &ScenarioExpectation) -> Result<(), Box<dyn Error>> {
    let manifest = fs::read_to_string(&expectation.manifest_path)?;
    let event_types = jsonl_event_types(&expectation.jsonl_path)?;
    let lifecycle_phases = jsonl_lifecycle_phases(&expectation.jsonl_path)?;

    assert!(
        expectation.manifest_path.is_file(),
        "missing manifest for scenario `{}` at {}",
        expectation.scenario,
        expectation.manifest_path.display()
    );
    assert!(
        expectation.jsonl_path.is_file(),
        "missing JSONL for scenario `{}` at {}",
        expectation.scenario,
        expectation.jsonl_path.display()
    );

    assert_eq!(
        extract_json_string_field(&manifest, "kind").as_deref(),
        Some(expectation.kind.as_str()),
        "scenario `{}` kind drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_string_field(&manifest, "runtime_mode").as_deref(),
        Some(expectation.runtime_mode.as_str()),
        "scenario `{}` runtime_mode drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_string_field(&manifest, "runtime_mode_taxonomy").as_deref(),
        Some(expectation.runtime_mode_taxonomy.as_str()),
        "scenario `{}` runtime_mode_taxonomy drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_string_field(&manifest, "runtime_mode_selector").as_deref(),
        Some(expectation.runtime_mode_selector.as_str()),
        "scenario `{}` runtime_mode_selector drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_string_field(&manifest, "runtime_mode_status").as_deref(),
        Some(expectation.runtime_mode_status.as_str()),
        "scenario `{}` runtime_mode_status drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_string_field(&manifest, "channel_mode_requested").as_deref(),
        Some(expectation.channel_mode_requested.as_str()),
        "scenario `{}` channel_mode_requested drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_string_field(&manifest, "channel_mode").as_deref(),
        Some(expectation.channel_mode_active.as_str()),
        "scenario `{}` channel_mode_active drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_bool_field(&manifest, "out_wav_materialized"),
        Some(expectation.out_wav_materialized),
        "scenario `{}` out_wav_materialized drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_bool_field(&manifest, "required"),
        Some(expectation.reconciliation_required),
        "scenario `{}` reconciliation.required drift",
        expectation.scenario
    );
    assert_eq!(
        extract_json_bool_field(&manifest, "applied"),
        Some(expectation.reconciliation_applied),
        "scenario `{}` reconciliation.applied drift",
        expectation.scenario
    );
    assert_eq!(
        lifecycle_phases, expectation.lifecycle_phases,
        "scenario `{}` lifecycle phase ordering drift",
        expectation.scenario
    );

    for expected_event_type in &expectation.jsonl_event_types {
        assert!(
            event_types.contains(expected_event_type),
            "scenario `{}` missing expected JSONL event_type `{}`",
            expectation.scenario,
            expected_event_type
        );
    }

    let observed_partial_count = jsonl_event_count(&expectation.jsonl_path, "partial")?;
    if matches!(
        expectation.scenario.as_str(),
        "live-stream-cold" | "live-stream-warm"
    ) && expectation.final_count > 0
    {
        let lower_bound = expectation.final_count;
        let upper_bound = expectation.partial_count;
        assert!(
            observed_partial_count >= lower_bound
                && observed_partial_count <= upper_bound
                && observed_partial_count % expectation.final_count == 0,
            "scenario `{}` partial count drift: observed={} allowed=[{}, {}] step={}",
            expectation.scenario,
            observed_partial_count,
            lower_bound,
            upper_bound,
            expectation.final_count
        );
    } else {
        assert_eq!(
            observed_partial_count, expectation.partial_count,
            "scenario `{}` partial count drift",
            expectation.scenario
        );
    }
    assert_eq!(
        jsonl_event_count(&expectation.jsonl_path, "final")?,
        expectation.final_count,
        "scenario `{}` final count drift",
        expectation.scenario
    );
    assert_eq!(
        jsonl_event_count(&expectation.jsonl_path, "llm_final")?,
        expectation.llm_final_count,
        "scenario `{}` llm_final count drift",
        expectation.scenario
    );
    assert_eq!(
        jsonl_event_count(&expectation.jsonl_path, "reconciled_final")?,
        expectation.reconciled_final_count,
        "scenario `{}` reconciled_final count drift",
        expectation.scenario
    );
    assert_eq!(
        jsonl_event_count(&expectation.jsonl_path, "trust_notice")?,
        expectation.trust_notice_count,
        "scenario `{}` trust_notice count drift",
        expectation.scenario
    );
    assert_eq!(
        jsonl_event_count(&expectation.jsonl_path, "mode_degradation")?,
        expectation.degradation_event_count,
        "scenario `{}` mode_degradation count drift",
        expectation.scenario
    );
    assert_eq!(
        jsonl_codes_for_event(&expectation.jsonl_path, "trust_notice")?,
        expectation.trust_codes,
        "scenario `{}` trust code drift",
        expectation.scenario
    );
    assert_eq!(
        jsonl_codes_for_event(&expectation.jsonl_path, "mode_degradation")?,
        expectation.degradation_codes,
        "scenario `{}` degradation code drift",
        expectation.scenario
    );

    Ok(())
}

fn copy_with_expected_paths(
    expectation: &ScenarioExpectation,
    manifest_path: PathBuf,
    jsonl_path: PathBuf,
) -> ScenarioExpectation {
    let mut copy = expectation.clone();
    copy.manifest_path = manifest_path;
    copy.jsonl_path = jsonl_path;
    copy
}

fn runtime_prereq_missing_reason() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return Some("requires macOS runtime".to_string());
    }

    let model = default_model_path();
    if !model.is_file() {
        return Some(format!("missing model fixture: {}", model.display()));
    }

    let fixture = default_fixture_path();
    if !fixture.is_file() {
        return Some(format!("missing capture fixture: {}", fixture.display()));
    }

    let bin = transcribe_live_bin();
    if !bin.is_file() {
        return Some(format!("missing transcribe-live binary: {}", bin.display()));
    }

    None
}

fn run_current_scenario(
    scenario: &ScenarioExpectation,
    mode: RuntimeMode,
) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let out_dir = temp_dir(&format!("recordit-bd-1n5v-{}", scenario.scenario));
    let input_wav = out_dir.join("session.input.wav");
    let out_wav = out_dir.join("session.wav");
    let out_jsonl = out_dir.join("session.jsonl");
    let out_manifest = out_dir.join("session.manifest.json");

    let mut command = Command::new(transcribe_live_bin());
    command.env("DYLD_LIBRARY_PATH", "/usr/lib/swift");

    if matches!(
        mode,
        RuntimeMode::RepresentativeChunked | RuntimeMode::LiveStream
    ) {
        command.env("RECORDIT_FAKE_CAPTURE_FIXTURE", default_fixture_path());
    }

    command
        .arg("--duration-sec")
        .arg(DEFAULT_DURATION_SEC.to_string())
        .arg("--input-wav")
        .arg(if matches!(mode, RuntimeMode::RepresentativeOffline) {
            default_fixture_path()
        } else {
            input_wav.clone()
        })
        .arg("--out-wav")
        .arg(&out_wav)
        .arg("--out-jsonl")
        .arg(&out_jsonl)
        .arg("--out-manifest")
        .arg(&out_manifest)
        .arg("--asr-backend")
        .arg("whispercpp")
        .arg("--asr-model")
        .arg(default_model_path())
        .arg("--benchmark-runs")
        .arg("1")
        .arg("--transcribe-channels")
        .arg("mixed-fallback");

    if matches!(mode, RuntimeMode::RepresentativeChunked) {
        command.arg("--live-chunked");
    }
    if matches!(mode, RuntimeMode::LiveStream) {
        command.arg("--live-stream");
    }
    if matches!(
        mode,
        RuntimeMode::RepresentativeChunked | RuntimeMode::LiveStream
    ) {
        command
            .arg("--chunk-window-ms")
            .arg(LIVE_CHUNK_WINDOW_MS.to_string())
            .arg("--chunk-stride-ms")
            .arg(LIVE_CHUNK_STRIDE_MS.to_string())
            .arg("--chunk-queue-cap")
            .arg(LIVE_CHUNK_QUEUE_CAP.to_string());
    }

    let output = command.output()?;
    assert!(
        output.status.success(),
        "scenario `{}` execution failed with status {}\nstdout:\n{}\nstderr:\n{}",
        scenario.scenario,
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok((out_manifest, out_jsonl))
}

#[test]
fn frozen_matrix_artifacts_match_declared_expectations() -> Result<(), Box<dyn Error>> {
    let expectations = load_matrix_expectations()?;
    assert!(
        !expectations.is_empty(),
        "matrix.csv produced zero scenarios"
    );

    for expectation in expectations.values() {
        assert_semantics_match(expectation)?;
    }

    Ok(())
}

#[test]
fn current_runtime_matches_frozen_semantics_for_core_modes() -> Result<(), Box<dyn Error>> {
    if let Some(reason) = runtime_prereq_missing_reason() {
        eprintln!("skipping bd-1n5v runtime regression harness: {reason}");
        return Ok(());
    }

    let expectations = load_matrix_expectations()?;
    let offline = expectations
        .get("representative-offline")
        .ok_or("missing representative-offline scenario in matrix.csv")?
        .clone();
    let chunked = expectations
        .get("representative-chunked")
        .ok_or("missing representative-chunked scenario in matrix.csv")?
        .clone();
    let live_stream = expectations
        .get("live-stream-cold")
        .ok_or("missing live-stream-cold scenario in matrix.csv")?
        .clone();

    let (offline_manifest, offline_jsonl) =
        run_current_scenario(&offline, RuntimeMode::RepresentativeOffline)?;
    assert_semantics_match(&copy_with_expected_paths(
        &offline,
        offline_manifest,
        offline_jsonl,
    ))?;

    let (chunked_manifest, chunked_jsonl) =
        run_current_scenario(&chunked, RuntimeMode::RepresentativeChunked)?;
    assert_semantics_match(&copy_with_expected_paths(
        &chunked,
        chunked_manifest,
        chunked_jsonl,
    ))?;

    let (live_manifest, live_jsonl) = run_current_scenario(&live_stream, RuntimeMode::LiveStream)?;
    assert_semantics_match(&copy_with_expected_paths(
        &live_stream,
        live_manifest,
        live_jsonl,
    ))?;

    Ok(())
}
