use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Scenario {
    name: String,
    jsonl_event_types: Vec<String>,
    lifecycle_phases: Vec<String>,
    partial_count: usize,
    final_count: usize,
    llm_final_count: usize,
    reconciled_final_count: usize,
    trust_notice_count: usize,
    jsonl_path: PathBuf,
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn matrix_csv_path() -> PathBuf {
    project_root().join("artifacts/validation/bd-1qfx/matrix.csv")
}

fn load_scenarios() -> Vec<Scenario> {
    let csv_path = matrix_csv_path();
    let contents = fs::read_to_string(&csv_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", csv_path.display()));
    let mut lines = contents.lines();
    let header = lines.next().expect("matrix.csv should have a header row");
    let headers: Vec<&str> = header.split(',').collect();

    let index = |name: &str| {
        headers
            .iter()
            .position(|header| *header == name)
            .unwrap_or_else(|| panic!("missing column `{name}` in {}", csv_path.display()))
    };

    let scenario_idx = index("scenario");
    let event_types_idx = index("jsonl_event_types");
    let lifecycle_idx = index("lifecycle_phases");
    let partial_idx = index("partial_count");
    let final_idx = index("final_count");
    let llm_final_idx = index("llm_final_count");
    let reconciled_final_idx = index("reconciled_final_count");
    let trust_notice_idx = index("trust_notice_count");
    let jsonl_path_idx = index("jsonl_path");

    lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let cols: Vec<&str> = line.split(',').collect();
            Scenario {
                name: cols[scenario_idx].to_string(),
                jsonl_event_types: split_pipe_field(cols[event_types_idx]),
                lifecycle_phases: split_pipe_field(cols[lifecycle_idx]),
                partial_count: parse_usize(cols[partial_idx], "partial_count", line),
                final_count: parse_usize(cols[final_idx], "final_count", line),
                llm_final_count: parse_usize(cols[llm_final_idx], "llm_final_count", line),
                reconciled_final_count: parse_usize(
                    cols[reconciled_final_idx],
                    "reconciled_final_count",
                    line,
                ),
                trust_notice_count: parse_usize(cols[trust_notice_idx], "trust_notice_count", line),
                jsonl_path: project_root().join(cols[jsonl_path_idx]),
            }
        })
        .collect()
}

fn split_pipe_field(value: &str) -> Vec<String> {
    if value.trim().is_empty() {
        Vec::new()
    } else {
        value.split('|').map(|part| part.to_string()).collect()
    }
}

fn parse_usize(value: &str, field: &str, line: &str) -> usize {
    value
        .parse::<usize>()
        .unwrap_or_else(|err| panic!("failed to parse {field} from `{line}`: {err}"))
}

fn read_jsonl_lines(path: &Path) -> Vec<String> {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect()
}

fn extract_string_field(line: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\":\"");
    let (_, rest) = line.split_once(&needle)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn has_field(line: &str, field: &str) -> bool {
    line.contains(&format!("\"{field}\":"))
}

fn required_fields_for_event(event_type: &str) -> &'static [&'static str] {
    match event_type {
        "partial" | "final" | "llm_final" | "reconciled_final" => &[
            "event_type",
            "channel",
            "segment_id",
            "start_ms",
            "end_ms",
            "text",
            "asr_backend",
            "vad_boundary_count",
        ],
        "vad_boundary" => &[
            "event_type",
            "channel",
            "boundary_id",
            "start_ms",
            "end_ms",
            "source",
            "vad_backend",
            "vad_threshold",
        ],
        "mode_degradation" => &[
            "event_type",
            "channel",
            "requested_mode",
            "active_mode",
            "code",
            "detail",
        ],
        "trust_notice" => &[
            "event_type",
            "channel",
            "code",
            "severity",
            "cause",
            "impact",
            "guidance",
        ],
        "lifecycle_phase" => &[
            "event_type",
            "channel",
            "phase",
            "transition_index",
            "entered_at_utc",
            "ready_for_transcripts",
            "detail",
        ],
        "reconciliation_matrix" => &[
            "event_type",
            "channel",
            "required",
            "applied",
            "trigger_count",
            "trigger_codes",
        ],
        "asr_worker_pool" => &[
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
        ],
        "chunk_queue" => &[
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
        ],
        "cleanup_queue" => &[
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
        ],
        _ => &[],
    }
}

fn transcript_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "partial" | "final" | "llm_final" | "reconciled_final"
    )
}

