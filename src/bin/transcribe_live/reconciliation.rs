use super::*;

#[cfg(test)]
pub(super) fn build_reconciliation_events(
    channel_transcripts: &[ChannelTranscriptSummary],
    vad_boundaries: &[VadBoundary],
) -> Vec<TranscriptEvent> {
    let ordered_transcripts = ordered_channel_transcripts(channel_transcripts);
    let ordered_boundaries = ordered_vad_boundaries_for_segments(vad_boundaries);
    let items = ordered_transcripts
        .iter()
        .flat_map(|transcript| {
            build_transcript_events(
                &transcript.text,
                &ordered_boundaries,
                &transcript.label,
                transcript.role,
                false,
                DEFAULT_CHUNK_WINDOW_MS,
                DEFAULT_CHUNK_STRIDE_MS,
            )
        })
        .filter_map(|event| {
            if event.event_type != "final" {
                return None;
            }
            Some(AsrWorkItem {
                class: AsrWorkClass::Reconcile,
                tick_index: 0,
                channel: event.channel,
                segment_id: format!("{}-reconciled", event.segment_id),
                start_ms: event.start_ms,
                end_ms: event.end_ms,
                text: event.text,
                source_final_segment_id: Some(event.segment_id),
            })
        })
        .collect::<Vec<_>>();
    items
        .into_iter()
        .flat_map(emit_asr_work_item_events)
        .collect()
}

pub(super) fn build_targeted_reconciliation_events(
    channel_transcripts: &[ChannelTranscriptSummary],
    vad_boundaries: &[VadBoundary],
    live_events: &[TranscriptEvent],
    reconciliation: &ReconciliationMatrix,
) -> Vec<TranscriptEvent> {
    let ordered_transcripts = ordered_channel_transcripts(channel_transcripts);
    let ordered_boundaries = ordered_vad_boundaries_for_segments(vad_boundaries);
    if ordered_transcripts.is_empty() || ordered_boundaries.is_empty() || !reconciliation.required {
        return Vec::new();
    }

    let trigger_codes = reconciliation
        .triggers
        .iter()
        .map(|trigger| trigger.code)
        .collect::<HashSet<_>>();
    let live_final_ids = live_events
        .iter()
        .filter(|event| event.event_type == "final")
        .map(|event| event.segment_id.clone())
        .collect::<HashSet<_>>();

    let capture_integrity_triggered = trigger_codes.contains("continuity_recovered_with_gaps")
        || trigger_codes.contains("continuity_unverified")
        || trigger_codes.contains("capture_transport_degraded")
        || trigger_codes.contains("capture_callback_contract_degraded");
    let queue_drop_triggered = trigger_codes.contains("chunk_queue_drop_oldest");
    let shutdown_flush_triggered = trigger_codes.contains("shutdown_flush_boundary");

    let mut targeted_boundary_indexes = BTreeSet::new();
    if capture_integrity_triggered {
        targeted_boundary_indexes.extend(0..ordered_boundaries.len());
    }

    if shutdown_flush_triggered {
        for (boundary_idx, boundary) in ordered_boundaries.iter().enumerate() {
            if boundary.source == "shutdown_flush" {
                targeted_boundary_indexes.insert(boundary_idx);
            }
        }
    }

    if queue_drop_triggered {
        for (boundary_idx, boundary) in ordered_boundaries.iter().enumerate() {
            let missing_final = ordered_transcripts.iter().any(|transcript| {
                let expected_segment_id = near_live_boundary_segment_id(
                    transcript.role,
                    boundary_idx,
                    boundary.start_ms,
                    boundary.end_ms,
                );
                !live_final_ids.contains(&expected_segment_id)
            });
            if missing_final {
                targeted_boundary_indexes.insert(boundary_idx);
            }
        }
    }

    if targeted_boundary_indexes.is_empty() {
        targeted_boundary_indexes.extend(0..ordered_boundaries.len());
    }

    let boundary_count = ordered_boundaries.len();
    let mut items = Vec::new();
    for boundary_idx in targeted_boundary_indexes {
        let boundary = &ordered_boundaries[boundary_idx];
        for transcript in &ordered_transcripts {
            let source_final_segment_id = near_live_boundary_segment_id(
                transcript.role,
                boundary_idx,
                boundary.start_ms,
                boundary.end_ms,
            );
            let segment_text = chunk_scoped_text(&transcript.text, boundary_idx, boundary_count);
            if segment_text.trim().is_empty() {
                continue;
            }
            items.push(AsrWorkItem {
                class: AsrWorkClass::Reconcile,
                tick_index: 0,
                channel: transcript.label.clone(),
                segment_id: format!("{source_final_segment_id}-reconciled"),
                start_ms: boundary.start_ms,
                end_ms: boundary.end_ms,
                text: segment_text,
                source_final_segment_id: Some(source_final_segment_id),
            });
        }
    }

    items
        .into_iter()
        .flat_map(emit_asr_work_item_events)
        .collect()
}

pub(super) fn build_reconciliation_matrix(
    vad_boundaries: &[VadBoundary],
    degradation_events: &[ModeDegradationEvent],
) -> ReconciliationMatrix {
    let mut triggers = Vec::new();
    let mut seen_codes = HashSet::new();

    for degradation in degradation_events {
        let trigger_code = match degradation.code {
            LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE => Some("chunk_queue_drop_oldest"),
            LIVE_CAPTURE_INTERRUPTION_RECOVERED_CODE => Some("continuity_recovered_with_gaps"),
            LIVE_CAPTURE_CONTINUITY_UNVERIFIED_CODE => Some("continuity_unverified"),
            LIVE_CAPTURE_TRANSPORT_DEGRADED_CODE => Some("capture_transport_degraded"),
            LIVE_CAPTURE_CALLBACK_CONTRACT_DEGRADED_CODE => {
                Some("capture_callback_contract_degraded")
            }
            _ => None,
        };
        if let Some(code) = trigger_code {
            if seen_codes.insert(code) {
                triggers.push(ReconciliationTrigger { code });
            }
        }
    }

    if vad_boundaries
        .iter()
        .any(|boundary| boundary.source == "shutdown_flush")
        && seen_codes.insert("shutdown_flush_boundary")
    {
        triggers.push(ReconciliationTrigger {
            code: "shutdown_flush_boundary",
        });
    }

    ReconciliationMatrix {
        required: !triggers.is_empty(),
        applied: false,
        triggers,
    }
}
