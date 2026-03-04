use recordit::live_stream_runtime::{
    AdaptiveBackpressureConfig, BackpressureMode, BackpressureTransition,
    BackpressureTransitionReason, CaptureScheduler, LiveAsrJobDraft, LiveRuntimePhase,
    SchedulerPressureSnapshot, SchedulingInput, StreamingSchedulerConfig, StreamingVadConfig,
    StreamingVadScheduler,
};
use std::collections::BTreeMap;

fn scripted_input(channel: &str, pts_ms: u64, duration_ms: u64, activity: u16) -> SchedulingInput {
    SchedulingInput {
        channel: channel.to_string(),
        pts_ms,
        frame_count: duration_ms as usize,
        duration_ms,
        activity_level_per_mille: activity,
    }
}

fn fairness_scheduler_config() -> StreamingSchedulerConfig {
    StreamingSchedulerConfig {
        partial_window_ms: 300,
        partial_stride_ms: 100,
        min_partial_span_ms: 80,
        backpressure: AdaptiveBackpressureConfig {
            pressure_window_ms: 600,
            severe_window_ms: 600,
            recovery_window_ms: 800,
            pressure_pending_jobs_threshold: 1,
            severe_pending_jobs_threshold: 100,
            severe_pending_final_jobs_threshold: 100,
            pressure_min_observations: 1,
            severe_min_observations: 2,
            pressure_stride_multiplier: 3,
        },
    }
}

fn severe_scheduler_config() -> StreamingSchedulerConfig {
    StreamingSchedulerConfig {
        partial_window_ms: 300,
        partial_stride_ms: 60,
        min_partial_span_ms: 50,
        backpressure: AdaptiveBackpressureConfig {
            pressure_window_ms: 600,
            severe_window_ms: 600,
            recovery_window_ms: 800,
            pressure_pending_jobs_threshold: 100,
            severe_pending_jobs_threshold: 100,
            severe_pending_final_jobs_threshold: 1,
            pressure_min_observations: 2,
            severe_min_observations: 1,
            pressure_stride_multiplier: 2,
        },
    }
}

fn test_vad_config() -> StreamingVadConfig {
    StreamingVadConfig {
        rolling_window_ms: 2_000,
        min_speech_ms: 200,
        min_silence_ms: 120,
        open_threshold_per_mille: 40,
        close_threshold_per_mille: 20,
    }
}

