use std::process::Command;

fn run_recordit(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_recordit"))
        .args(args)
        .output()
        .expect("failed to execute recordit binary")
}

#[test]
fn help_prints_canonical_top_level_verbs() {
    let output = run_recordit(&["--help"]);
    assert!(output.status.success(), "help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("recordit run --mode <live|offline>"));
    assert!(stdout.contains("recordit doctor"));
    assert!(stdout.contains("recordit preflight"));
    assert!(stdout.contains("recordit replay --jsonl <path>"));
    assert!(stdout.contains("recordit inspect-contract"));
}

#[test]
fn inspect_contract_cli_json_returns_machine_payload() {
    let output = run_recordit(&["inspect-contract", "cli", "--format", "json"]);
    assert!(
        output.status.success(),
        "inspect-contract cli should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"contract\":\"cli\""));
    assert!(stdout.contains("recordit run --mode <live|offline> [mode options] [shared options]"));
}

#[test]
fn inspect_contract_runtime_modes_json_reports_live_and_offline_mapping() {
    let output = run_recordit(&["inspect-contract", "runtime-modes", "--format", "json"]);
    assert!(
        output.status.success(),
        "inspect-contract runtime-modes should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"mode\":\"live\""));
    assert!(stdout.contains("\"runtime_mode\":\"live-stream\""));
    assert!(stdout.contains("\"mode\":\"offline\""));
    assert!(stdout.contains("\"runtime_mode\":\"representative-offline\""));
}

#[test]
fn unknown_command_exits_nonzero_with_help_hint() {
    let output = run_recordit(&["wat"]);
    assert!(!output.status.success(), "unknown command should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command `wat`"));
    assert!(stderr.contains("recordit --help"));
}
