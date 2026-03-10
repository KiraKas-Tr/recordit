use super::contracts_models::{runtime_jsonl, runtime_manifest};
use super::runtime_manifest_models as manifest_models;
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
    serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::VadBoundary(
        runtime_jsonl::VadBoundaryEventModel {
            channel: "merged".to_string(),
            boundary_id: boundary.id,
            start_ms: boundary.start_ms,
            end_ms: boundary.end_ms,
            source: boundary.source.to_string(),
            vad_backend: config.vad_backend.to_string(),
            vad_threshold: config.vad_threshold as f64,
        },
    ))
    .expect("runtime jsonl vad_boundary serialization")
}

pub(super) fn jsonl_transcript_event_line(
    event: &TranscriptEvent,
    backend_id: &str,
    vad_boundary_count: usize,
) -> String {
    let payload = runtime_jsonl::TranscriptArtifactEventModel {
        channel: event.channel.clone(),
        segment_id: event.segment_id.clone(),
        source_final_segment_id: event.source_final_segment_id.clone(),
        start_ms: event.start_ms,
        end_ms: event.end_ms,
        text: event.text.clone(),
        asr_backend: backend_id.to_string(),
        vad_boundary_count,
    };
    match event.event_type {
        runtime_jsonl::EVENT_TYPE_PARTIAL => {
            serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::Partial(payload))
                .expect("runtime jsonl transcript serialization")
        }
        runtime_jsonl::EVENT_TYPE_STABLE_PARTIAL => {
            serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::StablePartial(payload))
                .expect("runtime jsonl transcript serialization")
        }
        runtime_jsonl::EVENT_TYPE_FINAL => {
            serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::Final(payload))
                .expect("runtime jsonl transcript serialization")
        }
        runtime_jsonl::EVENT_TYPE_LLM_FINAL => {
            serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::LlmFinal(payload))
                .expect("runtime jsonl transcript serialization")
        }
        runtime_jsonl::EVENT_TYPE_RECONCILED_FINAL => {
            serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::ReconciledFinal(payload))
                .expect("runtime jsonl transcript serialization")
        }
        // Preserve forward compatibility for unknown transcript-like rows without
        // introducing a panic surface in artifact emission.
        other => serde_json::to_string(&serde_json::json!({
            "event_type": other,
            "channel": event.channel,
            "segment_id": event.segment_id,
            "source_final_segment_id": event.source_final_segment_id,
            "start_ms": event.start_ms,
            "end_ms": event.end_ms,
            "text": event.text,
            "asr_backend": backend_id,
            "vad_boundary_count": vad_boundary_count
        }))
        .expect("runtime jsonl transcript fallback serialization"),
    }
}

pub(super) fn jsonl_mode_degradation_line(
    requested_mode: ChannelMode,
    active_mode: ChannelMode,
    degradation: &ModeDegradationEvent,
) -> String {
    serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::ModeDegradation(
        runtime_jsonl::ModeDegradationEventModel {
            channel: "control".to_string(),
            requested_mode: requested_mode.to_string(),
            active_mode: active_mode.to_string(),
            code: degradation.code.to_string(),
            detail: degradation.detail.clone(),
        },
    ))
    .expect("runtime jsonl mode_degradation serialization")
}

pub(super) fn jsonl_trust_notice_line(notice: &TrustNotice) -> String {
    serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::TrustNotice(
        runtime_jsonl::TrustNoticeEventModel {
            channel: "control".to_string(),
            code: notice.code.clone(),
            severity: notice.severity.clone(),
            cause: notice.cause.clone(),
            impact: notice.impact.clone(),
            guidance: notice.guidance.clone(),
        },
    ))
    .expect("runtime jsonl trust_notice serialization")
}