fn collect_counts(jobs: &[LiveAsrJobDraft]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for job in jobs {
        let key = format!("{}:{}", job.job_class.as_str(), job.channel);
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

fn assert_channel_fairness(
    jobs: &[LiveAsrJobDraft],
    transitions: &[BackpressureTransition],
    require_zero_partials: bool,
) {
    let counts = collect_counts(jobs);
    let mic_final = *counts.get("final:microphone").unwrap_or(&0);
    let sys_final = *counts.get("final:system-audio").unwrap_or(&0);
    let mic_partial = *counts.get("partial:microphone").unwrap_or(&0);
    let sys_partial = *counts.get("partial:system-audio").unwrap_or(&0);
    let partial_skew = mic_partial.abs_diff(sys_partial);

    assert!(
        mic_final >= 1 && sys_final >= 1,
        "expected both channels to emit finals under pressure; counts={counts:?} transitions={transitions:?}"
    );
    assert_eq!(
        mic_final, sys_final,
        "final-event fairness skewed across channels; counts={counts:?} transitions={transitions:?}"
    );

    if require_zero_partials {
        assert_eq!(
            mic_partial + sys_partial,
            0,
            "severe mode should suppress partials; counts={counts:?} transitions={transitions:?}"
        );
    } else {
        assert!(
            partial_skew <= 1,
            "partial-event fairness skew exceeded threshold (skew={partial_skew}); counts={counts:?} transitions={transitions:?}"
        );
    }
}

#[test]
fn pressure_mode_preserves_per_channel_final_fairness_and_bounded_partial_skew() {
    let mut scheduler =
        StreamingVadScheduler::with_configs(test_vad_config(), fairness_scheduler_config());
    let mut jobs = Vec::new();
    let pressure = SchedulerPressureSnapshot {
        pending_jobs: 2,
        pending_final_jobs: 0,
    };

    for input in [
        scripted_input("microphone", 0, 120, 120),
        scripted_input("system-audio", 0, 120, 120),
        scripted_input("microphone", 120, 120, 120),
        scripted_input("system-audio", 120, 120, 120),
        scripted_input("microphone", 240, 140, 0),
        scripted_input("system-audio", 240, 140, 0),
    ] {
        jobs.extend(CaptureScheduler::on_capture(
            &mut scheduler,
            input,
            LiveRuntimePhase::Active,
            pressure,
        ));
    }

    let transitions = scheduler.drain_backpressure_transitions();
    jobs.extend(CaptureScheduler::on_phase_change(
        &mut scheduler,
        LiveRuntimePhase::Draining,
    ));

    assert!(
        transitions
            .iter()
            .any(|t| t.to_mode == BackpressureMode::Pressure),
        "expected a pressure-mode transition; transitions={transitions:?}"
    );
    assert_channel_fairness(&jobs, &transitions, false);
}

#[test]
fn severe_mode_suppresses_partials_without_channel_starvation() {
    let mut scheduler =
        StreamingVadScheduler::with_configs(test_vad_config(), severe_scheduler_config());
    let mut jobs = Vec::new();
    let severe_pressure = SchedulerPressureSnapshot {
        pending_jobs: 0,
        pending_final_jobs: 2,
    };

    for input in [
        scripted_input("microphone", 0, 120, 120),
        scripted_input("system-audio", 0, 120, 120),
        scripted_input("microphone", 120, 120, 120),
        scripted_input("system-audio", 120, 120, 120),
        scripted_input("microphone", 240, 140, 0),
        scripted_input("system-audio", 240, 140, 0),
    ] {
        jobs.extend(CaptureScheduler::on_capture(
            &mut scheduler,
            input,
            LiveRuntimePhase::Active,
            severe_pressure,
        ));
    }

    let transitions = scheduler.drain_backpressure_transitions();
    jobs.extend(CaptureScheduler::on_phase_change(
        &mut scheduler,
        LiveRuntimePhase::Draining,
    ));

    assert!(
        transitions.iter().any(|t| {
            t.to_mode == BackpressureMode::Severe
                && t.reason == BackpressureTransitionReason::PendingFinalJobsSustained
        }),
        "expected severe-mode transition from pending final backlog; transitions={transitions:?}"
    );
    assert_channel_fairness(&jobs, &transitions, true);
}

#[test]
fn pressure_fairness_failure_output_includes_diff_style_counts_and_transition_trace() {
    let mut scheduler =
        StreamingVadScheduler::with_configs(test_vad_config(), fairness_scheduler_config());
    let pressure = SchedulerPressureSnapshot {
        pending_jobs: 2,
        pending_final_jobs: 0,
    };
    let mut jobs = Vec::new();

    jobs.extend(CaptureScheduler::on_capture(
        &mut scheduler,
        scripted_input("microphone", 0, 120, 120),
        LiveRuntimePhase::Active,
        pressure,
    ));
    jobs.extend(CaptureScheduler::on_capture(
        &mut scheduler,
        scripted_input("microphone", 120, 120, 120),
        LiveRuntimePhase::Active,
        pressure,
    ));
    jobs.extend(CaptureScheduler::on_capture(
        &mut scheduler,
        scripted_input("system-audio", 0, 120, 120),
        LiveRuntimePhase::Active,
        pressure,
    ));
    jobs.extend(CaptureScheduler::on_capture(
        &mut scheduler,
        scripted_input("system-audio", 120, 120, 120),
        LiveRuntimePhase::Active,
        pressure,
    ));
    jobs.extend(CaptureScheduler::on_capture(
        &mut scheduler,
        scripted_input("microphone", 240, 140, 0),
        LiveRuntimePhase::Active,
        pressure,
    ));
    jobs.extend(CaptureScheduler::on_capture(
        &mut scheduler,
        scripted_input("system-audio", 240, 140, 0),
        LiveRuntimePhase::Active,
        pressure,
    ));
    let transitions = scheduler.drain_backpressure_transitions();
    jobs.extend(CaptureScheduler::on_phase_change(
        &mut scheduler,
        LiveRuntimePhase::Draining,
    ));

    let counts = collect_counts(&jobs);
    assert!(
        counts.contains_key("final:microphone") && counts.contains_key("final:system-audio"),
        "missing per-channel final counters for fairness triage; counts={counts:?} transitions={transitions:?}"
    );
}