#[test]
fn frozen_jsonl_matrix_keeps_expected_event_families_and_counts() {
    let scenarios = load_scenarios();
    assert!(
        !scenarios.is_empty(),
        "expected frozen scenarios in matrix.csv"
    );

    for scenario in &scenarios {
        let lines = read_jsonl_lines(&scenario.jsonl_path);
        let mut event_types = BTreeSet::new();
        let mut counts = BTreeMap::new();

        for line in &lines {
            let event_type = extract_string_field(line, "event_type").unwrap_or_else(|| {
                panic!(
                    "missing event_type in {} line: {}",
                    scenario.jsonl_path.display(),
                    line
                )
            });
            event_types.insert(event_type.clone());
            *counts.entry(event_type).or_insert(0usize) += 1;
        }

        let expected_event_types: BTreeSet<String> =
            scenario.jsonl_event_types.iter().cloned().collect();
        assert_eq!(
            event_types, expected_event_types,
            "event family drift for scenario {}",
            scenario.name
        );
        assert_eq!(
            counts.get("partial").copied().unwrap_or(0),
            scenario.partial_count
        );
        assert_eq!(
            counts.get("final").copied().unwrap_or(0),
            scenario.final_count
        );
        assert_eq!(
            counts.get("llm_final").copied().unwrap_or(0),
            scenario.llm_final_count
        );
        assert_eq!(
            counts.get("reconciled_final").copied().unwrap_or(0),
            scenario.reconciled_final_count
        );
        assert_eq!(
            counts.get("trust_notice").copied().unwrap_or(0),
            scenario.trust_notice_count
        );
    }
}

#[test]
fn frozen_jsonl_rows_keep_required_keys_by_event_family() {
    for scenario in load_scenarios() {
        for line in read_jsonl_lines(&scenario.jsonl_path) {
            let event_type = extract_string_field(&line, "event_type").unwrap_or_else(|| {
                panic!(
                    "missing event_type in {} line: {}",
                    scenario.jsonl_path.display(),
                    line
                )
            });
            let required = required_fields_for_event(&event_type);
            assert!(
                !required.is_empty(),
                "unexpected event_type `{}` in {}",
                event_type,
                scenario.jsonl_path.display()
            );
            for field in required {
                assert!(
                    has_field(&line, field),
                    "missing required field `{}` for event `{}` in {} line: {}",
                    field,
                    event_type,
                    scenario.jsonl_path.display(),
                    line
                );
            }
        }
    }
}

#[test]
fn frozen_live_jsonl_keeps_lifecycle_order_and_transcript_sequence() {
    for scenario in load_scenarios() {
        if scenario.lifecycle_phases.is_empty() {
            continue;
        }

        let lines = read_jsonl_lines(&scenario.jsonl_path);
        let lifecycle_phases: Vec<String> = lines
            .iter()
            .filter_map(|line| {
                let event_type = extract_string_field(line, "event_type")?;
                if event_type == "lifecycle_phase" {
                    extract_string_field(line, "phase")
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            lifecycle_phases, scenario.lifecycle_phases,
            "lifecycle phase ordering drift for scenario {}",
            scenario.name
        );

        let active_idx = lines
            .iter()
            .position(|line| {
                extract_string_field(line, "event_type").as_deref() == Some("lifecycle_phase")
                    && extract_string_field(line, "phase").as_deref() == Some("active")
            })
            .unwrap_or_else(|| panic!("missing active phase in {}", scenario.jsonl_path.display()));

        let first_transcript_idx = lines
            .iter()
            .position(|line| {
                extract_string_field(line, "event_type")
                    .as_deref()
                    .is_some_and(transcript_event_type)
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing transcript events in {}",
                    scenario.jsonl_path.display()
                )
            });

        let shutdown_idx = lines
            .iter()
            .position(|line| {
                extract_string_field(line, "event_type").as_deref() == Some("lifecycle_phase")
                    && extract_string_field(line, "phase").as_deref() == Some("shutdown")
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing shutdown phase in {}",
                    scenario.jsonl_path.display()
                )
            });

        assert!(
            active_idx < first_transcript_idx,
            "active phase must precede transcript emission in scenario {}",
            scenario.name
        );
        assert!(
            first_transcript_idx < shutdown_idx,
            "transcript emission must occur before shutdown in scenario {}",
            scenario.name
        );
    }
}