pub(super) fn jsonl_lifecycle_phase_line(
    index: usize,
    transition: &LiveLifecycleTransition,
) -> String {
    serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::LifecyclePhase(
        runtime_jsonl::LifecyclePhaseEventModel {
            channel: "control".to_string(),
            phase: transition.phase.to_string(),
            transition_index: index,
            entered_at_utc: transition.entered_at_utc.clone(),
            ready_for_transcripts: transition.phase.ready_for_transcripts(),
            detail: transition.detail.clone(),
        },
    ))
    .expect("runtime jsonl lifecycle_phase serialization")
}

pub(super) fn jsonl_reconciliation_matrix_line(reconciliation: &ReconciliationMatrix) -> String {
    serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::ReconciliationMatrix(
        runtime_jsonl::ReconciliationMatrixEventModel {
            channel: "control".to_string(),
            required: reconciliation.required,
            applied: reconciliation.applied,
            trigger_count: reconciliation.triggers.len(),
            trigger_codes: reconciliation
                .triggers
                .iter()
                .map(|trigger| trigger.code.to_string())
                .collect(),
        },
    ))
    .expect("runtime jsonl reconciliation_matrix serialization")
}

pub(super) fn jsonl_asr_worker_pool_line(asr_worker_pool: &LiveAsrPoolTelemetry) -> String {
    serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::AsrWorkerPool(
        runtime_jsonl::AsrWorkerPoolEventModel {
            channel: "control".to_string(),
            prewarm_ok: asr_worker_pool.prewarm_ok,
            submitted: asr_worker_pool.submitted,
            enqueued: asr_worker_pool.enqueued,
            dropped_queue_full: asr_worker_pool.dropped_queue_full,
            processed: asr_worker_pool.processed,
            succeeded: asr_worker_pool.succeeded,
            failed: asr_worker_pool.failed,
            retry_attempts: asr_worker_pool.retry_attempts,
            temp_audio_deleted: asr_worker_pool.temp_audio_deleted,
            temp_audio_retained: asr_worker_pool.temp_audio_retained,
        },
    ))
    .expect("runtime jsonl asr_worker_pool serialization")
}

pub(super) fn jsonl_chunk_queue_line(chunk_queue: &LiveChunkQueueTelemetry) -> String {
    serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::ChunkQueue(
        runtime_jsonl::ChunkQueueEventModel {
            channel: "control".to_string(),
            enabled: chunk_queue.enabled,
            max_queue: chunk_queue.max_queue,
            submitted: chunk_queue.submitted,
            enqueued: chunk_queue.enqueued,
            dropped_oldest: chunk_queue.dropped_oldest,
            processed: chunk_queue.processed,
            pending: chunk_queue.pending,
            high_water: chunk_queue.high_water,
            drain_completed: chunk_queue.drain_completed,
            lag_sample_count: chunk_queue.lag_sample_count,
            lag_p50_ms: chunk_queue.lag_p50_ms as usize,
            lag_p95_ms: chunk_queue.lag_p95_ms as usize,
            lag_max_ms: chunk_queue.lag_max_ms as usize,
        },
    ))
    .expect("runtime jsonl chunk_queue serialization")
}

