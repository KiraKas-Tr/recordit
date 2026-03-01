use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    recordit::live_capture::run_capture_cli(&args)
}
