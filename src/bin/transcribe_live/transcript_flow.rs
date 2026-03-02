use super::*;

pub(super) fn merge_transcript_events(mut events: Vec<TranscriptEvent>) -> Vec<TranscriptEvent> {
    events.sort_by(|a, b| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| a.end_ms.cmp(&b.end_ms))
            .then_with(|| event_type_rank(a.event_type).cmp(&event_type_rank(b.event_type)))
            .then_with(|| a.channel.cmp(&b.channel))
            .then_with(|| a.segment_id.cmp(&b.segment_id))
            .then_with(|| a.source_final_segment_id.cmp(&b.source_final_segment_id))
            .then_with(|| a.text.cmp(&b.text))
    });
    events
}

pub(super) fn event_type_rank(event_type: &str) -> u8 {
    match event_type {
        "partial" => 0,
        "final" => 1,
        "reconciled_final" => 2,
        "llm_final" => 3,
        _ => 4,
    }
}

pub(super) fn reconstruct_transcript(events: &[TranscriptEvent]) -> String {
    let finals = final_events_for_display(events);
    let mut lines = Vec::new();
    let mut previous: Option<&TranscriptEvent> = None;
    for event in finals {
        let text = event.text.trim();
        if text.is_empty() {
            continue;
        }
        let overlap_suffix = if let Some(prev) = previous {
            if has_near_simultaneous_overlap(prev, event) {
                format!(" (overlap<={OVERLAP_WINDOW_MS}ms with {})", prev.channel)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        lines.push(format!(
            "[{}-{}] {}: {}{}",
            format_timestamp(event.start_ms),
            format_timestamp(event.end_ms),
            event.channel,
            text,
            overlap_suffix
        ));
        previous = Some(event);
    }

    if lines.is_empty() {
        "<no speech detected>".to_string()
    } else {
        lines.join("\n")
    }
}

pub(super) fn reconstruct_transcript_per_channel(
    events: &[TranscriptEvent],
) -> Vec<ReadableChannelTranscript> {
    let finals = final_events_for_display(events);
    let mut channels = finals
        .iter()
        .map(|event| event.channel.clone())
        .collect::<Vec<_>>();
    channels.sort_by(|a, b| {
        channel_display_sort_key(a)
            .cmp(&channel_display_sort_key(b))
            .then_with(|| a.cmp(b))
    });
    channels.dedup();

    channels
        .into_iter()
        .filter_map(|channel| {
            let mut lines = Vec::new();
            for event in finals.iter().filter(|event| event.channel == channel) {
                let text = event.text.trim();
                if text.is_empty() {
                    continue;
                }
                lines.push(format!(
                    "[{}-{}] {}",
                    format_timestamp(event.start_ms),
                    format_timestamp(event.end_ms),
                    text
                ));
            }
            if lines.is_empty() {
                None
            } else {
                Some(ReadableChannelTranscript {
                    channel,
                    text: lines.join("\n"),
                })
            }
        })
        .collect()
}

pub(super) fn final_events_for_display<'a>(
    events: &'a [TranscriptEvent],
) -> Vec<&'a TranscriptEvent> {
    let has_reconciled = events
        .iter()
        .any(|event| event.event_type == "reconciled_final");
    let display_event_type = if has_reconciled {
        "reconciled_final"
    } else {
        "final"
    };
    let mut finals = events
        .iter()
        .filter(|event| event.event_type == display_event_type)
        .collect::<Vec<_>>();
    finals.sort_by(|a, b| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| a.end_ms.cmp(&b.end_ms))
            .then_with(|| {
                channel_display_sort_key(&a.channel).cmp(&channel_display_sort_key(&b.channel))
            })
            .then_with(|| a.channel.cmp(&b.channel))
            .then_with(|| a.segment_id.cmp(&b.segment_id))
            .then_with(|| a.source_final_segment_id.cmp(&b.source_final_segment_id))
            .then_with(|| a.text.cmp(&b.text))
    });
    finals
}

