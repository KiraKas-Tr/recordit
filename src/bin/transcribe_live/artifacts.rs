use super::*;

pub(super) fn emit_latest_lifecycle_transition_jsonl(
    stream: &mut RuntimeJsonlStream,
    lifecycle: &LiveLifecycleTelemetry,
) -> Result<(), CliError> {
    let Some((index, transition)) = lifecycle.transitions.iter().enumerate().last() else {
        return Ok(());
    };
    stream.write_line(&jsonl_lifecycle_phase_line(index, transition))?;
    stream.checkpoint()?;
    Ok(())
}

pub(super) fn ensure_runtime_jsonl_parent(path: &Path) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CliError::new(format!(
                "failed to create JSONL directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}

pub(super) fn jsonl_vad_boundary_line(boundary: &VadBoundary, config: &TranscribeConfig) -> String {
    format!(
        "{{\"event_type\":\"vad_boundary\",\"channel\":\"merged\",\"boundary_id\":{},\"start_ms\":{},\"end_ms\":{},\"source\":\"{}\",\"vad_backend\":\"{}\",\"vad_threshold\":{:.3}}}",
        boundary.id,
        boundary.start_ms,
        boundary.end_ms,
        json_escape(boundary.source),
        json_escape(&config.vad_backend.to_string()),
        config.vad_threshold,
    )
}

pub(super) fn jsonl_transcript_event_line(
    event: &TranscriptEvent,
    backend_id: &str,
    vad_boundary_count: usize,
) -> String {
    if let Some(source_segment_id) = &event.source_final_segment_id {
        format!(
            "{{\"event_type\":\"{}\",\"channel\":\"{}\",\"segment_id\":\"{}\",\"source_final_segment_id\":\"{}\",\"start_ms\":{},\"end_ms\":{},\"text\":\"{}\",\"asr_backend\":\"{}\",\"vad_boundary_count\":{}}}",
            event.event_type,
            event.channel,
            json_escape(&event.segment_id),
            json_escape(source_segment_id),
            event.start_ms,
            event.end_ms,
            json_escape(&event.text),
            json_escape(backend_id),
            vad_boundary_count
        )
    } else {
        format!(
            "{{\"event_type\":\"{}\",\"channel\":\"{}\",\"segment_id\":\"{}\",\"start_ms\":{},\"end_ms\":{},\"text\":\"{}\",\"asr_backend\":\"{}\",\"vad_boundary_count\":{}}}",
            event.event_type,
            event.channel,
            json_escape(&event.segment_id),
            event.start_ms,
            event.end_ms,
            json_escape(&event.text),
            json_escape(backend_id),
            vad_boundary_count
        )
    }
}

pub(super) fn jsonl_mode_degradation_line(
    requested_mode: ChannelMode,
    active_mode: ChannelMode,
    degradation: &ModeDegradationEvent,
) -> String {
    format!(
        "{{\"event_type\":\"mode_degradation\",\"channel\":\"control\",\"requested_mode\":\"{}\",\"active_mode\":\"{}\",\"code\":\"{}\",\"detail\":\"{}\"}}",
        json_escape(&requested_mode.to_string()),
        json_escape(&active_mode.to_string()),
        json_escape(degradation.code),
        json_escape(&degradation.detail)
    )
}

pub(super) fn jsonl_trust_notice_line(notice: &TrustNotice) -> String {
    format!(
        "{{\"event_type\":\"trust_notice\",\"channel\":\"control\",\"code\":\"{}\",\"severity\":\"{}\",\"cause\":\"{}\",\"impact\":\"{}\",\"guidance\":\"{}\"}}",
        json_escape(&notice.code),
        json_escape(&notice.severity),
        json_escape(&notice.cause),
        json_escape(&notice.impact),
        json_escape(&notice.guidance)
    )
}

pub(super) fn jsonl_lifecycle_phase_line(
    index: usize,
    transition: &LiveLifecycleTransition,
) -> String {
    format!(
        "{{\"event_type\":\"lifecycle_phase\",\"channel\":\"control\",\"phase\":\"{}\",\"transition_index\":{},\"entered_at_utc\":\"{}\",\"ready_for_transcripts\":{},\"detail\":\"{}\"}}",
        transition.phase,
        index,
        json_escape(&transition.entered_at_utc),
        transition.phase.ready_for_transcripts(),
        json_escape(&transition.detail)
    )
}

pub(super) fn jsonl_reconciliation_matrix_line(reconciliation: &ReconciliationMatrix) -> String {
    format!(
        "{{\"event_type\":\"reconciliation_matrix\",\"channel\":\"control\",\"required\":{},\"applied\":{},\"trigger_count\":{},\"trigger_codes\":[{}]}}",
        reconciliation.required,
        reconciliation.applied,
        reconciliation.triggers.len(),
        reconciliation_trigger_codes_json(reconciliation.triggers.as_slice())
    )
}

pub(super) fn jsonl_asr_worker_pool_line(asr_worker_pool: &LiveAsrPoolTelemetry) -> String {
    format!(
        "{{\"event_type\":\"asr_worker_pool\",\"channel\":\"control\",\"prewarm_ok\":{},\"submitted\":{},\"enqueued\":{},\"dropped_queue_full\":{},\"processed\":{},\"succeeded\":{},\"failed\":{},\"retry_attempts\":{},\"temp_audio_deleted\":{},\"temp_audio_retained\":{}}}",
        asr_worker_pool.prewarm_ok,
        asr_worker_pool.submitted,
        asr_worker_pool.enqueued,
        asr_worker_pool.dropped_queue_full,
        asr_worker_pool.processed,
        asr_worker_pool.succeeded,
        asr_worker_pool.failed,
        asr_worker_pool.retry_attempts,
        asr_worker_pool.temp_audio_deleted,
        asr_worker_pool.temp_audio_retained
    )
}

pub(super) fn jsonl_chunk_queue_line(chunk_queue: &LiveChunkQueueTelemetry) -> String {
    format!(
        "{{\"event_type\":\"chunk_queue\",\"channel\":\"control\",\"enabled\":{},\"max_queue\":{},\"submitted\":{},\"enqueued\":{},\"dropped_oldest\":{},\"processed\":{},\"pending\":{},\"high_water\":{},\"drain_completed\":{},\"lag_sample_count\":{},\"lag_p50_ms\":{},\"lag_p95_ms\":{},\"lag_max_ms\":{}}}",
        chunk_queue.enabled,
        chunk_queue.max_queue,
        chunk_queue.submitted,
        chunk_queue.enqueued,
        chunk_queue.dropped_oldest,
        chunk_queue.processed,
        chunk_queue.pending,
        chunk_queue.high_water,
        chunk_queue.drain_completed,
        chunk_queue.lag_sample_count,
        chunk_queue.lag_p50_ms,
        chunk_queue.lag_p95_ms,
        chunk_queue.lag_max_ms
    )
}

pub(super) fn jsonl_cleanup_queue_line(cleanup_queue: &CleanupQueueTelemetry) -> String {
    format!(
        "{{\"event_type\":\"cleanup_queue\",\"channel\":\"control\",\"enabled\":{},\"max_queue\":{},\"timeout_ms\":{},\"retries\":{},\"submitted\":{},\"enqueued\":{},\"dropped_queue_full\":{},\"processed\":{},\"succeeded\":{},\"timed_out\":{},\"failed\":{},\"retry_attempts\":{},\"pending\":{},\"drain_budget_ms\":{},\"drain_completed\":{}}}",
        cleanup_queue.enabled,
        cleanup_queue.max_queue,
        cleanup_queue.timeout_ms,
        cleanup_queue.retries,
        cleanup_queue.submitted,
        cleanup_queue.enqueued,
        cleanup_queue.dropped_queue_full,
        cleanup_queue.processed,
        cleanup_queue.succeeded,
        cleanup_queue.timed_out,
        cleanup_queue.failed,
        cleanup_queue.retry_attempts,
        cleanup_queue.pending,
        cleanup_queue.drain_budget_ms,
        cleanup_queue.drain_completed
    )
}

pub(super) fn write_runtime_jsonl(
    config: &TranscribeConfig,
    report: &LiveRunReport,
) -> Result<(), CliError> {
    ensure_runtime_jsonl_parent(&config.out_jsonl)?;
    let mut file = File::create(&config.out_jsonl).map_err(|err| {
        CliError::new(format!(
            "failed to create JSONL file {}: {err}",
            display_path(&config.out_jsonl)
        ))
    })?;

    let early_lifecycle_count = report
        .lifecycle
        .transitions
        .iter()
        .position(|transition| transition.phase == LiveLifecyclePhase::Active)
        .map(|index| index + 1)
        .unwrap_or_else(|| report.lifecycle.transitions.len());
    for (index, transition) in report
        .lifecycle
        .transitions
        .iter()
        .enumerate()
        .take(early_lifecycle_count)
    {
        writeln!(file, "{}", jsonl_lifecycle_phase_line(index, transition)).map_err(io_to_cli)?;
    }

    for boundary in &report.vad_boundaries {
        writeln!(file, "{}", jsonl_vad_boundary_line(boundary, config)).map_err(io_to_cli)?;
    }

    for event in &report.events {
        writeln!(
            file,
            "{}",
            jsonl_transcript_event_line(event, report.backend_id, report.vad_boundaries.len())
        )
        .map_err(io_to_cli)?;
    }

    for degradation in &report.degradation_events {
        writeln!(
            file,
            "{}",
            jsonl_mode_degradation_line(
                report.channel_mode,
                report.active_channel_mode,
                degradation
            )
        )
        .map_err(io_to_cli)?;
    }

    for notice in &report.trust_notices {
        writeln!(file, "{}", jsonl_trust_notice_line(notice)).map_err(io_to_cli)?;
    }

    for (index, transition) in report
        .lifecycle
        .transitions
        .iter()
        .enumerate()
        .skip(early_lifecycle_count)
    {
        writeln!(file, "{}", jsonl_lifecycle_phase_line(index, transition)).map_err(io_to_cli)?;
    }

    writeln!(
        file,
        "{}",
        jsonl_reconciliation_matrix_line(&report.reconciliation)
    )
    .map_err(io_to_cli)?;

    writeln!(
        file,
        "{}",
        jsonl_asr_worker_pool_line(&report.asr_worker_pool)
    )
    .map_err(io_to_cli)?;

    writeln!(file, "{}", jsonl_chunk_queue_line(&report.chunk_queue)).map_err(io_to_cli)?;

    writeln!(file, "{}", jsonl_cleanup_queue_line(&report.cleanup_queue)).map_err(io_to_cli)?;
    Ok(())
}

pub(super) fn write_runtime_manifest(
    config: &TranscribeConfig,
    report: &LiveRunReport,
) -> Result<(), CliError> {
    if let Some(parent) = config.out_manifest.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CliError::new(format!(
                "failed to create runtime manifest directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    let mut file = File::create(&config.out_manifest).map_err(|err| {
        CliError::new(format!(
            "failed to create runtime manifest {}: {err}",
            display_path(&config.out_manifest)
        ))
    })?;

    let first_start_ms = report
        .vad_boundaries
        .first()
        .map(|v| v.start_ms)
        .unwrap_or(0);
    let last_end_ms = report.vad_boundaries.last().map(|v| v.end_ms).unwrap_or(0);
    let mut event_channels = report
        .events
        .iter()
        .map(|event| event.channel.clone())
        .collect::<Vec<_>>();
    event_channels.sort();
    event_channels.dedup();
    let per_channel_defaults = reconstruct_transcript_per_channel(&report.events);
    let live_mode = report.chunk_queue.enabled;
    let stable_terminal_lines = stable_terminal_summary_lines(&report.events);
    let terminal_mode = terminal_render_mode();
    let first_emit = first_emit_timing(&report.events);
    let model_checksum = model_checksum_info(Some(&ResolvedModelPath {
        path: report.resolved_model_path.clone(),
        source: report.resolved_model_source.clone(),
    }));
    let out_wav_metadata = fs::metadata(&config.out_wav).ok();
    let out_wav_materialized = out_wav_metadata
        .as_ref()
        .map(|metadata| metadata.is_file())
        .unwrap_or(false);
    let out_wav_bytes = out_wav_metadata.map(|metadata| metadata.len()).unwrap_or(0);

    writeln!(file, "{{").map_err(io_to_cli)?;
    writeln!(file, "  \"schema_version\": \"1\",").map_err(io_to_cli)?;
    writeln!(file, "  \"kind\": \"transcribe-live-runtime\",").map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"generated_at_utc\": \"{}\",",
        json_escape(&report.generated_at_utc)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"asr_backend\": \"{}\",",
        json_escape(report.backend_id)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"asr_model\": \"{}\",",
        json_escape(&display_path(&report.resolved_model_path))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"asr_model_source\": \"{}\",",
        json_escape(&report.resolved_model_source)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"asr_model_checksum_sha256\": \"{}\",",
        json_escape(&model_checksum.sha256)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"asr_model_checksum_status\": \"{}\",",
        json_escape(&model_checksum.status)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"input_wav\": \"{}\",",
        json_escape(&display_path(&config.input_wav))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"input_wav_semantics\": \"{}\",",
        json_escape(input_wav_semantics(config))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"out_wav\": \"{}\",",
        json_escape(&display_path(&config.out_wav))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"out_wav_semantics\": \"{}\",",
        json_escape(OUT_WAV_SEMANTICS)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"out_wav_materialized\": {},",
        out_wav_materialized
    )
    .map_err(io_to_cli)?;
    writeln!(file, "  \"out_wav_bytes\": {},", out_wav_bytes).map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"channel_mode\": \"{}\",",
        json_escape(&report.active_channel_mode.to_string())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"channel_mode_requested\": \"{}\",",
        json_escape(&report.channel_mode.to_string())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"runtime_mode\": \"{}\",",
        json_escape(config.runtime_mode_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"runtime_mode_taxonomy\": \"{}\",",
        json_escape(config.runtime_mode_taxonomy_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"runtime_mode_selector\": \"{}\",",
        json_escape(config.runtime_mode_selector_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"runtime_mode_status\": \"{}\",",
        json_escape(config.runtime_mode_status_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"live_config\": {{\"live_chunked\":{},\"chunk_window_ms\":{},\"chunk_stride_ms\":{},\"chunk_queue_cap\":{}}},",
        config.live_chunked,
        config.chunk_window_ms,
        config.chunk_stride_ms,
        config.chunk_queue_cap
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"lifecycle\": {{\"current_phase\":\"{}\",\"ready_for_transcripts\":{},\"transitions\": [",
        json_escape(report.lifecycle.current_phase.as_str()),
        report.lifecycle.ready_for_transcripts
    )
    .map_err(io_to_cli)?;
    for (idx, transition) in report.lifecycle.transitions.iter().enumerate() {
        writeln!(
            file,
            "    {{\"phase\":\"{}\",\"transition_index\":{},\"entered_at_utc\":\"{}\",\"ready_for_transcripts\":{},\"detail\":\"{}\"}}{}",
            json_escape(transition.phase.as_str()),
            idx,
            json_escape(&transition.entered_at_utc),
            transition.phase.ready_for_transcripts(),
            json_escape(&transition.detail),
            if idx + 1 == report.lifecycle.transitions.len() {
                ""
            } else {
                ","
            }
        )
        .map_err(io_to_cli)?;
    }
    writeln!(file, "  ]}},").map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"speaker_labels\": [\"{}\",\"{}\"],",
        json_escape(&config.speaker_labels.mic),
        json_escape(&config.speaker_labels.system)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"event_channels\": [{}],",
        event_channels
            .iter()
            .map(|channel| format!("\"{}\"", json_escape(channel)))
            .collect::<Vec<_>>()
            .join(",")
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"vad\": {{\"backend\":\"{}\",\"threshold\":{:.3},\"min_speech_ms\":{},\"min_silence_ms\":{},\"boundary_count\":{}}},",
        json_escape(&config.vad_backend.to_string()),
        config.vad_threshold,
        config.vad_min_speech_ms,
        config.vad_min_silence_ms,
        report.vad_boundaries.len()
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"transcript\": {{\"segment_id\":\"representative-0\",\"start_ms\":{},\"end_ms\":{},\"text\":\"{}\"}},",
        first_start_ms,
        last_end_ms,
        json_escape(&report.transcript_text)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"readability_defaults\": {{\"merged_line_format\":\"[MM:SS.mmm-MM:SS.mmm] <channel>: <text>\",\"near_overlap_window_ms\":{},\"near_overlap_annotation\":\"(overlap<={}ms with <channel>)\",\"ordering\":\"start_ms,end_ms,event_type,channel,segment_id,source_final_segment_id,text\"}},",
        OVERLAP_WINDOW_MS,
        OVERLAP_WINDOW_MS
    )
    .map_err(io_to_cli)?;
    writeln!(file, "  \"transcript_per_channel\": [").map_err(io_to_cli)?;
    for (idx, channel) in per_channel_defaults.iter().enumerate() {
        writeln!(
            file,
            "    {{\"channel\":\"{}\",\"text\":\"{}\"}}{}",
            json_escape(&channel.channel),
            json_escape(&channel.text),
            if idx + 1 == per_channel_defaults.len() {
                ""
            } else {
                ","
            }
        )
        .map_err(io_to_cli)?;
    }
    writeln!(file, "  ],").map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"terminal_summary\": {{\"live_mode\":{},\"render_mode\":\"{}\",\"stable_line_policy\":\"final-only\",\"stable_line_count\":{},\"stable_lines_replayed\":{},\"stable_lines\": [",
        live_mode,
        json_escape(terminal_mode.as_str()),
        stable_terminal_lines.len(),
        !live_mode
    )
    .map_err(io_to_cli)?;
    for (idx, line) in stable_terminal_lines.iter().enumerate() {
        writeln!(
            file,
            "    \"{}\"{}",
            json_escape(line),
            if idx + 1 == stable_terminal_lines.len() {
                ""
            } else {
                ","
            }
        )
        .map_err(io_to_cli)?;
    }
    writeln!(file, "  ]}},").map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"first_emit_timing_ms\": {{\"first_any\":{},\"first_partial\":{},\"first_final\":{},\"first_stable\":{}}},",
        json_optional_u64(first_emit.first_any_end_ms),
        json_optional_u64(first_emit.first_partial_end_ms),
        json_optional_u64(first_emit.first_final_end_ms),
        json_optional_u64(first_emit.first_stable_end_ms)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"queue_defer\": {{\"submit_window\":{},\"deferred_final_submissions\":{},\"max_pending_final_backlog\":{}}},",
        report.final_buffering.submit_window,
        report.final_buffering.deferred_final_submissions,
        report.final_buffering.max_pending_final_backlog
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"ordering_metadata\": {{\"event_sort_key\":\"start_ms,end_ms,event_type,channel,segment_id,source_final_segment_id,text\",\"stable_line_sort_key\":\"start_ms,end_ms,channel,segment_id,source_final_segment_id,text\",\"stable_line_event_types\":[\"final\",\"llm_final\",\"reconciled_final\"],\"event_count\":{}}},",
        report.events.len()
    )
    .map_err(io_to_cli)?;
    writeln!(file, "  \"events\": [").map_err(io_to_cli)?;
    for (idx, event) in report.events.iter().enumerate() {
        if let Some(source_segment_id) = &event.source_final_segment_id {
            writeln!(
                file,
                "    {{\"event_type\":\"{}\",\"channel\":\"{}\",\"segment_id\":\"{}\",\"source_final_segment_id\":\"{}\",\"start_ms\":{},\"end_ms\":{},\"text\":\"{}\"}}{}",
                json_escape(event.event_type),
                json_escape(&event.channel),
                json_escape(&event.segment_id),
                json_escape(source_segment_id),
                event.start_ms,
                event.end_ms,
                json_escape(&event.text),
                if idx + 1 == report.events.len() { "" } else { "," }
            )
            .map_err(io_to_cli)?;
        } else {
            writeln!(
                file,
                "    {{\"event_type\":\"{}\",\"channel\":\"{}\",\"segment_id\":\"{}\",\"start_ms\":{},\"end_ms\":{},\"text\":\"{}\"}}{}",
                json_escape(event.event_type),
                json_escape(&event.channel),
                json_escape(&event.segment_id),
                event.start_ms,
                event.end_ms,
                json_escape(&event.text),
                if idx + 1 == report.events.len() { "" } else { "," }
            )
            .map_err(io_to_cli)?;
        }
    }
    writeln!(file, "  ],").map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"benchmark\": {{\"run_count\":{},\"wall_ms_p50\":{:.6},\"wall_ms_p95\":{:.6},\"partial_slo_met\":{},\"final_slo_met\":{},\"summary_csv\":\"{}\",\"runs_csv\":\"{}\"}},",
        report.benchmark.run_count,
        report.benchmark.wall_ms_p50,
        report.benchmark.wall_ms_p95,
        report.benchmark.partial_slo_met,
        report.benchmark.final_slo_met,
        json_escape(&report.benchmark_summary_csv.display().to_string()),
        json_escape(&report.benchmark_runs_csv.display().to_string())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"reconciliation\": {{\"required\":{},\"applied\":{},\"trigger_count\":{},\"trigger_codes\":[{}]}},",
        report.reconciliation.required,
        report.reconciliation.applied,
        report.reconciliation.triggers.len(),
        reconciliation_trigger_codes_json(&report.reconciliation.triggers)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"asr_worker_pool\": {{\"prewarm_ok\":{},\"submitted\":{},\"enqueued\":{},\"dropped_queue_full\":{},\"processed\":{},\"succeeded\":{},\"failed\":{},\"retry_attempts\":{},\"temp_audio_deleted\":{},\"temp_audio_retained\":{}}},",
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
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"chunk_queue\": {{\"enabled\":{},\"max_queue\":{},\"submitted\":{},\"enqueued\":{},\"dropped_oldest\":{},\"processed\":{},\"pending\":{},\"high_water\":{},\"drain_completed\":{},\"lag_sample_count\":{},\"lag_p50_ms\":{},\"lag_p95_ms\":{},\"lag_max_ms\":{}}},",
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
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"cleanup_queue\": {{\"enabled\":{},\"max_queue\":{},\"timeout_ms\":{},\"retries\":{},\"submitted\":{},\"enqueued\":{},\"dropped_queue_full\":{},\"processed\":{},\"succeeded\":{},\"timed_out\":{},\"failed\":{},\"retry_attempts\":{},\"pending\":{},\"drain_budget_ms\":{},\"drain_completed\":{}}},",
        report.cleanup_queue.enabled,
        report.cleanup_queue.max_queue,
        report.cleanup_queue.timeout_ms,
        report.cleanup_queue.retries,
        report.cleanup_queue.submitted,
        report.cleanup_queue.enqueued,
        report.cleanup_queue.dropped_queue_full,
        report.cleanup_queue.processed,
        report.cleanup_queue.succeeded,
        report.cleanup_queue.timed_out,
        report.cleanup_queue.failed,
        report.cleanup_queue.retry_attempts,
        report.cleanup_queue.pending,
        report.cleanup_queue.drain_budget_ms,
        report.cleanup_queue.drain_completed
    )
    .map_err(io_to_cli)?;
    writeln!(file, "  \"degradation_events\": [").map_err(io_to_cli)?;
    for (idx, degradation) in report.degradation_events.iter().enumerate() {
        writeln!(file, "    {{").map_err(io_to_cli)?;
        writeln!(
            file,
            "      \"code\": \"{}\",",
            json_escape(degradation.code)
        )
        .map_err(io_to_cli)?;
        writeln!(
            file,
            "      \"detail\": \"{}\"",
            json_escape(&degradation.detail)
        )
        .map_err(io_to_cli)?;
        if idx + 1 == report.degradation_events.len() {
            writeln!(file, "    }}").map_err(io_to_cli)?;
        } else {
            writeln!(file, "    }},").map_err(io_to_cli)?;
        }
    }
    writeln!(file, "  ],").map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"trust\": {{\"degraded_mode_active\":{},\"notice_count\":{},\"notices\": [",
        !report.trust_notices.is_empty(),
        report.trust_notices.len()
    )
    .map_err(io_to_cli)?;
    for (idx, notice) in report.trust_notices.iter().enumerate() {
        writeln!(
            file,
            "    {{\"code\":\"{}\",\"severity\":\"{}\",\"cause\":\"{}\",\"impact\":\"{}\",\"guidance\":\"{}\"}}{}",
            json_escape(&notice.code),
            json_escape(&notice.severity),
            json_escape(&notice.cause),
            json_escape(&notice.impact),
            json_escape(&notice.guidance),
            if idx + 1 == report.trust_notices.len() {
                ""
            } else {
                ","
            }
        )
        .map_err(io_to_cli)?;
    }
    writeln!(file, "  ]}},").map_err(io_to_cli)?;
    let partial_count = transcript_event_count(&report.events, "partial");
    let final_count = transcript_event_count(&report.events, "final");
    let llm_final_count = transcript_event_count(&report.events, "llm_final");
    let reconciled_final_count = transcript_event_count(&report.events, "reconciled_final");
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
    writeln!(
        file,
        "  \"event_counts\": {{\"vad_boundary\":{},\"transcript\":{},\"partial\":{},\"final\":{},\"llm_final\":{},\"reconciled_final\":{}}},",
        report.vad_boundaries.len(),
        report.events.len(),
        partial_count,
        final_count,
        llm_final_count,
        reconciled_final_count
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"session_summary\": {{\"session_status\":\"{}\",\"duration_sec\":{},\"channel_mode_requested\":\"{}\",\"channel_mode_active\":\"{}\",\"transcript_events\":{{\"partial\":{},\"final\":{},\"llm_final\":{},\"reconciled_final\":{}}},\"chunk_queue\":{{\"submitted\":{},\"enqueued\":{},\"dropped_oldest\":{},\"processed\":{},\"pending\":{},\"high_water\":{},\"drain_completed\":{}}},\"chunk_lag\":{{\"lag_sample_count\":{},\"lag_p50_ms\":{},\"lag_p95_ms\":{},\"lag_max_ms\":{}}},\"trust_notices\":{{\"count\":{},\"top_codes\":[{}]}},\"degradation_events\":{{\"count\":{},\"top_codes\":[{}]}},\"cleanup_queue\":{{\"enabled\":{},\"submitted\":{},\"enqueued\":{},\"dropped_queue_full\":{},\"processed\":{},\"succeeded\":{},\"timed_out\":{},\"failed\":{},\"retry_attempts\":{},\"pending\":{},\"drain_completed\":{}}},\"artifacts\":{{\"out_wav\":\"{}\",\"out_jsonl\":\"{}\",\"out_manifest\":\"{}\"}}}},",
        json_escape(session_status(report)),
        config.duration_sec,
        json_escape(&report.channel_mode.to_string()),
        json_escape(&report.active_channel_mode.to_string()),
        partial_count,
        final_count,
        llm_final_count,
        reconciled_final_count,
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
        report.chunk_queue.lag_max_ms,
        report.trust_notices.len(),
        top_codes_json(&trust_top_codes),
        report.degradation_events.len(),
        top_codes_json(&degradation_top_codes),
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
        json_escape(&display_path(&config.out_wav)),
        json_escape(&display_path(&config.out_jsonl)),
        json_escape(&display_path(&config.out_manifest))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"jsonl_path\": \"{}\"",
        json_escape(&display_path(&config.out_jsonl))
    )
    .map_err(io_to_cli)?;
    writeln!(file, "}}").map_err(io_to_cli)?;
    Ok(())
}

