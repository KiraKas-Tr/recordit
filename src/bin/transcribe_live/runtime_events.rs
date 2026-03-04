use super::*;

pub(super) fn runtime_job_class_to_pool_job_class(class: RuntimeAsrJobClass) -> LiveAsrJobClass {
    match class {
        RuntimeAsrJobClass::Partial => LiveAsrJobClass::Partial,
        RuntimeAsrJobClass::Final => LiveAsrJobClass::Final,
        RuntimeAsrJobClass::Reconcile => LiveAsrJobClass::Reconcile,
    }
}

pub(super) fn runtime_channel_role(channel: &str) -> &'static str {
    match channel {
        "microphone" => "mic",
        "system-audio" => "system",
        _ => "mixed",
    }
}

pub(super) fn runtime_channel_label(
    channel_mode: ChannelMode,
    speaker_labels: &SpeakerLabels,
    channel: &str,
) -> String {
    if channel_mode == ChannelMode::Mixed {
        return "merged".to_string();
    }
    match channel {
        "microphone" => speaker_labels.mic.clone(),
        "system-audio" => speaker_labels.system.clone(),
        _ => channel.to_string(),
    }
}

pub(super) fn write_runtime_job_wav(
    path: &Path,
    sample_rate_hz: u32,
    samples: &[f32],
) -> Result<(), CliError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sample_rate_hz,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|err| {
        CliError::new(format!(
            "failed to create runtime segment WAV {}: {err}",
            display_path(path)
        ))
    })?;
    for sample in samples {
        writer.write_sample(*sample).map_err(|err| {
            CliError::new(format!(
                "failed writing runtime segment WAV sample {}: {err}",
                display_path(path)
            ))
        })?;
    }
    writer.finalize().map_err(|err| {
        CliError::new(format!(
            "failed to finalize runtime segment WAV {}: {err}",
            display_path(path)
        ))
    })?;
    Ok(())
}

pub(super) fn runtime_phase_to_lifecycle_phase(phase: LiveRuntimePhase) -> LiveLifecyclePhase {
    match phase {
        LiveRuntimePhase::Warmup => LiveLifecyclePhase::Warmup,
        LiveRuntimePhase::Active => LiveLifecyclePhase::Active,
        LiveRuntimePhase::Draining => LiveLifecyclePhase::Draining,
        LiveRuntimePhase::Shutdown => LiveLifecyclePhase::Shutdown,
    }
}

pub(super) fn lifecycle_from_runtime_output_events(
    events: &[RuntimeOutputEvent],
) -> LiveLifecycleTelemetry {
    let mut lifecycle = LiveLifecycleTelemetry::new();
    for event in events {
        if let RuntimeOutputEvent::Lifecycle { phase, detail, .. } = event {
            lifecycle.transition(runtime_phase_to_lifecycle_phase(*phase), detail.clone());
        }
    }
    lifecycle
}

pub(super) fn transcript_events_from_runtime_output_events(
    config: &TranscribeConfig,
    events: &[RuntimeOutputEvent],
) -> Vec<TranscriptEvent> {
    let mut transcript_events = Vec::new();
    for event in events {
        let RuntimeOutputEvent::AsrCompleted { result, .. } = event else {
            continue;
        };
        transcript_events.extend(transcript_events_from_runtime_asr_result(
            config.channel_mode,
            &config.speaker_labels,
            result,
        ));
    }
    transcript_events
}

fn transcript_events_from_runtime_asr_result(
    channel_mode: ChannelMode,
    speaker_labels: &SpeakerLabels,
    result: &LiveAsrResult,
) -> Vec<TranscriptEvent> {
    let Some(stable_event) =
        transcript_event_from_runtime_asr_result(channel_mode, speaker_labels, result)
    else {
        return Vec::new();
    };

    if result.job.job_class != RuntimeAsrJobClass::Final {
        return vec![stable_event];
    }

    // Preserve legacy contract semantics: every final emits a companion partial.
    let partial_end_ms =
        stable_event.start_ms + ((stable_event.end_ms.saturating_sub(stable_event.start_ms)) / 2);
    let companion_partial = TranscriptEvent {
        event_type: "partial",
        channel: stable_event.channel.clone(),
        segment_id: stable_event.segment_id.clone(),
        start_ms: stable_event.start_ms,
        end_ms: partial_end_ms,
        text: partial_text(&stable_event.text),
        source_final_segment_id: None,
    };

    vec![companion_partial, stable_event]
}

