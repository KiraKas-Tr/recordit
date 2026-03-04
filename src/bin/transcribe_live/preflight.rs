use super::*;

pub(super) fn run_model_doctor(config: &TranscribeConfig) -> Result<PreflightReport, CliError> {
    let mut checks = Vec::new();
    checks.push(check_backend_runtime(config.asr_backend));

    match validate_model_path_for_backend(config) {
        Ok(resolved) => {
            let expected_kind = expected_model_kind(config.asr_backend);
            checks.push(PreflightCheck::pass(
                "model_path",
                format!(
                    "model path resolved: {} via {} (expected {expected_kind} for backend {})",
                    display_path(&resolved.path),
                    resolved.source,
                    config.asr_backend,
                ),
            ));
            checks.push(check_model_asset_readability(&resolved));
        }
        Err(err) => {
            checks.push(PreflightCheck::fail(
                "model_path",
                err.to_string(),
                "Pass --asr-model, set RECORDIT_ASR_MODEL, or install the backend default asset in the documented location.",
            ));
            checks.push(PreflightCheck::fail(
                "model_readability",
                "skipped because model_path did not validate".to_string(),
                "Fix model_path first, then rerun --model-doctor.",
            ));
        }
    }

    let generated_at_utc = command_stdout("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(PreflightReport {
        generated_at_utc,
        checks,
    })
}

fn check_model_asset_readability(resolved: &ResolvedModelPath) -> PreflightCheck {
    if resolved.path.is_file() {
        return match File::open(&resolved.path) {
            Ok(_) => PreflightCheck::pass(
                "model_readability",
                format!("model file is readable: {}", display_path(&resolved.path)),
            ),
            Err(err) => PreflightCheck::fail(
                "model_readability",
                format!(
                    "cannot read model file {}: {err}",
                    display_path(&resolved.path)
                ),
                "Fix file permissions or pass a different readable model path.",
            ),
        };
    }

    if resolved.path.is_dir() {
        return match fs::read_dir(&resolved.path) {
            Ok(_) => PreflightCheck::pass(
                "model_readability",
                format!(
                    "model directory is readable: {}",
                    display_path(&resolved.path)
                ),
            ),
            Err(err) => PreflightCheck::fail(
                "model_readability",
                format!(
                    "cannot read model directory {}: {err}",
                    display_path(&resolved.path)
                ),
                "Fix directory permissions or pass a different readable model path.",
            ),
        };
    }

    PreflightCheck::fail(
        "model_readability",
        format!(
            "model path is neither a file nor directory: {}",
            display_path(&resolved.path)
        ),
        "Use a readable file/directory path matching backend expectations.",
    )
}

pub(super) fn run_preflight(config: &TranscribeConfig) -> Result<PreflightReport, CliError> {
    let mut checks = Vec::new();
    checks.push(check_model_path(config));
    checks.push(check_output_target("out_wav", &config.out_wav));
    checks.push(check_output_target("out_jsonl", &config.out_jsonl));
    checks.push(check_output_target("out_manifest", &config.out_manifest));
    checks.push(check_sample_rate(config.sample_rate_hz));
    checks.push(check_screen_capture_access());
    checks.push(check_microphone_stream(config.sample_rate_hz));
    checks.push(check_backend_runtime(config.asr_backend));

    let generated_at_utc = command_stdout("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(PreflightReport {
        generated_at_utc,
        checks,
    })
}

fn check_model_path(config: &TranscribeConfig) -> PreflightCheck {
    match validate_model_path_for_backend(config) {
        Ok(resolved) => {
            let expected_kind = expected_model_kind(config.asr_backend);
            PreflightCheck::pass(
                "model_path",
                format!(
                    "model path resolved: {} via {} (expected {expected_kind} for backend {})",
                    display_path(&resolved.path),
                    resolved.source,
                    config.asr_backend,
                ),
            )
        }
        Err(err) => PreflightCheck::fail(
            "model_path",
            err.to_string(),
            "Pass --asr-model, set RECORDIT_ASR_MODEL, or install the backend default asset in the documented location.",
        ),
    }
}

