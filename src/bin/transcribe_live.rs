#[cfg(target_os = "macos")]
use std::process::Command;
use std::{env, path::Path, process::ExitCode};

#[path = "transcribe_live/app.rs"]
mod app;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if should_block_unsupported_no_arg_launch(&args) {
        emit_no_arg_launch_guidance();
        return ExitCode::from(2);
    }

    if args
        .iter()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!(
            "Migration note: prefer `recordit run --mode live` (or `--mode offline`) for normal operator usage."
        );
        println!(
            "Compatibility note: `transcribe-live` remains stable for legacy automation, gates, and expert controls."
        );
        println!();
    }
    app::run_with_args(args.into_iter().skip(1))
}

fn should_block_unsupported_no_arg_launch(args: &[String]) -> bool {
    let Some(program_path) = args.first() else {
        return false;
    };
    args.len() == 1 && is_sequoia_transcribe_program_path(program_path)
}

fn is_sequoia_transcribe_program_path(program_path: &str) -> bool {
    let executable_name = Path::new(program_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if executable_name == "SequoiaTranscribe" {
        return true;
    }
    program_path
        .to_ascii_lowercase()
        .contains("sequoiatranscribe.app")
}

fn emit_no_arg_launch_guidance() {
    let title = "Use Recordit.app (Default)";
    let message = "SequoiaTranscribe.app is a compatibility runtime path and does not support double-click no-arg launch.\n\nOpen Recordit.app instead (default user-facing app).\nIf needed, run SequoiaTranscribe from Terminal with explicit args via `make run-transcribe-app`.";
    eprintln!("{title}: {message}");
    attempt_display_guidance_dialog(title, message);
}

#[cfg(target_os = "macos")]
fn attempt_display_guidance_dialog(title: &str, message: &str) {
    let escaped_title = escape_applescript(title);
    let escaped_message = escape_applescript(message);
    let script = format!(
        "display alert \"{escaped_title}\" message \"{escaped_message}\" buttons {{\"OK\"}} default button \"OK\""
    );
    let _ = Command::new("osascript").arg("-e").arg(script).status();
}

#[cfg(not(target_os = "macos"))]
fn attempt_display_guidance_dialog(_title: &str, _message: &str) {}

#[cfg(target_os = "macos")]
fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{is_sequoia_transcribe_program_path, should_block_unsupported_no_arg_launch};

    #[test]
    fn blocks_no_arg_sequoia_transcribe_executable_launch() {
        let args =
            vec!["/tmp/dist/SequoiaTranscribe.app/Contents/MacOS/SequoiaTranscribe".to_string()];
        assert!(should_block_unsupported_no_arg_launch(&args));
    }

    #[test]
    fn does_not_block_when_args_are_present() {
        let args = vec![
            "/tmp/dist/SequoiaTranscribe.app/Contents/MacOS/SequoiaTranscribe".to_string(),
            "--live-stream".to_string(),
        ];
        assert!(!should_block_unsupported_no_arg_launch(&args));
    }

    #[test]
    fn does_not_block_non_sequoia_no_arg_launch() {
        let args = vec!["/tmp/target/debug/transcribe-live".to_string()];
        assert!(!should_block_unsupported_no_arg_launch(&args));
    }

    #[test]
    fn recognizes_bundle_path_marker_for_sequoia_transcribe() {
        assert!(is_sequoia_transcribe_program_path(
            "/Users/test/Apps/SequoiaTranscribe.app/Contents/MacOS/transcribe-live"
        ));
    }
}