pub(super) fn has_near_simultaneous_overlap(
    previous: &TranscriptEvent,
    current: &TranscriptEvent,
) -> bool {
    previous.channel != current.channel
        && current.start_ms.saturating_sub(previous.start_ms) <= OVERLAP_WINDOW_MS
}

pub(super) fn channel_display_sort_key(channel: &str) -> u8 {
    match channel {
        "mic" => 0,
        "system" => 1,
        "merged" => 2,
        _ => 3,
    }
}

pub(super) fn format_timestamp(ms: u64) -> String {
    let total_seconds = ms / 1_000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let millis = ms % 1_000;
    format!("{minutes:02}:{seconds:02}.{millis:03}")
}

pub(super) fn terminal_render_mode() -> TerminalRenderMode {
    if std::io::stdout().is_terminal() {
        TerminalRenderMode::InteractiveTty
    } else {
        TerminalRenderMode::DeterministicNonTty
    }
}

pub(super) fn stable_event_suffix(event_type: &str) -> &'static str {
    match event_type {
        "llm_final" => " [llm_final]",
        "reconciled_final" => " [reconciled_final]",
        _ => "",
    }
}

pub(super) fn format_stable_transcript_line(event: &TranscriptEvent) -> Option<String> {
    let cleaned = event.text.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(format!(
        "[{}-{}] {}: {}{}",
        format_timestamp(event.start_ms),
        format_timestamp(event.end_ms),
        event.channel,
        cleaned,
        stable_event_suffix(event.event_type)
    ))
}

pub(super) fn format_partial_transcript_line(event: &TranscriptEvent) -> Option<String> {
    let cleaned = event.text.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(format!(
        "[{}-{}] {} ~ {}",
        format_timestamp(event.start_ms),
        format_timestamp(event.end_ms),
        event.channel,
        cleaned
    ))
}

pub(super) fn is_stable_terminal_event(event_type: &str) -> bool {
    matches!(event_type, "final" | "llm_final" | "reconciled_final")
}

pub(super) fn build_terminal_render_actions(
    events: &[TranscriptEvent],
    mode: TerminalRenderMode,
) -> Vec<TerminalRenderAction> {
    match mode {
        TerminalRenderMode::DeterministicNonTty => events
            .iter()
            .filter(|event| is_stable_terminal_event(event.event_type))
            .filter_map(format_stable_transcript_line)
            .map(|line| TerminalRenderAction {
                kind: TerminalRenderActionKind::StableLine,
                line,
            })
            .collect(),
        TerminalRenderMode::InteractiveTty => {
            let mut actions = Vec::new();
            let mut last_partial_by_segment = HashMap::<(String, String), String>::new();
            for event in events {
                match event.event_type {
                    "partial" => {
                        let key = (event.channel.clone(), event.segment_id.clone());
                        let Some(line) = format_partial_transcript_line(event) else {
                            continue;
                        };
                        if last_partial_by_segment.get(&key) == Some(&line) {
                            continue;
                        }
                        last_partial_by_segment.insert(key, line.clone());
                        actions.push(TerminalRenderAction {
                            kind: TerminalRenderActionKind::PartialOverwrite,
                            line,
                        });
                    }
                    "final" | "llm_final" | "reconciled_final" => {
                        last_partial_by_segment
                            .remove(&(event.channel.clone(), event.segment_id.clone()));
                        let Some(line) = format_stable_transcript_line(event) else {
                            continue;
                        };
                        actions.push(TerminalRenderAction {
                            kind: TerminalRenderActionKind::StableLine,
                            line,
                        });
                    }
                    _ => {}
                }
            }
            actions
        }
    }
}

pub(super) fn live_terminal_render_actions(
    config: &TranscribeConfig,
    events: &[TranscriptEvent],
    mode: TerminalRenderMode,
) -> Vec<TerminalRenderAction> {
    if !(config.live_chunked || config.live_stream) {
        return Vec::new();
    }
    build_terminal_render_actions(events, mode)
}