pub(super) fn jsonl_cleanup_queue_line(cleanup_queue: &CleanupQueueTelemetry) -> String {
    serde_json::to_string(&runtime_jsonl::RuntimeJsonlEvent::CleanupQueue(
        runtime_jsonl::CleanupQueueEventModel {
            channel: "control".to_string(),
            enabled: cleanup_queue.enabled,
            max_queue: cleanup_queue.max_queue,
            timeout_ms: cleanup_queue.timeout_ms,
            retries: cleanup_queue.retries,
            submitted: cleanup_queue.submitted,
            enqueued: cleanup_queue.enqueued,
            dropped_queue_full: cleanup_queue.dropped_queue_full,
            processed: cleanup_queue.processed,
            succeeded: cleanup_queue.succeeded,
            timed_out: cleanup_queue.timed_out,
            failed: cleanup_queue.failed,
            retry_attempts: cleanup_queue.retry_attempts,
            pending: cleanup_queue.pending,
            drain_budget_ms: cleanup_queue.drain_budget_ms,
            drain_completed: cleanup_queue.drain_completed,
        },
    ))
    .expect("runtime jsonl cleanup_queue serialization")
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

fn build_runtime_manifest_model(
    config: &TranscribeConfig,
    report: &LiveRunReport,
) -> manifest_models::RuntimeManifest {
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
    let partial_count = transcript_event_count(&report.events, runtime_jsonl::EVENT_TYPE_PARTIAL);
    let stable_partial_count =
        transcript_event_count(&report.events, runtime_jsonl::EVENT_TYPE_STABLE_PARTIAL);
    let final_count = transcript_event_count(&report.events, runtime_jsonl::EVENT_TYPE_FINAL);
    let llm_final_count =
        transcript_event_count(&report.events, runtime_jsonl::EVENT_TYPE_LLM_FINAL);
    let reconciled_final_count =
        transcript_event_count(&report.events, runtime_jsonl::EVENT_TYPE_RECONCILED_FINAL);
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

    manifest_models::RuntimeManifest {
        schema_version: "1".to_string(),
        kind: runtime_manifest::KIND_RUNTIME_MANIFEST.to_string(),
        generated_at_utc: report.generated_at_utc.clone(),
        asr_backend: report.backend_id.to_string(),
        asr_model: display_path(&report.resolved_model_path),
        asr_model_source: report.resolved_model_source.clone(),
        asr_model_checksum_sha256: model_checksum.sha256,
        asr_model_checksum_status: model_checksum.status,
        input_wav: display_path(&config.input_wav),
        input_wav_semantics: input_wav_semantics(config).to_string(),
        out_wav: display_path(&config.out_wav),
        out_wav_semantics: OUT_WAV_SEMANTICS.to_string(),
        out_wav_materialized,
        out_wav_bytes,
        channel_mode: report.active_channel_mode.to_string(),
        channel_mode_requested: report.channel_mode.to_string(),
        runtime_mode: config.runtime_mode_label().to_string(),
        runtime_mode_taxonomy: config.runtime_mode_taxonomy_label().to_string(),
        runtime_mode_selector: config.runtime_mode_selector_label().to_string(),
        runtime_mode_status: config.runtime_mode_status_label().to_string(),
        live_config: manifest_models::RuntimeLiveConfig {
            live_chunked: config.live_chunked,
            chunk_window_ms: config.chunk_window_ms,
            chunk_stride_ms: config.chunk_stride_ms,
            chunk_queue_cap: config.chunk_queue_cap,
        },
        lifecycle: manifest_models::RuntimeLifecycle {
            current_phase: report.lifecycle.current_phase.as_str().to_string(),
            ready_for_transcripts: report.lifecycle.ready_for_transcripts,
            transitions: report
                .lifecycle
                .transitions
                .iter()
                .enumerate()
                .map(
                    |(idx, transition)| manifest_models::RuntimeLifecycleTransition {
                        phase: transition.phase.as_str().to_string(),
                        transition_index: idx,
                        entered_at_utc: transition.entered_at_utc.clone(),
                        ready_for_transcripts: transition.phase.ready_for_transcripts(),
                        detail: transition.detail.clone(),
                    },
                )
                .collect(),
        },
        speaker_labels: vec![
            config.speaker_labels.mic.clone(),
            config.speaker_labels.system.clone(),
        ],
        event_channels,
        vad: manifest_models::RuntimeVad {
            backend: config.vad_backend.to_string(),
            threshold: config.vad_threshold,
            min_speech_ms: u64::from(config.vad_min_speech_ms),
            min_silence_ms: u64::from(config.vad_min_silence_ms),
            boundary_count: report.vad_boundaries.len(),
        },
        transcript: manifest_models::RuntimeTranscript {
            segment_id: "representative-0".to_string(),
            start_ms: first_start_ms,
            end_ms: last_end_ms,
            text: report.transcript_text.clone(),
        },
        readability_defaults: manifest_models::RuntimeReadabilityDefaults {
            merged_line_format: "[MM:SS.mmm-MM:SS.mmm] <channel>: <text>".to_string(),
            near_overlap_window_ms: OVERLAP_WINDOW_MS,
            near_overlap_annotation: format!("(overlap<={}ms with <channel>)", OVERLAP_WINDOW_MS),
            ordering: "start_ms,end_ms,event_type,channel,segment_id,source_final_segment_id,text"
                .to_string(),
        },
        transcript_per_channel: per_channel_defaults
            .iter()
            .map(|channel| manifest_models::RuntimeTranscriptPerChannel {
                channel: channel.channel.clone(),
                text: channel.text.clone(),
            })
            .collect(),
        terminal_summary: manifest_models::RuntimeTerminalSummary {
            live_mode,
            render_mode: terminal_mode.as_str().to_string(),
            stable_line_policy: "final-only".to_string(),
            stable_line_count: stable_terminal_lines.len(),
            stable_lines_replayed: !live_mode,
            stable_lines: stable_terminal_lines,
        },
        first_emit_timing_ms: manifest_models::RuntimeFirstEmitTiming {
            first_any: first_emit.first_any_end_ms,
            first_partial: first_emit.first_partial_end_ms,
            first_final: first_emit.first_final_end_ms,
            first_stable: first_emit.first_stable_end_ms,
        },
        queue_defer: manifest_models::RuntimeQueueDefer {
            submit_window: report.final_buffering.submit_window,
            deferred_final_submissions: report.final_buffering.deferred_final_submissions,
            max_pending_final_backlog: report.final_buffering.max_pending_final_backlog,
        },
        ordering_metadata: manifest_models::RuntimeOrderingMetadata {
            event_sort_key:
                "start_ms,end_ms,event_type,channel,segment_id,source_final_segment_id,text"
                    .to_string(),
            stable_line_sort_key: "start_ms,end_ms,channel,segment_id,source_final_segment_id,text"
                .to_string(),
            stable_line_event_types: vec![
                runtime_jsonl::EVENT_TYPE_FINAL.to_string(),
                runtime_jsonl::EVENT_TYPE_LLM_FINAL.to_string(),
                runtime_jsonl::EVENT_TYPE_RECONCILED_FINAL.to_string(),
            ],
            event_count: report.events.len(),
        },
        events: report
            .events
            .iter()
            .map(|event| manifest_models::RuntimeTranscriptEvent {
                event_type: event.event_type.to_string(),
                channel: event.channel.clone(),
                segment_id: event.segment_id.clone(),
                source_final_segment_id: event.source_final_segment_id.clone(),
                start_ms: event.start_ms,
                end_ms: event.end_ms,
                text: event.text.clone(),
            })
            .collect(),
        benchmark: manifest_models::RuntimeBenchmark {
            run_count: report.benchmark.run_count,
            wall_ms_p50: report.benchmark.wall_ms_p50,
            wall_ms_p95: report.benchmark.wall_ms_p95,
            partial_slo_met: report.benchmark.partial_slo_met,
            final_slo_met: report.benchmark.final_slo_met,
            summary_csv: report.benchmark_summary_csv.display().to_string(),
            runs_csv: report.benchmark_runs_csv.display().to_string(),
        },
        reconciliation: manifest_models::RuntimeReconciliation {
            required: report.reconciliation.required,
            applied: report.reconciliation.applied,
            trigger_count: report.reconciliation.triggers.len(),
            trigger_codes: report
                .reconciliation
                .triggers
                .iter()
                .map(|trigger| trigger.code.to_string())
                .collect(),
        },
        asr_worker_pool: manifest_models::RuntimeAsrWorkerPool {
            prewarm_ok: report.asr_worker_pool.prewarm_ok,
            submitted: report.asr_worker_pool.submitted,
            enqueued: report.asr_worker_pool.enqueued,
            dropped_queue_full: report.asr_worker_pool.dropped_queue_full,
            processed: report.asr_worker_pool.processed,
            succeeded: report.asr_worker_pool.succeeded,
            failed: report.asr_worker_pool.failed,
            retry_attempts: report.asr_worker_pool.retry_attempts,
            temp_audio_deleted: report.asr_worker_pool.temp_audio_deleted,
            temp_audio_retained: report.asr_worker_pool.temp_audio_retained,
        },
        chunk_queue: manifest_models::RuntimeChunkQueue {
            enabled: report.chunk_queue.enabled,
            max_queue: report.chunk_queue.max_queue,
            submitted: report.chunk_queue.submitted,
            enqueued: report.chunk_queue.enqueued,
            dropped_oldest: report.chunk_queue.dropped_oldest,
            processed: report.chunk_queue.processed,
            pending: report.chunk_queue.pending,
            high_water: report.chunk_queue.high_water,
            drain_completed: report.chunk_queue.drain_completed,
            lag_sample_count: report.chunk_queue.lag_sample_count,
            lag_p50_ms: report.chunk_queue.lag_p50_ms,
            lag_p95_ms: report.chunk_queue.lag_p95_ms,
            lag_max_ms: report.chunk_queue.lag_max_ms,
        },
        cleanup_queue: manifest_models::RuntimeCleanupQueue {
            enabled: report.cleanup_queue.enabled,
            max_queue: report.cleanup_queue.max_queue,
            timeout_ms: report.cleanup_queue.timeout_ms,
            retries: report.cleanup_queue.retries,
            submitted: report.cleanup_queue.submitted,
            enqueued: report.cleanup_queue.enqueued,
            dropped_queue_full: report.cleanup_queue.dropped_queue_full,
            processed: report.cleanup_queue.processed,
            succeeded: report.cleanup_queue.succeeded,
            timed_out: report.cleanup_queue.timed_out,
            failed: report.cleanup_queue.failed,
            retry_attempts: report.cleanup_queue.retry_attempts,
            pending: report.cleanup_queue.pending,
            drain_budget_ms: report.cleanup_queue.drain_budget_ms,
            drain_completed: report.cleanup_queue.drain_completed,
        },
        degradation_events: report
            .degradation_events
            .iter()
            .map(|event| manifest_models::RuntimeDegradationEvent {
                code: event.code.to_string(),
                detail: event.detail.clone(),
            })
            .collect(),
        trust: manifest_models::RuntimeTrust {
            degraded_mode_active: !report.trust_notices.is_empty(),
            notice_count: report.trust_notices.len(),
            notices: report
                .trust_notices
                .iter()
                .map(|notice| manifest_models::RuntimeTrustNotice {
                    code: notice.code.clone(),
                    severity: notice.severity.clone(),
                    cause: notice.cause.clone(),
                    impact: notice.impact.clone(),
                    guidance: notice.guidance.clone(),
                })
                .collect(),
        },
        event_counts: manifest_models::RuntimeEventCounts {
            vad_boundary: report.vad_boundaries.len(),
            transcript: report.events.len(),
            partial: partial_count,
            stable_partial: stable_partial_count,
            final_count,
            llm_final: llm_final_count,
            reconciled_final: reconciled_final_count,
        },
        session_summary: manifest_models::RuntimeSessionSummary {
            session_status: session_status(report).to_string(),
            duration_sec: config.duration_sec,
            channel_mode_requested: report.channel_mode.to_string(),
            channel_mode_active: report.active_channel_mode.to_string(),
            transcript_events: manifest_models::RuntimeSessionTranscriptEvents {
                partial: partial_count,
                stable_partial: stable_partial_count,
                final_count,
                llm_final: llm_final_count,
                reconciled_final: reconciled_final_count,
            },
            chunk_queue: manifest_models::RuntimeSessionChunkQueue {
                submitted: report.chunk_queue.submitted,
                enqueued: report.chunk_queue.enqueued,
                dropped_oldest: report.chunk_queue.dropped_oldest,
                processed: report.chunk_queue.processed,
                pending: report.chunk_queue.pending,
                high_water: report.chunk_queue.high_water,
                drain_completed: report.chunk_queue.drain_completed,
            },
            chunk_lag: manifest_models::RuntimeSessionChunkLag {
                lag_sample_count: report.chunk_queue.lag_sample_count,
                lag_p50_ms: report.chunk_queue.lag_p50_ms,
                lag_p95_ms: report.chunk_queue.lag_p95_ms,
                lag_max_ms: report.chunk_queue.lag_max_ms,
            },
            trust_notices: manifest_models::RuntimeSessionCodeSummary {
                count: report.trust_notices.len(),
                top_codes: trust_top_codes,
            },
            degradation_events: manifest_models::RuntimeSessionCodeSummary {
                count: report.degradation_events.len(),
                top_codes: degradation_top_codes,
            },
            cleanup_queue: manifest_models::RuntimeSessionCleanupQueue {
                enabled: report.cleanup_queue.enabled,
                submitted: report.cleanup_queue.submitted,
                enqueued: report.cleanup_queue.enqueued,
                dropped_queue_full: report.cleanup_queue.dropped_queue_full,
                processed: report.cleanup_queue.processed,
                succeeded: report.cleanup_queue.succeeded,
                timed_out: report.cleanup_queue.timed_out,
                failed: report.cleanup_queue.failed,
                retry_attempts: report.cleanup_queue.retry_attempts,
                pending: report.cleanup_queue.pending,
                drain_completed: report.cleanup_queue.drain_completed,
            },
            artifacts: manifest_models::RuntimeSessionArtifacts {
                out_wav: display_path(&config.out_wav),
                out_jsonl: display_path(&config.out_jsonl),
                out_manifest: display_path(&config.out_manifest),
            },
        },
        jsonl_path: display_path(&config.out_jsonl),
    }
}

fn build_preflight_manifest_model(
    config: &TranscribeConfig,
    report: &PreflightReport,
) -> manifest_models::PreflightManifest {
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

    manifest_models::PreflightManifest {
        schema_version: "1".to_string(),
        kind: runtime_manifest::KIND_PREFLIGHT_MANIFEST.to_string(),
        generated_at_utc: report.generated_at_utc.clone(),
        overall_status: report.overall_status().to_string(),
        runtime_mode: Some(config.runtime_mode_label().to_string()),
        runtime_mode_taxonomy: Some(config.runtime_mode_taxonomy_label().to_string()),
        runtime_mode_selector: Some(config.runtime_mode_selector_label().to_string()),
        runtime_mode_status: Some(config.runtime_mode_status_label().to_string()),
        config: manifest_models::PreflightConfig {
            input_wav: Some(display_path(&config.input_wav)),
            input_wav_semantics: Some(input_wav_semantics(config).to_string()),
            out_wav: display_path(&config.out_wav),
            out_wav_semantics: Some(OUT_WAV_SEMANTICS.to_string()),
            out_jsonl: display_path(&config.out_jsonl),
            out_manifest: display_path(&config.out_manifest),
            asr_backend: config.asr_backend.to_string(),
            asr_model_requested: requested_model,
            asr_model_resolved: resolved_model_path,
            asr_model_source: resolved_model_source.to_string(),
            asr_model_checksum_sha256: Some(model_checksum.sha256),
            asr_model_checksum_status: Some(model_checksum.status),
            runtime_mode: Some(config.runtime_mode_label().to_string()),
            runtime_mode_taxonomy: Some(config.runtime_mode_taxonomy_label().to_string()),
            runtime_mode_selector: Some(config.runtime_mode_selector_label().to_string()),
            runtime_mode_status: Some(config.runtime_mode_status_label().to_string()),
            live_chunked: Some(config.live_chunked),
            chunk_window_ms: Some(config.chunk_window_ms),
            chunk_stride_ms: Some(config.chunk_stride_ms),
            chunk_queue_cap: Some(config.chunk_queue_cap),
            sample_rate_hz: config.sample_rate_hz,
        },
        checks: report
            .checks
            .iter()
            .map(|check| manifest_models::PreflightCheck {
                id: check.id.to_string(),
                status: check.status.to_string(),
                detail: check.detail.clone(),
                remediation: check.remediation.as_deref().unwrap_or("").to_string(),
            })
            .collect(),
    }
}

fn next_atomic_write_nonce() -> u64 {
    static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn atomic_temp_path(target_path: &Path) -> PathBuf {
    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");
    let temp_name = format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        next_atomic_write_nonce()
    );
    parent.join(temp_name)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), CliError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let dir = File::open(parent).map_err(|err| {
        CliError::new(format!(
            "failed to open artifact parent directory {} for fsync: {err}",
            parent.display()
        ))
    })?;
    dir.sync_all().map_err(io_to_cli)
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), CliError> {
    Ok(())
}

