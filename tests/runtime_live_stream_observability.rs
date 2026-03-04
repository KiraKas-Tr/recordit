use recordit::live_stream_runtime::{
    BackpressureMode, BackpressureTransitionReason, CaptureScheduler, LiveRuntimePhase,
    SchedulerDiagnosticsSnapshot, SchedulerPressureSnapshot, SchedulingInput,
    StreamingSchedulerConfig, StreamingVadConfig, StreamingVadScheduler,
};

fn manual_input(
    channel: &str,
    pts_ms: u64,
    duration_ms: u64,
    frame_count: usize,
    activity_level_per_mille: u16,
) -> SchedulingInput {
    SchedulingInput {
        channel: channel.to_string(),
        pts_ms,
        frame_count,
        duration_ms,
        activity_level_per_mille,
    }
}

fn test_vad_config() -> StreamingVadConfig {
    StreamingVadConfig {
        rolling_window_ms: 3_000,
        min_speech_ms: 20,
        min_silence_ms: 20,
        open_threshold_per_mille: 40,
        close_threshold_per_mille: 20,
    }
}

fn test_scheduler_config() -> StreamingSchedulerConfig {
    let mut cfg = StreamingSchedulerConfig::default();
    cfg.partial_window_ms = 40;
    cfg.partial_stride_ms = 20;
    cfg.min_partial_span_ms = 20;
    cfg
}

fn diagnostics_breadcrumbs(diag: &SchedulerDiagnosticsSnapshot) -> String {
    let mut channels = diag
        .channels
        .iter()
        .map(|(channel, details)| {
            format!(
                "{channel}{{pressure:{} severe:{} pending:{} pending_final:{} queued_p:{} queued_f:{} queued_r:{}}}",
                details.pressure_samples,
                details.severe_samples,
                details.last_pending_jobs,
                details.last_pending_final_jobs,
                details.queued_partials,
                details.queued_finals,
                details.queued_reconciles
            )
        })
        .collect::<Vec<_>>();
    channels.sort();
    format!(
        "mode={} transition_count={} last_reason={:?} channels={}",
        diag.backpressure_mode.as_str(),
        diag.backpressure_transition_count,
        diag.last_backpressure_transition_reason,
        channels.join("|")
    )
}

#[test]
fn persistent_transition_reason_survives_transition_queue_drain() {
    let mut scheduler_cfg = test_scheduler_config();
    scheduler_cfg.backpressure.pressure_pending_jobs_threshold = 1;
    scheduler_cfg.backpressure.pressure_min_observations = 1;
    scheduler_cfg.backpressure.severe_pending_jobs_threshold = 100;
    scheduler_cfg
        .backpressure
        .severe_pending_final_jobs_threshold = 100;
    scheduler_cfg.backpressure.severe_min_observations = 10;

    let mut scheduler = StreamingVadScheduler::with_configs(test_vad_config(), scheduler_cfg);
    let _ = scheduler.on_capture(
        manual_input("microphone", 0, 20, 960, 90),
        LiveRuntimePhase::Active,
        SchedulerPressureSnapshot {
            pending_jobs: 1,
            pending_final_jobs: 0,
        },
    );
    let _ = scheduler.on_capture(
        manual_input("microphone", 20, 20, 960, 90),
        LiveRuntimePhase::Active,
        SchedulerPressureSnapshot {
            pending_jobs: 1,
            pending_final_jobs: 0,
        },
    );
    let drained = scheduler.drain_backpressure_transitions();
    assert_eq!(drained.len(), 1);

    let diag = scheduler.diagnostics_snapshot();
    let breadcrumbs = diagnostics_breadcrumbs(&diag);
    assert_eq!(
        diag.backpressure_mode,
        BackpressureMode::Pressure,
        "{breadcrumbs}"
    );
    assert_eq!(diag.backpressure_transition_count, 1, "{breadcrumbs}");
    assert_eq!(
        diag.last_backpressure_transition_reason,
        Some(BackpressureTransitionReason::PendingJobsSustained),
        "{breadcrumbs}"
    );
}

#[test]
fn diagnostics_snapshot_reports_per_channel_pressure_and_queue_counts() {
    let mut scheduler_cfg = test_scheduler_config();
    scheduler_cfg.backpressure.pressure_pending_jobs_threshold = 1;
    scheduler_cfg.backpressure.pressure_min_observations = 1;
    scheduler_cfg.backpressure.severe_pending_jobs_threshold = 100;
    scheduler_cfg
        .backpressure
        .severe_pending_final_jobs_threshold = 100;
    scheduler_cfg.backpressure.severe_min_observations = 10;

    let mut scheduler = StreamingVadScheduler::with_configs(test_vad_config(), scheduler_cfg);
    for (channel, pts_ms, pending_jobs) in [
        ("microphone", 0, 0),
        ("system-audio", 0, 0),
        ("microphone", 20, 1),
        ("system-audio", 20, 1),
    ] {
        let _ = scheduler.on_capture(
            manual_input(channel, pts_ms, 20, 960, 95),
            LiveRuntimePhase::Active,
            SchedulerPressureSnapshot {
                pending_jobs,
                pending_final_jobs: 0,
            },
        );
    }
    let _ = scheduler.on_phase_change(LiveRuntimePhase::Draining);
    let diag = scheduler.diagnostics_snapshot();
    let breadcrumbs = diagnostics_breadcrumbs(&diag);

    assert_eq!(
        diag.backpressure_mode,
        BackpressureMode::Normal,
        "{breadcrumbs}"
    );
    assert!(diag.backpressure_transition_count >= 1, "{breadcrumbs}");
    assert_eq!(
        diag.last_backpressure_transition_reason,
        Some(BackpressureTransitionReason::PhaseBoundaryReset),
        "{breadcrumbs}"
    );

    let mic = diag
        .channels
        .get("microphone")
        .expect("missing microphone diagnostics");
    let sys = diag
        .channels
        .get("system-audio")
        .expect("missing system-audio diagnostics");
    assert!(
        mic.pressure_samples >= 1 && sys.pressure_samples >= 1,
        "{breadcrumbs}"
    );
    assert_eq!(mic.last_pending_jobs, 1, "{breadcrumbs}");
    assert_eq!(sys.last_pending_jobs, 1, "{breadcrumbs}");
    assert!(mic.queued_finals + sys.queued_finals >= 1, "{breadcrumbs}");
}
