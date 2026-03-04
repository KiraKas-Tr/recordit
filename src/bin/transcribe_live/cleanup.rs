use super::*;

pub(super) fn run_cleanup_queue(
    config: &TranscribeConfig,
    events: &[TranscriptEvent],
) -> CleanupRunResult {
    run_cleanup_queue_with(config, events, invoke_cleanup_endpoint)
}

pub(super) fn run_cleanup_queue_with<F>(
    config: &TranscribeConfig,
    events: &[TranscriptEvent],
    invoke_cleanup: F,
) -> CleanupRunResult
where
    F: Fn(&CleanupClientConfig, &CleanupRequest) -> CleanupAttemptOutcome + Send + Sync + 'static,
{
    if !config.llm_cleanup {
        return CleanupRunResult {
            telemetry: CleanupQueueTelemetry::disabled(config),
            llm_events: Vec::new(),
        };
    }

    let mut telemetry = CleanupQueueTelemetry {
        enabled: true,
        max_queue: config.llm_max_queue,
        timeout_ms: config.llm_timeout_ms,
        retries: config.llm_retries,
        submitted: 0,
        enqueued: 0,
        dropped_queue_full: 0,
        processed: 0,
        succeeded: 0,
        timed_out: 0,
        failed: 0,
        retry_attempts: 0,
        pending: 0,
        drain_budget_ms: config.llm_timeout_ms,
        drain_completed: true,
    };
    let mut llm_events = Vec::new();

    let Some(endpoint) = config.llm_endpoint.clone() else {
        return CleanupRunResult {
            telemetry,
            llm_events,
        };
    };
    let Some(model) = config.llm_model.clone() else {
        return CleanupRunResult {
            telemetry,
            llm_events,
        };
    };

    let requests = cleanup_requests_from_events(events);
    telemetry.submitted = requests.len();
    if requests.is_empty() {
        return CleanupRunResult {
            telemetry,
            llm_events,
        };
    }

    let client = CleanupClientConfig {
        endpoint,
        model,
        timeout_ms: config.llm_timeout_ms,
        retries: config.llm_retries,
    };
    let (request_tx, request_rx) = sync_channel::<CleanupRequest>(config.llm_max_queue);
    let (result_tx, result_rx) = mpsc::channel::<CleanupTaskResult>();
    let invoke_cleanup = Arc::new(invoke_cleanup);
    let worker_invoke = Arc::clone(&invoke_cleanup);
    let worker_handle = thread::spawn(move || {
        cleanup_worker_loop(request_rx, result_tx, client, worker_invoke);
    });

    for request in requests {
        match request_tx.try_send(request) {
            Ok(()) => telemetry.enqueued += 1,
            Err(TrySendError::Full(_)) => telemetry.dropped_queue_full += 1,
            Err(TrySendError::Disconnected(_)) => telemetry.failed += 1,
        }
    }
    drop(request_tx);

    let drain_deadline = Instant::now() + Duration::from_millis(config.llm_timeout_ms);
    while telemetry.processed < telemetry.enqueued {
        let now = Instant::now();
        if now >= drain_deadline {
            break;
        }
        let remaining = drain_deadline.saturating_duration_since(now);
        let wait_for = remaining.min(Duration::from_millis(5));
        match result_rx.recv_timeout(wait_for) {
            Ok(result) => {
                telemetry.processed += 1;
                telemetry.retry_attempts += result.retry_attempts;
                match result.status {
                    CleanupTaskStatus::Succeeded => {
                        telemetry.succeeded += 1;
                        if let Some(cleaned_text) = result.cleaned_text {
                            let source_segment_id = result.request.segment_id.clone();
                            llm_events.push(TranscriptEvent {
                                event_type: "llm_final",
                                channel: result.request.channel.clone(),
                                segment_id: format!("{source_segment_id}-llm"),
                                start_ms: result.request.start_ms,
                                end_ms: result.request.end_ms,
                                text: cleaned_text,
                                source_final_segment_id: Some(source_segment_id),
                            });
                        }
                    }
                    CleanupTaskStatus::TimedOut => telemetry.timed_out += 1,
                    CleanupTaskStatus::Failed => telemetry.failed += 1,
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    telemetry.pending = telemetry.enqueued.saturating_sub(telemetry.processed);
    telemetry.drain_completed = telemetry.pending == 0;
    if telemetry.drain_completed {
        let _ = worker_handle.join();
    }
    CleanupRunResult {
        telemetry,
        llm_events,
    }
}

fn cleanup_requests_from_events(events: &[TranscriptEvent]) -> Vec<CleanupRequest> {
    events
        .iter()
        .filter(|event| event.event_type == "final")
        .map(|event| CleanupRequest {
            segment_id: event.segment_id.clone(),
            channel: event.channel.clone(),
            start_ms: event.start_ms,
            end_ms: event.end_ms,
            text: event.text.clone(),
        })
        .collect()
}

fn cleanup_worker_loop<F>(
    request_rx: Receiver<CleanupRequest>,
    result_tx: mpsc::Sender<CleanupTaskResult>,
    client: CleanupClientConfig,
    invoke_cleanup: Arc<F>,
) where
    F: Fn(&CleanupClientConfig, &CleanupRequest) -> CleanupAttemptOutcome + Send + Sync + 'static,
{
    while let Ok(request) = request_rx.recv() {
        let mut status = CleanupTaskStatus::Failed;
        let mut retry_attempts = 0usize;
        let mut cleaned_text = None;

        for attempt in 0..=client.retries {
            let outcome = invoke_cleanup(&client, &request);
            status = outcome.status;
            cleaned_text = outcome.cleaned_text;
            if status == CleanupTaskStatus::Succeeded {
                break;
            }
            if attempt < client.retries {
                retry_attempts += 1;
            }
        }

        if result_tx
            .send(CleanupTaskResult {
                request,
                status,
                retry_attempts,
                cleaned_text,
            })
            .is_err()
        {
            break;
        }
    }
}

fn invoke_cleanup_endpoint(
    client: &CleanupClientConfig,
    request: &CleanupRequest,
) -> CleanupAttemptOutcome {
    let timeout_secs = format!("{:.3}", (client.timeout_ms as f64 / 1_000.0).max(0.001));
    let prompt = format!(
        "Polish this transcript segment for readability without changing meaning. Return only cleaned text.\nsegment_id={}\nchannel={}\ntext={}",
        request.segment_id, request.channel, request.text
    );
    let payload = format!(
        "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}],\"stream\":false}}",
        json_escape(&client.model),
        json_escape("You clean transcript text. Do not add content."),
        json_escape(&prompt)
    );

    let output = Command::new("curl")
        .arg("-sS")
        .arg("--fail-with-body")
        .arg("--max-time")
        .arg(&timeout_secs)
        .arg("-X")
        .arg("POST")
        .arg(&client.endpoint)
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(&payload)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let cleaned_text = cleanup_content_from_response(&stdout)
                .or_else(|| {
                    if stdout.is_empty() {
                        None
                    } else {
                        Some(stdout.clone())
                    }
                })
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            match cleaned_text {
                Some(text) => CleanupAttemptOutcome {
                    status: CleanupTaskStatus::Succeeded,
                    cleaned_text: Some(text),
                },
                None => CleanupAttemptOutcome {
                    status: CleanupTaskStatus::Failed,
                    cleaned_text: None,
                },
            }
        }
        Ok(output) if output.status.code() == Some(28) => CleanupAttemptOutcome {
            status: CleanupTaskStatus::TimedOut,
            cleaned_text: None,
        },
        Ok(_) => CleanupAttemptOutcome {
            status: CleanupTaskStatus::Failed,
            cleaned_text: None,
        },
        Err(_) => CleanupAttemptOutcome {
            status: CleanupTaskStatus::Failed,
            cleaned_text: None,
        },
    }
}

pub(super) fn cleanup_content_from_response(stdout: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(stdout).ok()?;
    find_first_json_string_field(&parsed, "content")
}
