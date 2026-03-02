use std::path::PathBuf;
use std::process::Command;

fn run_recordit(args: &[String]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_recordit"))
        .args(args)
        .output()
        .expect("failed to execute recordit binary")
}

#[test]
fn unknown_command_maps_to_usage_failure_exit() {
    let output = run_recordit(&["wat".to_string()]);
    assert!(!output.status.success(), "unknown command should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command `wat`"));
    assert!(stderr.contains("recordit --help"));
    assert!(stderr.contains("run_status=failed"));
    assert!(stderr.contains("remediation_hint="));
}

#[test]
fn offline_mode_without_input_wav_maps_to_config_failure_exit() {
    let output = run_recordit(&[
        "run".to_string(),
        "--mode".to_string(),
        "offline".to_string(),
    ]);
    assert!(
        !output.status.success(),
        "offline run without input should fail"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires `--input-wav <path>`"));
    assert!(stderr.contains("run_status=failed"));
    assert!(stderr.contains("remediation_hint="));
}

#[test]
fn replay_missing_jsonl_maps_to_runtime_failure_exit() {
    let missing = PathBuf::from("/tmp").join(format!(
        "recordit-missing-replay-{}.jsonl",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&missing);

    let output = run_recordit(&[
        "replay".to_string(),
        "--jsonl".to_string(),
        missing.display().to_string(),
        "--format".to_string(),
        "text".to_string(),
    ]);
    assert!(!output.status.success(), "missing replay file should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("replay failed") || stderr.contains("failed to read replay JSONL"),
        "unexpected stderr: {stderr}"
    );
    assert!(stderr.contains("run_status=failed"));
    assert!(stderr.contains("remediation_hint="));
}

#[test]
fn inspect_contract_jsonl_schema_stays_machine_readable() {
    let output = run_recordit(&[
        "inspect-contract".to_string(),
        "jsonl-schema".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ]);
    assert!(output.status.success(), "inspect-contract should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"$schema\"") || stdout.contains("\"contract\":\"jsonl-schema\""),
        "unexpected schema payload: {stdout}"
    );
}