pub(super) fn transcript_event_from_runtime_asr_result(
    channel_mode: ChannelMode,
    speaker_labels: &SpeakerLabels,
    result: &LiveAsrResult,
) -> Option<TranscriptEvent> {
    let event_type = match result.job.job_class {
        RuntimeAsrJobClass::Partial => "partial",
        RuntimeAsrJobClass::Final => "final",
        RuntimeAsrJobClass::Reconcile => "reconciled_final",
    };
    let text = result.transcript_text.trim();
    if text.is_empty() {
        return None;
    }
    let segment_id = if result.job.job_class == RuntimeAsrJobClass::Reconcile {
        format!("{}-reconciled", result.job.segment_id)
    } else {
        result.job.segment_id.clone()
    };
    Some(TranscriptEvent {
        event_type,
        channel: runtime_channel_label(channel_mode, speaker_labels, result.job.channel.as_str()),
        segment_id,
        start_ms: result.job.start_ms,
        end_ms: result.job.end_ms,
        text: text.to_string(),
        source_final_segment_id: if result.job.job_class == RuntimeAsrJobClass::Reconcile {
            Some(result.job.segment_id.clone())
        } else {
            None
        },
    })
}

pub(super) fn vad_boundaries_from_runtime_output_events(
    events: &[RuntimeOutputEvent],
) -> Vec<VadBoundary> {
    let mut boundaries = events
        .iter()
        .filter_map(|event| match event {
            RuntimeOutputEvent::AsrCompleted { result, .. }
                if matches!(
                    result.job.job_class,
                    RuntimeAsrJobClass::Final | RuntimeAsrJobClass::Reconcile
                ) =>
            {
                Some((result.job.start_ms, result.job.end_ms))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();

    boundaries
        .into_iter()
        .enumerate()
        .map(|(id, (start_ms, end_ms))| VadBoundary {
            id,
            start_ms,
            end_ms,
            source: "live_runtime",
        })
        .collect()
}

pub(super) fn fallback_vad_boundaries_from_events(events: &[TranscriptEvent]) -> Vec<VadBoundary> {
    let mut boundaries = events
        .iter()
        .filter(|event| matches!(event.event_type, "final" | "reconciled_final"))
        .map(|event| (event.start_ms, event.end_ms))
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();

    boundaries
        .into_iter()
        .enumerate()
        .map(|(id, (start_ms, end_ms))| VadBoundary {
            id,
            start_ms,
            end_ms,
            source: "live_runtime",
        })
        .collect()
}

pub(super) fn merge_live_transcript_events_for_display(
    events: Vec<TranscriptEvent>,
) -> Vec<TranscriptEvent> {
    events
}

pub(super) fn live_stream_chunk_queue_telemetry(
    config: &TranscribeConfig,
    runtime_summary: &LiveRuntimeSummary,
    asr_worker_pool: &LiveAsrPoolTelemetry,
) -> LiveChunkQueueTelemetry {
    let mut telemetry = LiveChunkQueueTelemetry::enabled(config.chunk_queue_cap);
    telemetry.submitted = runtime_summary.asr_jobs_queued as usize;
    telemetry.enqueued = asr_worker_pool.enqueued;
    telemetry.dropped_oldest = asr_worker_pool.dropped_queue_full;
    telemetry.processed = runtime_summary.asr_results_emitted as usize;
    telemetry.pending = runtime_summary.pending_jobs as usize;
    telemetry.high_water = telemetry.enqueued.min(telemetry.max_queue.max(1));
    telemetry.drain_completed = runtime_summary.final_phase == LiveRuntimePhase::Shutdown
        && runtime_summary.pending_jobs == 0;
    telemetry
}

pub(super) fn channel_transcript_summaries_from_events(
    config: &TranscribeConfig,
    events: &[TranscriptEvent],
) -> Vec<ChannelTranscriptSummary> {
    let finals = final_events_for_display(events);
    let mut by_channel = HashMap::<String, Vec<String>>::new();
    for event in finals {
        let text = event.text.trim();
        if text.is_empty() {
            continue;
        }
        by_channel
            .entry(event.channel.clone())
            .or_default()
            .push(text.to_string());
    }

    let mut summaries = by_channel
        .into_iter()
        .map(|(channel, lines)| {
            let role = if channel == "merged" {
                "mixed"
            } else if channel == config.speaker_labels.mic {
                "mic"
            } else if channel == config.speaker_labels.system {
                "system"
            } else {
                "mixed"
            };
            ChannelTranscriptSummary {
                role,
                label: channel,
                text: lines.join(" "),
            }
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|a, b| {
        channel_sort_key(a.role)
            .cmp(&channel_sort_key(b.role))
            .then_with(|| a.label.cmp(&b.label))
    });
    summaries
}

pub(super) fn active_channel_mode_from_transcripts(
    config: &TranscribeConfig,
    channel_transcripts: &[ChannelTranscriptSummary],
) -> ChannelMode {
    if config.channel_mode == ChannelMode::Mixed {
        return ChannelMode::Mixed;
    }
    let has_mic = channel_transcripts
        .iter()
        .any(|summary| summary.role == "mic");
    let has_system = channel_transcripts
        .iter()
        .any(|summary| summary.role == "system");
    if has_mic && has_system {
        ChannelMode::Separate
    } else {
        ChannelMode::Mixed
    }
}