fn write_atomic_file<F>(target_path: &Path, label: &str, mut write_fn: F) -> Result<(), CliError>
where
    F: FnMut(&mut File) -> Result<(), CliError>,
{
    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|err| {
        CliError::new(format!(
            "failed to create {label} directory {}: {err}",
            parent.display()
        ))
    })?;

    let temp_path = atomic_temp_path(target_path);
    let mut temp_file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|err| {
            CliError::new(format!(
                "failed to create temporary {label} file {}: {err}",
                temp_path.display()
            ))
        })?;

    if let Err(err) = write_fn(&mut temp_file) {
        drop(temp_file);
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    temp_file.sync_all().map_err(io_to_cli)?;
    drop(temp_file);

    fs::rename(&temp_path, target_path).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        CliError::new(format!(
            "failed to atomically replace {label} {}: {err}",
            display_path(target_path)
        ))
    })?;
    sync_parent_directory(target_path)?;
    Ok(())
}

pub(super) fn write_runtime_manifest(
    config: &TranscribeConfig,
    report: &LiveRunReport,
) -> Result<(), CliError> {
    let manifest_model = build_runtime_manifest_model(config, report);
    write_atomic_file(&config.out_manifest, "runtime manifest", |file| {
        serde_json::to_writer_pretty(&mut *file, &manifest_model).map_err(|err| {
            CliError::new(format!(
                "failed to serialize runtime manifest {}: {err}",
                display_path(&config.out_manifest)
            ))
        })?;
        writeln!(file).map_err(io_to_cli)?;
        Ok(())
    })
}