fn check_output_target(id: &'static str, path: &Path) -> PreflightCheck {
    let absolute = display_path(path);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    if path.exists() && path.is_dir() {
        return PreflightCheck::fail(
            id,
            format!("target path is a directory: {absolute}"),
            "Provide a file path, not a directory.",
        );
    }

    if let Err(err) = fs::create_dir_all(&parent) {
        return PreflightCheck::fail(
            id,
            format!("cannot create parent directory {}: {err}", parent.display()),
            "Choose an output location in a writable directory.",
        );
    }

    let probe = parent.join(format!(
        ".recordit-preflight-write-{}-{}",
        id,
        std::process::id()
    ));
    match File::create(&probe).and_then(|mut file| file.write_all(b"ok")) {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            PreflightCheck::pass(id, format!("writable output target: {absolute}"))
        }
        Err(err) => PreflightCheck::fail(
            id,
            format!("cannot write under {}: {err}", parent.display()),
            "Choose an output path in a writable directory.",
        ),
    }
}

fn check_sample_rate(sample_rate_hz: u32) -> PreflightCheck {
    if sample_rate_hz == 48_000 {
        return PreflightCheck::pass("sample_rate", "sample rate is 48000 Hz");
    }

    PreflightCheck::warn(
        "sample_rate",
        format!("non-default sample rate configured: {sample_rate_hz} Hz"),
        "Use --sample-rate 48000 unless you intentionally need a different rate.",
    )
}

fn check_screen_capture_access() -> PreflightCheck {
    let content = match SCShareableContent::get() {
        Ok(content) => content,
        Err(err) => {
            return PreflightCheck::fail(
                "screen_capture_access",
                format!("failed to query ScreenCaptureKit content: {err}"),
                "Grant Screen Recording permission and ensure at least one active display.",
            );
        }
    };

    let displays = content.displays();
    if displays.is_empty() {
        return PreflightCheck::fail(
            "display_availability",
            "ScreenCaptureKit returned no displays".to_string(),
            "Connect/enable a display and retry. Closed-lid headless mode is unsupported.",
        );
    }

    PreflightCheck::pass(
        "screen_capture_access",
        format!(
            "ScreenCaptureKit access OK; displays available={}",
            displays.len()
        ),
    )
}