pub(super) fn emit_terminal_render_actions(
    actions: &[TerminalRenderAction],
    mode: TerminalRenderMode,
) {
    if actions.is_empty() {
        return;
    }
    let mut stdout = std::io::stdout();
    match mode {
        TerminalRenderMode::DeterministicNonTty => {
            for action in actions
                .iter()
                .filter(|action| action.kind == TerminalRenderActionKind::StableLine)
            {
                let _ = writeln!(stdout, "{}", action.line);
            }
        }
        TerminalRenderMode::InteractiveTty => {
            let mut partial_visible = false;
            for action in actions {
                match action.kind {
                    TerminalRenderActionKind::PartialOverwrite => {
                        let _ = write!(stdout, "\r\x1b[2K{}", action.line);
                        let _ = stdout.flush();
                        partial_visible = true;
                    }
                    TerminalRenderActionKind::StableLine => {
                        if partial_visible {
                            let _ = write!(stdout, "\r\x1b[2K");
                            partial_visible = false;
                        }
                        let _ = writeln!(stdout, "{}", action.line);
                    }
                }
            }
            if partial_visible {
                let _ = writeln!(stdout);
            }
        }
    }
}

pub(super) fn maybe_emit_live_terminal_stream(
    config: &TranscribeConfig,
    events: &[TranscriptEvent],
) {
    let mode = terminal_render_mode();
    let actions = live_terminal_render_actions(config, events, mode);
    emit_terminal_render_actions(&actions, mode);
}