pub(super) fn write_preflight_manifest(
    config: &TranscribeConfig,
    report: &PreflightReport,
) -> Result<(), CliError> {
    let manifest_model = build_preflight_manifest_model(config, report);
    write_atomic_file(&config.out_manifest, "preflight manifest", |file| {
        serde_json::to_writer_pretty(&mut *file, &manifest_model).map_err(|err| {
            CliError::new(format!(
                "failed to serialize preflight manifest {}: {err}",
                display_path(&config.out_manifest)
            ))
        })?;
        writeln!(file).map_err(io_to_cli)?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn keys_set(value: &Value) -> BTreeSet<String> {
        value
            .as_object()
            .expect("json object")
            .keys()
            .cloned()
            .collect()
    }

    fn expected_set(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|key| (*key).to_string()).collect()
    }

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "recordit-{test_name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn contracts_models_vad_boundary_keys_match_emitted_jsonl() {
        let config = TranscribeConfig::default();
        let boundary = VadBoundary {
            id: 3,
            start_ms: 120,
            end_ms: 980,
            source: "live_runtime",
        };

        let line = jsonl_vad_boundary_line(&boundary, &config);
        let parsed: Value = serde_json::from_str(&line).expect("valid json line");
        assert_eq!(
            parsed.get("event_type").and_then(Value::as_str),
            Some(runtime_jsonl::EVENT_TYPE_VAD_BOUNDARY)
        );
        assert_eq!(
            keys_set(&parsed),
            expected_set(runtime_jsonl::VAD_BOUNDARY_KEYS)
        );
    }

    #[test]
    fn contracts_models_cleanup_queue_keys_match_emitted_jsonl() {
        let config = TranscribeConfig::default();
        let telemetry = CleanupQueueTelemetry::disabled(&config);
        let line = jsonl_cleanup_queue_line(&telemetry);
        let parsed: Value = serde_json::from_str(&line).expect("valid json line");
        assert_eq!(
            parsed.get("event_type").and_then(Value::as_str),
            Some(runtime_jsonl::EVENT_TYPE_CLEANUP_QUEUE)
        );
        assert_eq!(
            keys_set(&parsed),
            expected_set(runtime_jsonl::CLEANUP_QUEUE_KEYS)
        );
    }

    #[test]
    fn contracts_models_manifest_constants_preserve_kind_and_core_keys() {
        assert_eq!(
            runtime_manifest::KIND_RUNTIME_MANIFEST,
            "transcribe-live-runtime"
        );
        assert_eq!(
            runtime_manifest::KIND_PREFLIGHT_MANIFEST,
            "transcribe-live-preflight"
        );
        for required in [
            "runtime_mode",
            "runtime_mode_taxonomy",
            "runtime_mode_selector",
            "runtime_mode_status",
            "events",
            "session_summary",
        ] {
            assert!(
                runtime_manifest::RUNTIME_TOP_LEVEL_KEYS.contains(&required),
                "missing top-level runtime manifest key {required}"
            );
        }
        for required in ["session_status", "transcript_events", "artifacts"] {
            assert!(
                runtime_manifest::SESSION_SUMMARY_KEYS.contains(&required),
                "missing session_summary key {required}"
            );
        }
    }

    #[test]
    fn write_atomic_file_preserves_existing_content_on_failure() {
        let dir = temp_test_dir("atomic-preserve");
        let target = dir.join("session.manifest.json");
        fs::write(&target, "{\"status\":\"old\"}\n").expect("seed target");

        let err = write_atomic_file(&target, "manifest", |file| {
            file.write_all(br#"{"status":"partial"#)
                .map_err(io_to_cli)?;
            Err(CliError::new("simulated interruption"))
        })
        .expect_err("expected simulated interruption");
        assert!(
            err.to_string().contains("simulated interruption"),
            "unexpected error: {err}"
        );

        let current = fs::read_to_string(&target).expect("read target");
        assert_eq!(current, "{\"status\":\"old\"}\n");

        let leftover_temps = fs::read_dir(&dir)
            .expect("read temp dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with(".session.manifest.json.tmp-"))
            .collect::<Vec<_>>();
        assert!(
            leftover_temps.is_empty(),
            "unexpected temporary artifacts left behind: {leftover_temps:?}"
        );
    }

    #[test]
    fn write_atomic_file_replaces_existing_content() {
        let dir = temp_test_dir("atomic-replace");
        let target = dir.join("session.manifest.json");
        fs::write(&target, "{\"status\":\"old\"}\n").expect("seed target");

        write_atomic_file(&target, "manifest", |file| {
            file.write_all(br#"{"status":"new"}"#).map_err(io_to_cli)?;
            file.write_all(b"\n").map_err(io_to_cli)?;
            Ok(())
        })
        .expect("atomic write succeeds");

        let current = fs::read_to_string(&target).expect("read target");
        assert_eq!(current, "{\"status\":\"new\"}\n");

        let leftover_temps = fs::read_dir(&dir)
            .expect("read temp dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with(".session.manifest.json.tmp-"))
            .collect::<Vec<_>>();
        assert!(
            leftover_temps.is_empty(),
            "unexpected temporary artifacts left behind: {leftover_temps:?}"
        );
    }
}