pub(super) fn write_preflight_manifest(
    config: &TranscribeConfig,
    report: &PreflightReport,
) -> Result<(), CliError> {
    let resolved_model = validate_model_path_for_backend(config).ok();
    let model_checksum = model_checksum_info(resolved_model.as_ref());
    let requested_model = if config.asr_model.as_os_str().is_empty() {
        "<auto-discover>".to_string()
    } else {
        display_path(&config.asr_model)
    };
    let resolved_model_path = resolved_model
        .as_ref()
        .map(|model| display_path(&model.path))
        .unwrap_or_else(|| "<unresolved>".to_string());
    let resolved_model_source = resolved_model
        .as_ref()
        .map(|model| model.source.as_str())
        .unwrap_or("unresolved");

    if let Some(parent) = config.out_manifest.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CliError::new(format!(
                "failed to create manifest directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let mut file = File::create(&config.out_manifest).map_err(|err| {
        CliError::new(format!(
            "failed to create manifest {}: {err}",
            display_path(&config.out_manifest)
        ))
    })?;

    writeln!(file, "{{").map_err(io_to_cli)?;
    writeln!(file, "  \"schema_version\": \"1\",").map_err(io_to_cli)?;
    writeln!(file, "  \"kind\": \"transcribe-live-preflight\",").map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"generated_at_utc\": \"{}\",",
        json_escape(&report.generated_at_utc)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"overall_status\": \"{}\",",
        report.overall_status()
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"runtime_mode\": \"{}\",",
        json_escape(config.runtime_mode_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"runtime_mode_taxonomy\": \"{}\",",
        json_escape(config.runtime_mode_taxonomy_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"runtime_mode_selector\": \"{}\",",
        json_escape(config.runtime_mode_selector_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "  \"runtime_mode_status\": \"{}\",",
        json_escape(config.runtime_mode_status_label())
    )
    .map_err(io_to_cli)?;
    writeln!(file, "  \"config\": {{").map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"input_wav\": \"{}\",",
        json_escape(&display_path(&config.input_wav))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"input_wav_semantics\": \"{}\",",
        json_escape(input_wav_semantics(config))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"out_wav\": \"{}\",",
        json_escape(&display_path(&config.out_wav))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"out_wav_semantics\": \"{}\",",
        json_escape(OUT_WAV_SEMANTICS)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"out_jsonl\": \"{}\",",
        json_escape(&display_path(&config.out_jsonl))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"out_manifest\": \"{}\",",
        json_escape(&display_path(&config.out_manifest))
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"asr_backend\": \"{}\",",
        json_escape(&config.asr_backend.to_string())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"asr_model_requested\": \"{}\",",
        json_escape(&requested_model)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"asr_model_resolved\": \"{}\",",
        json_escape(&resolved_model_path)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"asr_model_source\": \"{}\",",
        json_escape(resolved_model_source)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"asr_model_checksum_sha256\": \"{}\",",
        json_escape(&model_checksum.sha256)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"asr_model_checksum_status\": \"{}\",",
        json_escape(&model_checksum.status)
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"runtime_mode\": \"{}\",",
        json_escape(config.runtime_mode_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"runtime_mode_taxonomy\": \"{}\",",
        json_escape(config.runtime_mode_taxonomy_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"runtime_mode_selector\": \"{}\",",
        json_escape(config.runtime_mode_selector_label())
    )
    .map_err(io_to_cli)?;
    writeln!(
        file,
        "    \"runtime_mode_status\": \"{}\",",
        json_escape(config.runtime_mode_status_label())
    )
    .map_err(io_to_cli)?;
    writeln!(file, "    \"live_chunked\": {},", config.live_chunked).map_err(io_to_cli)?;
    writeln!(file, "    \"chunk_window_ms\": {},", config.chunk_window_ms).map_err(io_to_cli)?;
    writeln!(file, "    \"chunk_stride_ms\": {},", config.chunk_stride_ms).map_err(io_to_cli)?;
    writeln!(file, "    \"chunk_queue_cap\": {},", config.chunk_queue_cap).map_err(io_to_cli)?;
    writeln!(file, "    \"sample_rate_hz\": {}", config.sample_rate_hz).map_err(io_to_cli)?;
    writeln!(file, "  }},").map_err(io_to_cli)?;
    writeln!(file, "  \"checks\": [").map_err(io_to_cli)?;

    for (idx, check) in report.checks.iter().enumerate() {
        writeln!(file, "    {{").map_err(io_to_cli)?;
        writeln!(file, "      \"id\": \"{}\",", json_escape(check.id)).map_err(io_to_cli)?;
        writeln!(
            file,
            "      \"status\": \"{}\",",
            json_escape(&check.status.to_string())
        )
        .map_err(io_to_cli)?;
        writeln!(
            file,
            "      \"detail\": \"{}\",",
            json_escape(&check.detail)
        )
        .map_err(io_to_cli)?;
        writeln!(
            file,
            "      \"remediation\": \"{}\"",
            json_escape(check.remediation.as_deref().unwrap_or(""))
        )
        .map_err(io_to_cli)?;
        if idx + 1 == report.checks.len() {
            writeln!(file, "    }}").map_err(io_to_cli)?;
        } else {
            writeln!(file, "    }},").map_err(io_to_cli)?;
        }
    }

    writeln!(file, "  ]").map_err(io_to_cli)?;
    writeln!(file, "}}").map_err(io_to_cli)?;
    Ok(())
}