pub(super) fn build_trust_notices(
    requested_mode: ChannelMode,
    active_mode: ChannelMode,
    degradation_events: &[ModeDegradationEvent],
    cleanup_queue: &CleanupQueueTelemetry,
    chunk_queue: &LiveChunkQueueTelemetry,
) -> Vec<TrustNotice> {
    let mut notices = Vec::new();

    for degradation in degradation_events {
        match degradation.code {
            "fallback_to_mixed" => {
                notices.push(TrustNotice {
                    code: "mode_degradation".to_string(),
                    severity: "warn".to_string(),
                    cause: degradation.detail.clone(),
                    impact: if requested_mode != active_mode {
                        format!(
                            "requested channel mode `{requested_mode}` degraded to `{active_mode}`; transcript attribution and separation guarantees are reduced"
                        )
                    } else {
                        "runtime entered degraded channel mode".to_string()
                    },
                    guidance: "Use `--transcribe-channels separate` with a stereo input fixture to restore channel-level attribution.".to_string(),
                });
            }
            LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE => {
                notices.push(TrustNotice {
                    code: "continuity_recovered_with_gaps".to_string(),
                    severity: "warn".to_string(),
                    cause: degradation.detail.clone(),
                    impact:
                        "capture continuity was preserved via bounded restart recovery, but transcript timing/content may contain interruption boundaries".to_string(),
                    guidance:
                        "Inspect continuity telemetry and runtime timeline before treating this session as gap-free.".to_string(),
                });
            }
            LIVE_CAPTURE_CONTINUITY_UNVERIFIED_CODE => {
                notices.push(TrustNotice {
                    code: "continuity_unverified".to_string(),
                    severity: "warn".to_string(),
                    cause: degradation.detail.clone(),
                    impact:
                        "near-live continuity guarantees cannot be fully confirmed for this session".to_string(),
                    guidance:
                        "Ensure capture telemetry is writable/readable and rerun the session to verify interruption recovery state.".to_string(),
                });
            }
            LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE => {
                notices.push(TrustNotice {
                    code: "capture_transport_degraded".to_string(),
                    severity: "warn".to_string(),
                    cause: degradation.detail.clone(),
                    impact:
                        "capture transport reported drop/failure signals; near-live completeness may be reduced".to_string(),
                    guidance:
                        "Inspect capture telemetry transport counters and reduce capture pressure before treating this session as fully gap-free.".to_string(),
                });
            }
            LIVE_CAPTURE_CALLBACK_CONTRACT_DEGRADED_CODE => {
                notices.push(TrustNotice {
                    code: "capture_callback_contract_degraded".to_string(),
                    severity: "warn".to_string(),
                    cause: degradation.detail.clone(),
                    impact:
                        "capture callback contract violations were detected; audio continuity/fidelity may be affected".to_string(),
                    guidance:
                        "Review callback-contract counters in capture telemetry and validate host capture configuration before relying on strict completeness.".to_string(),
                });
            }
            LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE => {
                notices.push(TrustNotice {
                    code: "chunk_queue_backpressure".to_string(),
                    severity: "warn".to_string(),
                    cause: degradation.detail.clone(),
                    impact:
                        "near-live chunk backlog exceeded queue capacity; some oldest chunk tasks were dropped to keep producer non-blocking".to_string(),
                    guidance: format!(
                        "Increase `--chunk-queue-cap` (current={}) or reduce near-live load to lower backlog pressure.",
                        chunk_queue.max_queue
                    ),
                });
            }
            LIVE_CHUNK_QUEUE_BACKPRESSURE_SEVERE_CODE => {
                notices.push(TrustNotice {
                    code: "chunk_queue_backpressure_severe".to_string(),
                    severity: "error".to_string(),
                    cause: degradation.detail.clone(),
                    impact:
                        "near-live queue pressure is sustained; incremental transcript fidelity and timeliness are materially reduced".to_string(),
                    guidance: format!(
                        "Increase `--chunk-queue-cap` (current={}), reduce capture/load pressure, or switch to offline/reconciled artifact review for canonical completeness.",
                        chunk_queue.max_queue
                    ),
                });
            }
            RECONCILIATION_APPLIED_CODE => {
                notices.push(TrustNotice {
                    code: "reconciliation_applied".to_string(),
                    severity: "warn".to_string(),
                    cause: degradation.detail.clone(),
                    impact:
                        "post-session reconciliation ran to stabilize canonical completeness under one or more live degradation triggers".to_string(),
                    guidance:
                        "Use `reconciled_final` events as canonical output and inspect `reconciliation_matrix` trigger codes for the root cause path.".to_string(),
                });
            }
            _ => {
                notices.push(TrustNotice {
                    code: degradation.code.to_string(),
                    severity: "warn".to_string(),
                    cause: degradation.detail.clone(),
                    impact: "runtime entered degraded mode".to_string(),
                    guidance: "Inspect degradation details and rerun with recommended defaults."
                        .to_string(),
                });
            }
        }
    }

    if cleanup_queue.enabled {
        if cleanup_queue.dropped_queue_full > 0 {
            notices.push(TrustNotice {
                code: "cleanup_queue_drop".to_string(),
                severity: "warn".to_string(),
                cause: format!(
                    "{} cleanup request(s) dropped due to full queue",
                    cleanup_queue.dropped_queue_full
                ),
                impact:
                    "some `llm_final` readability refinements are missing; raw `final` transcript remains canonical"
                        .to_string(),
                guidance:
                    "Increase `--llm-max-queue`, reduce cleanup load, or disable cleanup for strict throughput runs."
                        .to_string(),
            });
        }

        if cleanup_queue.timed_out > 0 || cleanup_queue.failed > 0 {
            notices.push(TrustNotice {
                code: "cleanup_processing_failure".to_string(),
                severity: "warn".to_string(),
                cause: format!(
                    "cleanup failures detected (timed_out={}, failed={})",
                    cleanup_queue.timed_out, cleanup_queue.failed
                ),
                impact:
                    "cleanup outputs may be incomplete or absent; rely on `final` events for authoritative transcript text"
                        .to_string(),
                guidance:
                    "Validate cleanup endpoint/model health or run with `--llm-cleanup` disabled for deterministic core transcripts."
                        .to_string(),
            });
        }

        if !cleanup_queue.drain_completed || cleanup_queue.pending > 0 {
            notices.push(TrustNotice {
                code: "cleanup_drain_incomplete".to_string(),
                severity: "warn".to_string(),
                cause: format!(
                    "cleanup drain incomplete (pending={}, drain_completed={})",
                    cleanup_queue.pending, cleanup_queue.drain_completed
                ),
                impact:
                    "session ended before all queued cleanup work finished; readability post-processing is partial"
                        .to_string(),
                guidance: "Increase `--llm-timeout-ms` or reduce workload to allow cleanup drain completion."
                    .to_string(),
            });
        }
    }

    notices
}
