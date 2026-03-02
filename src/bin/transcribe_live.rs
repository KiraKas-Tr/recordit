use std::{env, process::ExitCode};

#[path = "transcribe_live/app.rs"]
mod app;

fn main() -> ExitCode {
    if env::args()
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
    app::main()
}
