use std::collections::BTreeSet;

use super::{
    build_terminal_render_actions, display_path, input_wav_semantics, model_checksum_info,
    reconstruct_transcript_per_channel, BackpressureTransitionReason, ChannelPressureSnapshot,
    LiveRunReport, ResolvedModelPath, TerminalRenderActionKind, TerminalRenderMode,
    TranscriptEvent, TranscribeConfig, OUT_WAV_SEMANTICS, OVERLAP_WINDOW_MS,
};

pub(super) fn stable_terminal_summary_lines(events: &[TranscriptEvent]) -> Vec<String> {
    build_terminal_render_actions(events, TerminalRenderMode::DeterministicNonTty)
        .into_iter()
        .filter(|action| action.kind == TerminalRenderActionKind::StableLine)
        .map(|action| action.line)
        .collect()
}

pub(super) fn transcript_event_count(events: &[TranscriptEvent], event_type: &str) -> usize {
    events
        .iter()
        .filter(|event| event.event_type == event_type)
        .count()
}

fn backpressure_reason_label(reason: Option<BackpressureTransitionReason>) -> &'static str {
    reason
        .map(BackpressureTransitionReason::as_str)
        .unwrap_or("none")
}

fn channel_pressure_snapshots_csv(snapshots: &[ChannelPressureSnapshot]) -> String {
    if snapshots.is_empty() {
        return "<none>".to_string();
    }
    snapshots
        .iter()
        .map(|snapshot| {
            format!(
                "{}{{processed:{} partial:{} stable:{} pending_est:{} dropped_est:{}}}",
                snapshot.channel,
                snapshot.processed_events,
                snapshot.partial_events,
                snapshot.stable_events,
                snapshot.pending_estimate,
                snapshot.dropped_oldest_estimate
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub(super) fn runtime_failure_breadcrumbs(
    config: &TranscribeConfig,
    report: &LiveRunReport,
) -> String {
    format!(
        "artifacts[out_wav:{} out_jsonl:{} out_manifest:{}] transport[path:{} pcm_window:{}] scratch[writes_est:{} reuse_overwrites_est:{} retained_hint:{}] backpressure[mode:{} transitions:{} last_reason:{} pending_jobs:{} pending_final_jobs:{}] pump[chunk_decisions:{} forced_decisions:{}]",
        display_path(&config.out_wav),
        display_path(&config.out_jsonl),
        display_path(&config.out_manifest),
        report.hot_path_diagnostics.transport.path_requests,
        report.hot_path_diagnostics.transport.pcm_window_requests,
        report.hot_path_diagnostics.scratch.write_attempts_estimate,
        report.hot_path_diagnostics.scratch.reuse_overwrites_estimate,
        report.hot_path_diagnostics.scratch.retained_for_review_hint,
        report.hot_path_diagnostics.backpressure.mode.as_str(),
        report.hot_path_diagnostics.backpressure.transition_count,
        backpressure_reason_label(report.hot_path_diagnostics.backpressure.last_transition_reason),
        report.hot_path_diagnostics.backpressure.pending_jobs,
        report.hot_path_diagnostics.backpressure.pending_final_jobs,
        report.hot_path_diagnostics.pump.chunk_decisions,
        report.hot_path_diagnostics.pump.forced_decisions
    )
}

pub(super) fn session_status(report: &LiveRunReport) -> &'static str {
    if report.trust_notices.is_empty() {
        "ok"
    } else {
        "degraded"
    }
}

pub(super) fn top_codes<I>(codes: I, limit: usize) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut sorted = codes.into_iter().collect::<Vec<_>>();
    sorted.sort();
    sorted.dedup();
    sorted.into_iter().take(limit).collect()
}

fn top_codes_csv(codes: &[String]) -> String {
    if codes.is_empty() {
        "<none>".to_string()
    } else {
        codes.join("|")
    }
}

pub(super) fn top_remediation_hints(report: &LiveRunReport, limit: usize) -> Vec<String> {
    let mut hints = BTreeSet::new();
    for notice in &report.trust_notices {
        let guidance = notice.guidance.trim();
        if !guidance.is_empty() {
            hints.insert(guidance.to_string());
        }
    }

    if hints.is_empty() && !report.degradation_events.is_empty() {
        for event in &report.degradation_events {
            hints.insert(format!(
                "inspect degradation code `{}` in the runtime manifest",
                event.code
            ));
            if hints.len() >= limit {
                break;
            }
        }
    }

    hints.into_iter().take(limit).collect()
}

pub(super) fn remediation_hints_csv(hints: &[String]) -> String {
    if hints.is_empty() {
        "<none>".to_string()
    } else {
        hints.join(" | ")
    }
}

pub(super) fn build_live_close_summary_lines(
    config: &TranscribeConfig,
    report: &LiveRunReport,
) -> Vec<String> {
    let trust_top_codes = top_codes(
        report
            .trust_notices
            .iter()
            .map(|notice| notice.code.clone()),
        3,
    );
    let degradation_top_codes = top_codes(
        report
            .degradation_events
            .iter()
            .map(|event| event.code.to_string()),
        3,
    );

    vec![
        format!("session_status={}", session_status(report)),
        format!("duration_sec={}", config.duration_sec),
        format!("channel_mode_requested={}", report.channel_mode),
        format!("channel_mode_active={}", report.active_channel_mode),
        format!(
            "transcript_events=partial:{} final:{} llm_final:{} reconciled_final:{}",
            transcript_event_count(&report.events, "partial"),
            transcript_event_count(&report.events, "final"),
            transcript_event_count(&report.events, "llm_final"),
            transcript_event_count(&report.events, "reconciled_final"),
        ),
        format!(
            "chunk_queue=submitted:{} enqueued:{} dropped_oldest:{} processed:{} pending:{} high_water:{} drain_completed:{}",
            report.chunk_queue.submitted,
            report.chunk_queue.enqueued,
            report.chunk_queue.dropped_oldest,
            report.chunk_queue.processed,
            report.chunk_queue.pending,
            report.chunk_queue.high_water,
            report.chunk_queue.drain_completed,
        ),
        format!(
            "chunk_lag=lag_sample_count:{} lag_p50_ms:{} lag_p95_ms:{} lag_max_ms:{}",
            report.chunk_queue.lag_sample_count,
            report.chunk_queue.lag_p50_ms,
            report.chunk_queue.lag_p95_ms,
            report.chunk_queue.lag_max_ms,
        ),
        format!(
            "trust_notices=count:{} top_codes:{}",
            report.trust_notices.len(),
            top_codes_csv(&trust_top_codes),
        ),
        format!(
            "degradation_events=count:{} top_codes:{}",
            report.degradation_events.len(),
            top_codes_csv(&degradation_top_codes),
        ),
        format!(
            "cleanup_queue=enabled:{} submitted:{} enqueued:{} dropped_queue_full:{} processed:{} succeeded:{} timed_out:{} failed:{} retry_attempts:{} pending:{} drain_completed:{}",
            report.cleanup_queue.enabled,
            report.cleanup_queue.submitted,
            report.cleanup_queue.enqueued,
            report.cleanup_queue.dropped_queue_full,
            report.cleanup_queue.processed,
            report.cleanup_queue.succeeded,
            report.cleanup_queue.timed_out,
            report.cleanup_queue.failed,
            report.cleanup_queue.retry_attempts,
            report.cleanup_queue.pending,
            report.cleanup_queue.drain_completed,
        ),
        format!(
            "diagnostics_transport=path:{} pcm_window:{}",
            report.hot_path_diagnostics.transport.path_requests,
            report.hot_path_diagnostics.transport.pcm_window_requests
        ),
        format!(
            "diagnostics_scratch=worker_paths_max:{} writes_est:{} reuse_overwrites_est:{} retained_for_review_hint:{}",
            report
                .hot_path_diagnostics
                .scratch
                .worker_scratch_paths_upper_bound,
            report.hot_path_diagnostics.scratch.write_attempts_estimate,
            report.hot_path_diagnostics.scratch.reuse_overwrites_estimate,
            report.hot_path_diagnostics.scratch.retained_for_review_hint
        ),
        format!(
            "diagnostics_backpressure=mode:{} transitions:{} last_reason:{} pending_jobs:{} pending_final_jobs:{} per_channel:{}",
            report.hot_path_diagnostics.backpressure.mode.as_str(),
            report.hot_path_diagnostics.backpressure.transition_count,
            backpressure_reason_label(report.hot_path_diagnostics.backpressure.last_transition_reason),
            report.hot_path_diagnostics.backpressure.pending_jobs,
            report.hot_path_diagnostics.backpressure.pending_final_jobs,
            channel_pressure_snapshots_csv(&report.hot_path_diagnostics.backpressure.channel_snapshots)
        ),
        format!(
            "diagnostics_pump=chunk_decisions:{} forced_decisions:{} forced_capture_event_triggers:{} forced_shutdown_triggers:{}",
            report.hot_path_diagnostics.pump.chunk_decisions,
            report.hot_path_diagnostics.pump.forced_decisions,
            report.hot_path_diagnostics.pump.forced_capture_event_triggers,
            report.hot_path_diagnostics.pump.forced_shutdown_triggers
        ),
        format!(
            "artifacts=out_wav:{} out_jsonl:{} out_manifest:{}",
            display_path(&config.out_wav),
            display_path(&config.out_jsonl),
            display_path(&config.out_manifest),
        ),
    ]
}

pub(super) fn print_live_report(
    config: &TranscribeConfig,
    report: &LiveRunReport,
    concise_only: bool,
) {
    let model_checksum = model_checksum_info(Some(&ResolvedModelPath {
        path: report.resolved_model_path.clone(),
        source: report.resolved_model_source.clone(),
    }));
    let remediation_hints = top_remediation_hints(report, 3);

    println!();
    println!("Runtime result");
    println!("  runtime_mode: {}", config.runtime_mode_label());
    println!(
        "  runtime_mode_taxonomy: {}",
        config.runtime_mode_taxonomy_label()
    );
    println!(
        "  runtime_mode_selector: {}",
        config.runtime_mode_selector_label()
    );
    println!(
        "  runtime_mode_status: {}",
        config.runtime_mode_status_label()
    );
    println!("  generated_at_utc: {}", report.generated_at_utc);
    println!("  backend: {}", report.backend_id);
    println!(
        "  asr_model_resolved: {}",
        report.resolved_model_path.display()
    );
    println!("  asr_model_source: {}", report.resolved_model_source);
    println!("  asr_model_checksum_sha256: {}", model_checksum.sha256);
    println!("  asr_model_checksum_status: {}", model_checksum.status);
    println!("  run_status: {}", session_status(report));
    println!(
        "  remediation_hints: {}",
        remediation_hints_csv(&remediation_hints)
    );
    println!("  channel_mode_requested: {}", report.channel_mode);
    println!("  channel_mode_active: {}", report.active_channel_mode);
    println!("  close_summary:");
    for line in build_live_close_summary_lines(config, report) {
        println!("    {line}");
    }
    println!(
        "  diagnostics_breadcrumbs: {}",
        runtime_failure_breadcrumbs(config, report)
    );
    if concise_only {
        println!(
            "  telemetry_manifest: {}",
            display_path(&config.out_manifest)
        );
        println!("  telemetry_jsonl: {}", display_path(&config.out_jsonl));
        return;
    }

    let per_channel_defaults = reconstruct_transcript_per_channel(&report.events);
    println!(
        "  lifecycle: current_phase={} ready_for_transcripts={} transition_count={}",
        report.lifecycle.current_phase,
        report.lifecycle.ready_for_transcripts,
        report.lifecycle.transitions.len()
    );
    println!("  lifecycle_transitions:");
    for transition in &report.lifecycle.transitions {
        println!(
            "    - phase={} entered_at_utc={} detail={}",
            transition.phase, transition.entered_at_utc, transition.detail
        );
    }
    println!("  transcript_default_line_format: [MM:SS.mmm-MM:SS.mmm] <channel>: <text>");
    println!(
        "  transcript_overlap_policy: adjacent cross-channel finals within {OVERLAP_WINDOW_MS}ms keep sort order and add overlap annotation"
    );
    println!("  transcript_text:");
    for line in report.transcript_text.lines() {
        println!("    {line}");
    }
    println!("  transcript_per_channel:");
    for channel in &per_channel_defaults {
        println!("    - channel={}", channel.channel);
        for line in channel.text.lines() {
            println!("      {line}");
        }
    }
    println!("  channel_transcripts:");
    for channel in &report.channel_transcripts {
        println!(
            "    - role={} label={} text={}",
            channel.role, channel.label, channel.text
        );
    }
    println!(
        "  benchmark_wall_ms: p50={:.2} p95={:.2} (runs={})",
        report.benchmark.wall_ms_p50, report.benchmark.wall_ms_p95, report.benchmark.run_count
    );
    println!(
        "  slo_check: partial_p95<=1500ms={} final_p95<=2500ms={}",
        report.benchmark.partial_slo_met, report.benchmark.final_slo_met
    );
    println!(
        "  benchmark_summary_csv: {}",
        report.benchmark_summary_csv.display()
    );
    println!(
        "  benchmark_runs_csv: {}",
        report.benchmark_runs_csv.display()
    );
    println!("  input_wav_semantics: {}", input_wav_semantics(config));
    println!("  out_wav_semantics: {OUT_WAV_SEMANTICS}");
    println!("  vad_boundaries: {}", report.vad_boundaries.len());
    for boundary in &report.vad_boundaries {
        println!(
            "    - id={} start_ms={} end_ms={} source={}",
            boundary.id, boundary.start_ms, boundary.end_ms, boundary.source
        );
    }
    println!("  terminal_transcript_stream:");
    let live_mode = report.chunk_queue.enabled;
    let stable_terminal_lines = stable_terminal_summary_lines(&report.events);
    if live_mode {
        println!(
            "    <rendered during active runtime; summary suppresses duplicate stable-line replay>"
        );
    } else if stable_terminal_lines.is_empty() {
        println!("    <no stable transcript events>");
    } else {
        for line in stable_terminal_lines {
            println!("    {line}");
        }
    }
    println!("  degradation_events: {}", report.degradation_events.len());
    for event in &report.degradation_events {
        println!("    - code={} detail={}", event.code, event.detail);
    }
    println!("  trust_notices: {}", report.trust_notices.len());
    if report.trust_notices.is_empty() {
        println!("    - none (runtime trust posture: nominal)");
    } else {
        println!("  degraded_mode_notices:");
        for notice in &report.trust_notices {
            println!(
                "    - [{}] code={} cause={} | impact={} | next={}",
                notice.severity, notice.code, notice.cause, notice.impact, notice.guidance
            );
        }
    }
    println!(
        "  reconciliation_matrix: required={} applied={} trigger_count={} trigger_codes={}",
        report.reconciliation.required,
        report.reconciliation.applied,
        report.reconciliation.triggers.len(),
        report.reconciliation.trigger_codes_csv()
    );
    println!(
        "  asr_worker_pool: prewarm_ok={} submitted={} enqueued={} dropped_queue_full={} processed={} succeeded={} failed={} retry_attempts={} temp_audio_deleted={} temp_audio_retained={}",
        report.asr_worker_pool.prewarm_ok,
        report.asr_worker_pool.submitted,
        report.asr_worker_pool.enqueued,
        report.asr_worker_pool.dropped_queue_full,
        report.asr_worker_pool.processed,
        report.asr_worker_pool.succeeded,
        report.asr_worker_pool.failed,
        report.asr_worker_pool.retry_attempts,
        report.asr_worker_pool.temp_audio_deleted,
        report.asr_worker_pool.temp_audio_retained
    );
    println!(
        "  chunk_queue: enabled={} max_queue={} submitted={} enqueued={} dropped_oldest={} processed={} pending={} high_water={} drain_completed={} lag_sample_count={} lag_p50_ms={} lag_p95_ms={} lag_max_ms={}",
        report.chunk_queue.enabled,
        report.chunk_queue.max_queue,
        report.chunk_queue.submitted,
        report.chunk_queue.enqueued,
        report.chunk_queue.dropped_oldest,
        report.chunk_queue.processed,
        report.chunk_queue.pending,
        report.chunk_queue.high_water,
        report.chunk_queue.drain_completed,
        report.chunk_queue.lag_sample_count,
        report.chunk_queue.lag_p50_ms,
        report.chunk_queue.lag_p95_ms,
        report.chunk_queue.lag_max_ms
    );
    println!(
        "  hot_path_transport: request_input_path={} request_input_pcm_window={}",
        report.hot_path_diagnostics.transport.path_requests,
        report.hot_path_diagnostics.transport.pcm_window_requests
    );
    println!(
        "  hot_path_scratch: worker_paths_max={} writes_est={} reuse_overwrites_est={} retained_for_review_hint={}",
        report
            .hot_path_diagnostics
            .scratch
            .worker_scratch_paths_upper_bound,
        report.hot_path_diagnostics.scratch.write_attempts_estimate,
        report.hot_path_diagnostics.scratch.reuse_overwrites_estimate,
        report.hot_path_diagnostics.scratch.retained_for_review_hint
    );
    println!(
        "  hot_path_backpressure: mode={} transitions={} last_reason={} pending_jobs={} pending_final_jobs={}",
        report.hot_path_diagnostics.backpressure.mode.as_str(),
        report.hot_path_diagnostics.backpressure.transition_count,
        backpressure_reason_label(report.hot_path_diagnostics.backpressure.last_transition_reason),
        report.hot_path_diagnostics.backpressure.pending_jobs,
        report.hot_path_diagnostics.backpressure.pending_final_jobs
    );
    println!("  hot_path_backpressure_channels:");
    if report
        .hot_path_diagnostics
        .backpressure
        .channel_snapshots
        .is_empty()
    {
        println!("    - <none>");
    } else {
        for snapshot in &report.hot_path_diagnostics.backpressure.channel_snapshots {
            println!(
                "    - channel={} processed={} partial={} stable={} pending_est={} dropped_est={}",
                snapshot.channel,
                snapshot.processed_events,
                snapshot.partial_events,
                snapshot.stable_events,
                snapshot.pending_estimate,
                snapshot.dropped_oldest_estimate
            );
        }
    }
    println!(
        "  hot_path_pump: chunk_decisions={} forced_decisions={} forced_capture_event_triggers={} forced_shutdown_triggers={}",
        report.hot_path_diagnostics.pump.chunk_decisions,
        report.hot_path_diagnostics.pump.forced_decisions,
        report.hot_path_diagnostics.pump.forced_capture_event_triggers,
        report.hot_path_diagnostics.pump.forced_shutdown_triggers
    );
    println!(
        "  cleanup_queue: enabled={} submitted={} enqueued={} dropped_queue_full={} processed={} succeeded={} timed_out={} failed={} retry_attempts={} pending={} drain_completed={}",
        report.cleanup_queue.enabled,
        report.cleanup_queue.submitted,
        report.cleanup_queue.enqueued,
        report.cleanup_queue.dropped_queue_full,
        report.cleanup_queue.processed,
        report.cleanup_queue.succeeded,
        report.cleanup_queue.timed_out,
        report.cleanup_queue.failed,
        report.cleanup_queue.retry_attempts,
        report.cleanup_queue.pending,
        report.cleanup_queue.drain_completed
    );
    println!("  jsonl_written: true");
    println!("  manifest_written: true");
}