fn check_microphone_stream(sample_rate_hz: u32) -> PreflightCheck {
    let content = match SCShareableContent::get() {
        Ok(content) => content,
        Err(err) => {
            return PreflightCheck::fail(
                "microphone_access",
                format!("cannot initialize microphone preflight (shareable content error): {err}"),
                "Grant Screen Recording first, then rerun preflight.",
            );
        }
    };

    let displays = content.displays();
    if displays.is_empty() {
        return PreflightCheck::fail(
            "microphone_access",
            "cannot run microphone preflight without an active display".to_string(),
            "Connect/enable a display and rerun preflight.",
        );
    }

    let filter = SCContentFilter::create()
        .with_display(&displays[0])
        .with_excluding_windows(&[])
        .build();

    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(false)
        .with_captures_microphone(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(sample_rate_hz as i32)
        .with_channel_count(1);

    let queue = DispatchQueue::new(
        "com.recordit.transcribe.preflight",
        DispatchQoS::UserInteractive,
    );
    let (tx, rx) = sync_channel::<()>(1);

    let mut stream = SCStream::new(&filter, &config);
    let tx_mic = tx.clone();
    if stream
        .add_output_handler_with_queue(
            move |_sample, _kind| {
                let _ = tx_mic.try_send(());
            },
            SCStreamOutputType::Microphone,
            Some(&queue),
        )
        .is_none()
    {
        return PreflightCheck::fail(
            "microphone_access",
            "failed to register microphone output handler".to_string(),
            "Retry preflight; if it persists, restart the app/session.",
        );
    }

    if let Err(err) = stream.start_capture() {
        return PreflightCheck::fail(
            "microphone_access",
            format!("failed to start microphone capture: {err}"),
            "Grant Microphone permission and verify an input device is connected and enabled.",
        );
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut observed_mic = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(_) => {
                observed_mic = true;
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let stop_result = stream.stop_capture();
    if let Err(err) = stop_result {
        return PreflightCheck::warn(
            "microphone_access",
            format!("microphone stream started but stop_capture reported: {err}"),
            "Retry preflight; if repeated, restart the app/session.",
        );
    }

    if observed_mic {
        PreflightCheck::pass(
            "microphone_access",
            "observed at least one microphone sample buffer".to_string(),
        )
    } else {
        PreflightCheck::fail(
            "microphone_access",
            "no microphone sample buffer observed within 2s".to_string(),
            "Grant Microphone permission, unmute/select input device, and speak briefly during preflight.",
        )
    }
}

fn check_backend_runtime(backend: AsrBackend) -> PreflightCheck {
    let tool_name = match backend {
        AsrBackend::WhisperCpp => "whisper-cli",
        AsrBackend::WhisperKit => "whisperkit-cli",
        AsrBackend::Moonshine => "moonshine",
    };

    match command_stdout("which", &[tool_name]) {
        Ok(path) => PreflightCheck::pass(
            "backend_runtime",
            format!("detected backend helper binary `{tool_name}` at {path}"),
        ),
        Err(_) => PreflightCheck::warn(
            "backend_runtime",
            format!("backend helper binary `{tool_name}` not found in PATH"),
            "Install backend tooling or keep using Rust-native integration once wired.",
        ),
    }
}

pub(super) fn print_preflight_report(report: &PreflightReport) {
    let mut pass_count = 0usize;
    let mut warn_count = 0usize;
    let mut fail_count = 0usize;

    println!("Transcribe-live preflight diagnostics");
    println!("  generated_at_utc: {}", report.generated_at_utc);
    println!("  overall_status: {}", report.overall_status());
    println!();
    println!("id\tstatus\tdetail\tremediation");

    for check in &report.checks {
        match check.status {
            CheckStatus::Pass => pass_count += 1,
            CheckStatus::Warn => warn_count += 1,
            CheckStatus::Fail => fail_count += 1,
        }
        println!(
            "{}\t{}\t{}\t{}",
            check.id,
            check.status,
            clean_field(&check.detail),
            clean_field(check.remediation.as_deref().unwrap_or("-")),
        );
    }

    println!();
    println!(
        "summary\t{}\tpass={}\twarn={}\tfail={}",
        report.overall_status(),
        pass_count,
        warn_count,
        fail_count
    );
}

pub(super) fn print_model_doctor_report(report: &PreflightReport) {
    let mut pass_count = 0usize;
    let mut warn_count = 0usize;
    let mut fail_count = 0usize;

    println!("Transcribe-live model doctor");
    println!("  generated_at_utc: {}", report.generated_at_utc);
    println!("  overall_status: {}", report.overall_status());
    println!();
    println!("id\tstatus\tdetail\tremediation");

    for check in &report.checks {
        match check.status {
            CheckStatus::Pass => pass_count += 1,
            CheckStatus::Warn => warn_count += 1,
            CheckStatus::Fail => fail_count += 1,
        }
        println!(
            "{}\t{}\t{}\t{}",
            check.id,
            check.status,
            clean_field(&check.detail),
            clean_field(check.remediation.as_deref().unwrap_or("-")),
        );
    }

    println!();
    println!(
        "summary\t{}\tpass={}\twarn={}\tfail={}",
        report.overall_status(),
        pass_count,
        warn_count,
        fail_count
    );
}
