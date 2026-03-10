use super::*;

/// Minimum character length for a stable-partial promotion.
/// Setting this too low (e.g. 6) creates choppy, ultra-short transcript lines
/// in the live GUI.  A higher threshold batches more text before committing,
/// producing more natural reading-length chunks.  15 chars is roughly 2-3
/// words — fast enough for realtime feedback without fragmentation.
const STABLE_PARTIAL_MIN_CHARS: usize = 15;
const STABLE_PARTIAL_MIN_OBSERVATIONS: usize = 2;
const STABLE_PARTIAL_STABILITY_WINDOW_MS: u64 = 500;

#[derive(Debug, Clone, Default)]
struct StablePartialState {
    promoted_prefix: String,
    last_partial_text: String,
    last_partial_end_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub(super) struct LiveTranscriptDisplayReducer {
    state_by_segment: HashMap<(String, String), StablePartialState>,
}

fn stable_partial_key(event: &TranscriptEvent) -> (String, String) {
    let lineage = event
        .source_final_segment_id
        .clone()
        .unwrap_or_else(|| event.segment_id.clone());
    (event.channel.clone(), lineage)
}

fn trim_to_word_boundary(text: &str) -> String {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed
        .chars()
        .last()
        .is_some_and(|ch| ch.is_whitespace() || matches!(ch, '.' | ',' | '!' | '?' | ';' | ':'))
    {
        return trimmed.to_string();
    }
    let mut boundary = 0usize;
    for (idx, ch) in trimmed.char_indices() {
        if ch.is_whitespace() || matches!(ch, '.' | ',' | '!' | '?' | ';' | ':') {
            boundary = idx + ch.len_utf8();
        }
    }
    trimmed[..boundary].trim_end().to_string()
}

fn longest_common_prefix(a: &str, b: &str) -> String {
    let mut prefix_end = 0usize;
    for ((a_idx, a_ch), (_, b_ch)) in a.char_indices().zip(b.char_indices()) {
        if a_ch != b_ch {
            break;
        }
        prefix_end = a_idx + a_ch.len_utf8();
    }
    a[..prefix_end].to_string()
}

fn trim_promoted_prefix(text: &str, promoted_prefix: &str) -> String {
    if promoted_prefix.is_empty() {
        return text.trim().to_string();
    }
    let trimmed_text = text.trim();
    let trimmed_prefix = promoted_prefix.trim();
    if let Some(stripped) = trimmed_text.strip_prefix(trimmed_prefix) {
        return stripped.trim_start().to_string();
    }
    trimmed_text.to_string()
}

fn stable_prefix_candidate(state: &StablePartialState, current_text: &str) -> String {
    let common_prefix = trim_to_word_boundary(&longest_common_prefix(
        &state.last_partial_text,
        current_text,
    ));
    if common_prefix.len() <= state.promoted_prefix.len() {
        return String::new();
    }
    let previous_word_boundary = trim_to_word_boundary(&state.last_partial_text);
    if previous_word_boundary.len() > state.promoted_prefix.len()
        && current_text.starts_with(&previous_word_boundary)
    {
        return previous_word_boundary;
    }
    common_prefix
}

fn push_stable_partial_delta(
    output: &mut Vec<TranscriptEvent>,
    event: &TranscriptEvent,
    state: &mut StablePartialState,
    promoted_prefix: &str,
) {
    if promoted_prefix.len() <= state.promoted_prefix.len() {
        return;
    }
    let delta = promoted_prefix[state.promoted_prefix.len()..].trim();
    if delta.is_empty() {
        state.promoted_prefix = promoted_prefix.to_string();
        return;
    }
    output.push(TranscriptEvent {
        event_type: "stable_partial",
        channel: event.channel.clone(),
        segment_id: event.segment_id.clone(),
        start_ms: event.start_ms,
        end_ms: event.end_ms,
        text: delta.to_string(),
        source_final_segment_id: event.source_final_segment_id.clone(),
    });
    state.promoted_prefix = promoted_prefix.to_string();
}

fn reduce_partial_event(
    output: &mut Vec<TranscriptEvent>,
    state: &mut StablePartialState,
    event: TranscriptEvent,
) {
    let current_text = event.text.trim().to_string();
    if current_text.is_empty() {
        return;
    }
    if state.last_partial_text.is_empty() {
        state.last_partial_text = current_text.clone();
        state.last_partial_end_ms = Some(event.end_ms);
        output.push(TranscriptEvent {
            text: trim_promoted_prefix(&current_text, &state.promoted_prefix),
            ..event
        });
        return;
    }

    let stable_prefix = stable_prefix_candidate(state, &current_text);
    let stable_duration_ms = state
        .last_partial_end_ms
        .map(|last_end_ms| event.end_ms.saturating_sub(last_end_ms))
        .unwrap_or(0);
    if stable_prefix.len() >= STABLE_PARTIAL_MIN_CHARS
        && (STABLE_PARTIAL_MIN_OBSERVATIONS <= 2
            || stable_duration_ms >= STABLE_PARTIAL_STABILITY_WINDOW_MS)
    {
        push_stable_partial_delta(output, &event, state, &stable_prefix);
    }

    state.last_partial_text = current_text.clone();
    state.last_partial_end_ms = Some(event.end_ms);
    let suffix_text = trim_promoted_prefix(&current_text, &state.promoted_prefix);
    if !suffix_text.is_empty() {
        output.push(TranscriptEvent {
            text: suffix_text,
            ..event
        });
    }
}

fn reduce_stable_event(
    output: &mut Vec<TranscriptEvent>,
    state: &mut StablePartialState,
    event: TranscriptEvent,
) {
    let suffix_text = trim_promoted_prefix(&event.text, &state.promoted_prefix);
    if !suffix_text.is_empty() {
        output.push(TranscriptEvent {
            text: suffix_text,
            ..event
        });
    }
    *state = StablePartialState::default();
}

impl LiveTranscriptDisplayReducer {
    pub(super) fn process_event(&mut self, event: TranscriptEvent) -> Vec<TranscriptEvent> {
        let mut output = Vec::new();
        match event.event_type {
            "partial" => {
                let key = stable_partial_key(&event);
                let state = self.state_by_segment.entry(key).or_default();
                reduce_partial_event(&mut output, state, event);
            }
            "final" | "reconciled_final" => {
                let key = stable_partial_key(&event);
                let state = self.state_by_segment.entry(key.clone()).or_default();
                reduce_stable_event(&mut output, state, event);
                self.state_by_segment.remove(&key);
            }
            _ => output.push(event),
        }
        output
    }
}

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
    let mut ordered = merge_transcript_events(events);
    let mut reducer = LiveTranscriptDisplayReducer::default();
    let mut output = Vec::new();
    for event in ordered.drain(..) {
        output.extend(reducer.process_event(event));
    }
    output
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

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        event_type: &'static str,
        segment_id: &str,
        end_ms: u64,
        text: &str,
    ) -> TranscriptEvent {
        TranscriptEvent {
            event_type,
            channel: "system".to_string(),
            segment_id: segment_id.to_string(),
            start_ms: 0,
            end_ms,
            text: text.to_string(),
            source_final_segment_id: None,
        }
    }

    #[test]
    fn stable_partial_promotes_prefix_and_final_emits_only_suffix() {
        let merged = merge_live_transcript_events_for_display(vec![
            event(
                "partial",
                "seg-1",
                1000,
                "how we handled the project schedule for the quarter",
            ),
            event(
                "partial",
                "seg-1",
                1300,
                "how we handled the project schedule for the quarter tasks",
            ),
            event(
                "partial",
                "seg-1",
                1600,
                "how we handled the project schedule for the quarter tasks today",
            ),
            event(
                "final",
                "seg-1",
                2200,
                "how we handled the project schedule for the quarter tasks today",
            ),
        ]);

        let rendered = merged
            .iter()
            .map(|evt| (evt.event_type, evt.text.as_str()))
            .collect::<Vec<_>>();

        assert!(rendered.iter().any(|(kind, text)| {
            *kind == "stable_partial" && text.starts_with("how we handled")
        }));
        assert!(rendered
            .iter()
            .any(|(kind, text)| { *kind == "partial" && text.contains("tasks") }));
        assert!(rendered
            .iter()
            .any(|(kind, text)| { *kind == "final" && text.contains("tasks") }));
        assert!(!rendered.contains(&(
            "final",
            "how we handled the project schedule for the quarter tasks today"
        )));
    }

    #[test]
    fn stable_partial_never_commits_mid_word_prefix() {
        let merged = merge_live_transcript_events_for_display(vec![
            event(
                "partial",
                "seg-1",
                1000,
                "architectural visualization of the design concept under review",
            ),
            event(
                "partial",
                "seg-1",
                1300,
                "architectural visualization of the design concepts under review",
            ),
        ]);

        // The common prefix "architectural visualization of the design concept" is long enough
        // but "concept" vs "concepts" means the word-boundary trim should NOT commit "concept"
        // partially (the word boundary would be "architectural visualization of the design ").
        assert!(!merged
            .iter()
            .any(|evt| { evt.event_type == "stable_partial" && evt.text.contains("concept") }));
    }

    #[test]
    fn stable_partial_state_is_isolated_per_segment() {
        let merged = merge_live_transcript_events_for_display(vec![
            event(
                "partial",
                "seg-a",
                1000,
                "hello there world this is a much longer partial for segment a",
            ),
            event(
                "partial",
                "seg-b",
                1050,
                "different stream entirely with a completely separate text body",
            ),
            event(
                "partial",
                "seg-a",
                1300,
                "hello there world this is a much longer partial for segment a again",
            ),
            event(
                "final",
                "seg-b",
                1800,
                "different stream entirely with a completely separate text body",
            ),
        ]);

        assert!(merged.iter().any(|evt| {
            evt.event_type == "stable_partial"
                && evt.segment_id == "seg-a"
                && evt.text.starts_with("hello there")
        }));
        assert!(merged.iter().any(|evt| {
            evt.event_type == "final"
                && evt.segment_id == "seg-b"
                && evt.text.contains("different stream")
        }));
    }

    #[test]
    fn display_reducer_supports_incremental_live_path() {
        let mut reducer = LiveTranscriptDisplayReducer::default();
        let first = reducer.process_event(event(
            "partial",
            "seg-live",
            1000,
            "how we handled the project schedule for the quarter",
        ));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].event_type, "partial");

        let second = reducer.process_event(event(
            "partial",
            "seg-live",
            1300,
            "how we handled the project schedule for the quarter tasks",
        ));
        assert_eq!(second.len(), 2);
        assert_eq!(second[0].event_type, "stable_partial");
        assert!(second[0].text.starts_with("how we handled"));
        assert_eq!(second[1].event_type, "partial");
        assert!(second[1].text.contains("tasks"));

        let final_events = reducer.process_event(event(
            "final",
            "seg-live",
            2000,
            "how we handled the project schedule for the quarter tasks",
        ));
        assert_eq!(final_events.len(), 1);
        assert_eq!(final_events[0].event_type, "final");
        assert!(final_events[0].text.contains("tasks"));
    }
}
