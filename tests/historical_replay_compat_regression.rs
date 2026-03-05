use serde_json::Value;
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

fn run_transcribe_live(args: &[&str]) -> Output {
    Command::new(transcribe_live_bin())
        .args(args)
        .output()
        .expect("failed to execute transcribe-live")
}

fn frozen_replay_fixtures() -> Vec<PathBuf> {
    let root = project_root();
    vec![
        root.join("artifacts/validation/bd-1qfx/live-stream-cold.runtime.jsonl"),
        root.join("artifacts/bench/gate_v1_acceptance/20260301T130355Z/cold/runtime.jsonl"),
        root.join("artifacts/bench/gate_v1_acceptance/20260301T130355Z/warm/runtime.jsonl"),
        root.join("artifacts/bench/gate_backlog_pressure/20260302T074649Z/runtime.jsonl"),
    ]
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    let raw = fs::read_to_string(path).expect("failed to read fixture JSONL");
    raw.lines()
        .enumerate()
        .map(|(_idx, line)| serde_json::from_str::<Value>(line).expect("invalid fixture JSON row"))
        .collect()
}

fn transcript_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "partial" | "final" | "llm_final" | "reconciled_final"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplayRow {
    event_type: String,
    channel: String,
    start_ms: u64,
    end_ms: u64,
    text: String,
    segment_id: String,
}

fn fixture_transcript_rows(path: &Path) -> Vec<ReplayRow> {
    read_jsonl(path)
        .into_iter()
        .filter_map(|row| {
            let event_type = row.get("event_type")?.as_str()?.to_string();
            if !transcript_event_type(&event_type) {
                return None;
            }
            let channel = row.get("channel")?.as_str()?.to_string();
            let start_ms = row.get("start_ms")?.as_u64()?;
            let end_ms = row.get("end_ms")?.as_u64()?;
            let text = row
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let segment_id = row
                .get("segment_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some(ReplayRow {
                event_type,
                channel,
                start_ms,
                end_ms,
                text,
                segment_id,
            })
        })
        .collect()
}

fn parse_replay_rows(stdout: &str) -> Vec<ReplayRow> {
    stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("partial channel=")
                || trimmed.starts_with("final channel=")
                || trimmed.starts_with("llm_final channel=")
                || trimmed.starts_with("reconciled_final channel="))
            {
                return None;
            }

            let mut parts = trimmed.splitn(3, ' ');
            let event_type = parts.next()?.to_string();
            let channel = parts.next()?.strip_prefix("channel=")?.to_string();
            let timing_and_text = parts.next()?;
            let close_bracket = timing_and_text.find("] ")?;
            let timing = timing_and_text
                .strip_prefix('[')?
                .get(..close_bracket - 1)?;
            let text = timing_and_text
                .get(close_bracket + 2..)
                .unwrap_or("")
                .to_string();

            let mut timing_parts = timing.splitn(2, '-');
            let start_ms = timing_parts.next()?.parse::<u64>().ok()?;
            let end_ms = timing_parts
                .next()?
                .strip_suffix("ms")?
                .parse::<u64>()
                .ok()?;

            Some(ReplayRow {
                event_type,
                channel,
                start_ms,
                end_ms,
                text,
                segment_id: String::new(),
            })
        })
        .collect()
}

fn parse_reported_replay_event_count(stdout: &str) -> Option<usize> {
    stdout
        .lines()
        .find_map(|line| {
            line.trim_start()
                .strip_prefix("events:")
                .map(str::trim)
                .map(str::to_string)
        })
        .and_then(|raw| raw.parse::<usize>().ok())
}

#[test]
fn historical_frozen_artifacts_replay_with_event_level_parity() {
    for fixture in frozen_replay_fixtures() {
        assert!(fixture.is_file(), "missing fixture {}", fixture.display());

        let expected_rows = fixture_transcript_rows(&fixture);
        assert!(
            !expected_rows.is_empty(),
            "fixture {} has no transcript rows",
            fixture.display()
        );

        let output = run_transcribe_live(&["--replay-jsonl", fixture.to_str().unwrap()]);
        assert!(
            output.status.success(),
            "replay failed for {}\nstderr={}\nstdout={}",
            fixture.display(),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        let reported_count =
            parse_reported_replay_event_count(&stdout).expect("missing replay event count");
        assert_eq!(
            reported_count,
            expected_rows.len(),
            "reported replay event count mismatch for {}",
            fixture.display()
        );

        let actual_rows = parse_replay_rows(&stdout);
        assert_eq!(
            actual_rows.len(),
            expected_rows.len(),
            "replay row count mismatch for {}\nexpected={} actual={}",
            fixture.display(),
            expected_rows.len(),
            actual_rows.len()
        );

        for (idx, (expected, actual)) in expected_rows.iter().zip(actual_rows.iter()).enumerate() {
            assert_eq!(
                actual.event_type,
                expected.event_type,
                "event_type mismatch at {} row {} expected_segment_id={} expected_channel={} actual_channel={}",
                fixture.display(),
                idx + 1,
                expected.segment_id,
                expected.channel,
                actual.channel
            );
            assert_eq!(
                actual.channel,
                expected.channel,
                "channel mismatch at {} row {} expected_segment_id={} expected_event_type={}",
                fixture.display(),
                idx + 1,
                expected.segment_id,
                expected.event_type
            );
            assert_eq!(
                actual.start_ms,
                expected.start_ms,
                "start_ms mismatch at {} row {} expected_segment_id={} expected_event_type={}",
                fixture.display(),
                idx + 1,
                expected.segment_id,
                expected.event_type
            );
            assert_eq!(
                actual.end_ms,
                expected.end_ms,
                "end_ms mismatch at {} row {} expected_segment_id={} expected_event_type={}",
                fixture.display(),
                idx + 1,
                expected.segment_id,
                expected.event_type
            );
            assert_eq!(
                actual.text,
                expected.text,
                "text mismatch at {} row {} expected_segment_id={} expected_event_type={} expected_channel={}",
                fixture.display(),
                idx + 1,
                expected.segment_id,
                expected.event_type,
                expected.channel
            );
        }
    }
}

#[test]
fn malformed_historical_replay_rows_emit_line_and_payload_diagnostics() {
    let root = project_root();
    let fixture = root.join("artifacts/validation/bd-1qfx/live-stream-cold.runtime.jsonl");
    assert!(fixture.is_file(), "missing fixture {}", fixture.display());

    let malformed_path = root.join(format!(
        "artifacts/tmp-historical-replay-malformed-{}.jsonl",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos()
    ));

    fs::write(
        &malformed_path,
        "{\"event_type\":\"partial\",\"channel\":\"mic\",\"segment_id\":\"seg-1\"}\n",
    )
    .expect("failed to write malformed replay fixture");

    let output = run_transcribe_live(&["--replay-jsonl", malformed_path.to_str().unwrap()]);
    let _ = fs::remove_file(&malformed_path);

    assert_eq!(
        output.status.code(),
        Some(2),
        "malformed replay should fail with compatibility/runtime failure code"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid replay line 1"),
        "expected line-index diagnostics, got: {stderr}"
    );
    assert!(
        stderr.contains("event_type `partial` payload mismatch"),
        "expected event-type payload diagnostics, got: {stderr}"
    );
    assert!(
        stderr.contains("missing field `start_ms`"),
        "expected missing-field diagnostics, got: {stderr}"
    );
}
