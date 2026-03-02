use std::process::Command;

#[test]
fn transcribe_live_help_prints_recordit_migration_guidance() {
    let output = Command::new(env!("CARGO_BIN_EXE_transcribe-live"))
        .arg("--help")
        .output()
        .expect("failed to execute transcribe-live");

    assert!(output.status.success(), "--help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("recordit run --mode live")
            && stdout.contains("recordit run --mode offline"),
        "help output should recommend recordit run modes"
    );
    assert!(
        stdout.contains("transcribe-live")
            && stdout.contains("legacy automation")
            && stdout.contains("expert controls"),
        "help output should preserve compatibility guidance"
    );
}
