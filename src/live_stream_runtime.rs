use crate::capture_api::{CaptureChunk, CaptureEvent, CaptureEventCode, CaptureStream};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRuntimePhase {
    Warmup,
    Active,
    Draining,
    Shutdown,
}

impl LiveRuntimePhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Active => "active",
            Self::Draining => "draining",
            Self::Shutdown => "shutdown",
        }
    }

    pub const fn ready_for_transcripts(self) -> bool {
        !matches!(self, Self::Warmup)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleTransition {
    pub phase: LiveRuntimePhase,
    pub entered_at_utc: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct LiveRuntimeState {
    pub current_phase: LiveRuntimePhase,
    pub ready_for_transcripts: bool,
    pub transitions: Vec<LifecycleTransition>,
    pub capture_chunks_seen: u64,
    pub capture_events_seen: u64,
    pub asr_jobs_queued: u64,
    pub asr_results_emitted: u64,
    next_emit_seq: u64,
}

impl LiveRuntimeState {
    fn new() -> Self {
        Self {
            current_phase: LiveRuntimePhase::Warmup,
            ready_for_transcripts: false,
            transitions: vec![LifecycleTransition {
                phase: LiveRuntimePhase::Warmup,
                entered_at_utc: runtime_timestamp_utc(),
                detail: "coordinator initialized".to_string(),
            }],
            capture_chunks_seen: 0,
            capture_events_seen: 0,
            asr_jobs_queued: 0,
            asr_results_emitted: 0,
            next_emit_seq: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveAsrJobClass {
    Partial,
    Final,
    Reconcile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAsrJobDraft {
    pub class: LiveAsrJobClass,
    pub channel: String,
    pub segment_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAsrJobSpec {
    pub emit_seq: u64,
    pub class: LiveAsrJobClass,
    pub channel: String,
    pub segment_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAsrResult {
    pub job: LiveAsrJobSpec,
    pub transcript_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingInput {
    pub channel: String,
    pub pts_ms: u64,
    pub frame_count: usize,
}

impl SchedulingInput {
    pub fn from_capture_chunk(chunk: &CaptureChunk) -> Self {
        Self {
            channel: match chunk.stream {
                CaptureStream::Microphone => "microphone".to_string(),
                CaptureStream::SystemAudio => "system-audio".to_string(),
            },
            pts_ms: seconds_to_millis(chunk.pts_seconds),
            frame_count: chunk.mono_samples.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeOutputEvent {
    Lifecycle {
        emit_seq: u64,
        phase: LiveRuntimePhase,
        detail: String,
    },
    CaptureEvent {
        emit_seq: u64,
        code: String,
        detail: String,
        count: u64,
    },
    AsrQueued {
        emit_seq: u64,
        job: LiveAsrJobSpec,
    },
    AsrCompleted {
        emit_seq: u64,
        result: LiveAsrResult,
    },
}

impl RuntimeOutputEvent {
    pub const fn emit_seq(&self) -> u64 {
        match self {
            Self::Lifecycle { emit_seq, .. }
            | Self::CaptureEvent { emit_seq, .. }
            | Self::AsrQueued { emit_seq, .. }
            | Self::AsrCompleted { emit_seq, .. } => *emit_seq,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRuntimeSummary {
    pub final_phase: LiveRuntimePhase,
    pub ready_for_transcripts: bool,
    pub transition_count: usize,
    pub capture_chunks_seen: u64,
    pub capture_events_seen: u64,
    pub asr_jobs_queued: u64,
    pub asr_results_emitted: u64,
}

pub trait CaptureScheduler {
    fn on_capture(
        &mut self,
        input: SchedulingInput,
        phase: LiveRuntimePhase,
    ) -> Vec<LiveAsrJobDraft>;

    fn on_phase_change(&mut self, _phase: LiveRuntimePhase) -> Vec<LiveAsrJobDraft> {
        Vec::new()
    }
}

pub trait RuntimeOutputSink {
    fn emit(&mut self, event: RuntimeOutputEvent) -> Result<(), String>;
}

pub trait RuntimeFinalizer {
    fn finalize(&mut self, summary: &LiveRuntimeSummary) -> Result<(), String>;
}

pub struct LiveStreamCoordinator<S, O, F>
where
    S: CaptureScheduler,
    O: RuntimeOutputSink,
    F: RuntimeFinalizer,
{
    state: LiveRuntimeState,
    scheduler: S,
    output: O,
    finalizer: F,
    pending_jobs: VecDeque<LiveAsrJobSpec>,
}

impl<S, O, F> LiveStreamCoordinator<S, O, F>
where
    S: CaptureScheduler,
    O: RuntimeOutputSink,
    F: RuntimeFinalizer,
{
    pub fn new(scheduler: S, output: O, finalizer: F) -> Self {
        Self {
            state: LiveRuntimeState::new(),
            scheduler,
            output,
            finalizer,
            pending_jobs: VecDeque::new(),
        }
    }

    pub fn state(&self) -> &LiveRuntimeState {
        &self.state
    }

    pub fn transition_to(
        &mut self,
        phase: LiveRuntimePhase,
        detail: impl Into<String>,
    ) -> Result<(), String> {
        let detail = detail.into();
        self.state.current_phase = phase;
        self.state.ready_for_transcripts = phase.ready_for_transcripts();
        self.state.transitions.push(LifecycleTransition {
            phase,
            entered_at_utc: runtime_timestamp_utc(),
            detail: detail.clone(),
        });

        let emit_seq = self.next_emit_seq();
        self.output.emit(RuntimeOutputEvent::Lifecycle {
            emit_seq,
            phase,
            detail,
        })?;

        let scheduled = self.scheduler.on_phase_change(phase);
        self.enqueue_jobs(scheduled)?;
        Ok(())
    }

    pub fn on_capture_chunk(&mut self, chunk: CaptureChunk) -> Result<(), String> {
        self.state.capture_chunks_seen += 1;
        let scheduled = self.scheduler.on_capture(
            SchedulingInput::from_capture_chunk(&chunk),
            self.state.current_phase,
        );
        self.enqueue_jobs(scheduled)
    }

    pub fn on_capture_event(&mut self, event: CaptureEvent) -> Result<(), String> {
        self.state.capture_events_seen += 1;
        let emit_seq = self.next_emit_seq();
        self.output.emit(RuntimeOutputEvent::CaptureEvent {
            emit_seq,
            code: capture_event_code(event.code).to_string(),
            detail: event.detail,
            count: event.count,
        })
    }

    pub fn pop_next_job(&mut self) -> Option<LiveAsrJobSpec> {
        self.pending_jobs.pop_front()
    }

    pub fn on_asr_result(&mut self, result: LiveAsrResult) -> Result<(), String> {
        self.state.asr_results_emitted += 1;
        let emit_seq = self.next_emit_seq();
        self.output
            .emit(RuntimeOutputEvent::AsrCompleted { emit_seq, result })
    }

    pub fn summary_snapshot(&self) -> LiveRuntimeSummary {
        LiveRuntimeSummary {
            final_phase: self.state.current_phase,
            ready_for_transcripts: self.state.ready_for_transcripts,
            transition_count: self.state.transitions.len(),
            capture_chunks_seen: self.state.capture_chunks_seen,
            capture_events_seen: self.state.capture_events_seen,
            asr_jobs_queued: self.state.asr_jobs_queued,
            asr_results_emitted: self.state.asr_results_emitted,
        }
    }

    pub fn finalize(mut self) -> Result<(S, O, F, LiveRuntimeSummary), String> {
        if self.state.current_phase != LiveRuntimePhase::Shutdown {
            self.transition_to(
                LiveRuntimePhase::Shutdown,
                "runtime finalized; coordinator is shutting down",
            )?;
        }
        let summary = self.summary_snapshot();
        self.finalizer.finalize(&summary)?;
        Ok((self.scheduler, self.output, self.finalizer, summary))
    }

    fn enqueue_jobs(&mut self, jobs: Vec<LiveAsrJobDraft>) -> Result<(), String> {
        for job in jobs {
            let emit_seq = self.next_emit_seq();
            let spec = LiveAsrJobSpec {
                emit_seq,
                class: job.class,
                channel: job.channel,
                segment_id: job.segment_id,
                start_ms: job.start_ms,
                end_ms: job.end_ms,
            };
            self.pending_jobs.push_back(spec.clone());
            self.state.asr_jobs_queued += 1;
            self.output.emit(RuntimeOutputEvent::AsrQueued {
                emit_seq,
                job: spec,
            })?;
        }
        Ok(())
    }

    fn next_emit_seq(&mut self) -> u64 {
        let seq = self.state.next_emit_seq;
        self.state.next_emit_seq += 1;
        seq
    }
}

fn capture_event_code(code: CaptureEventCode) -> &'static str {
    code.as_str()
}

fn seconds_to_millis(seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * 1_000.0).round() as u64
}

fn runtime_timestamp_utc() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_api::{
        CaptureEvent, CaptureEventCode, CaptureRecoveryAction, CaptureStream,
    };

    #[derive(Default)]
    struct TestScheduler {
        queued_on_capture: usize,
    }

    impl CaptureScheduler for TestScheduler {
        fn on_capture(
            &mut self,
            input: SchedulingInput,
            phase: LiveRuntimePhase,
        ) -> Vec<LiveAsrJobDraft> {
            if phase != LiveRuntimePhase::Active {
                return Vec::new();
            }
            self.queued_on_capture += 1;
            vec![LiveAsrJobDraft {
                class: LiveAsrJobClass::Partial,
                channel: input.channel,
                segment_id: format!("seg-{}", self.queued_on_capture),
                start_ms: input.pts_ms,
                end_ms: input.pts_ms + 500,
            }]
        }
    }

    #[derive(Default)]
    struct TestOutput {
        events: Vec<RuntimeOutputEvent>,
    }

    impl RuntimeOutputSink for TestOutput {
        fn emit(&mut self, event: RuntimeOutputEvent) -> Result<(), String> {
            self.events.push(event);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestFinalizer {
        summary: Option<LiveRuntimeSummary>,
    }

    impl RuntimeFinalizer for TestFinalizer {
        fn finalize(&mut self, summary: &LiveRuntimeSummary) -> Result<(), String> {
            self.summary = Some(summary.clone());
            Ok(())
        }
    }

    fn sample_chunk() -> CaptureChunk {
        CaptureChunk {
            stream: CaptureStream::Microphone,
            pts_seconds: 1.25,
            sample_rate_hz: 48_000,
            mono_samples: vec![0.1; 240],
        }
    }

    fn sample_event() -> CaptureEvent {
        CaptureEvent {
            generated_unix: 0,
            code: CaptureEventCode::QueueFullDrops,
            count: 2,
            recovery_action: CaptureRecoveryAction::DropSampleContinue,
            detail: "queue full".to_string(),
        }
    }

    #[test]
    fn lifecycle_phase_controls_ready_for_transcripts() {
        let mut coordinator = LiveStreamCoordinator::new(
            TestScheduler::default(),
            TestOutput::default(),
            TestFinalizer::default(),
        );

        assert_eq!(coordinator.state().current_phase, LiveRuntimePhase::Warmup);
        assert!(!coordinator.state().ready_for_transcripts);

        coordinator
            .transition_to(LiveRuntimePhase::Active, "ready")
            .expect("transition should succeed");
        assert!(coordinator.state().ready_for_transcripts);

        coordinator
            .transition_to(LiveRuntimePhase::Shutdown, "done")
            .expect("transition should succeed");
        assert!(coordinator.state().ready_for_transcripts);
    }

    #[test]
    fn scheduler_only_queues_jobs_during_active_phase() {
        let mut coordinator = LiveStreamCoordinator::new(
            TestScheduler::default(),
            TestOutput::default(),
            TestFinalizer::default(),
        );

        coordinator
            .on_capture_chunk(sample_chunk())
            .expect("warmup chunk should not fail");
        assert_eq!(coordinator.state().asr_jobs_queued, 0);

        coordinator
            .transition_to(LiveRuntimePhase::Active, "start stream")
            .expect("transition should succeed");
        coordinator
            .on_capture_chunk(sample_chunk())
            .expect("active chunk should be scheduled");
        assert_eq!(coordinator.state().asr_jobs_queued, 1);
        assert!(coordinator.pop_next_job().is_some());
    }

    #[test]
    fn emit_sequence_is_monotonic_for_all_output_events() {
        let mut coordinator = LiveStreamCoordinator::new(
            TestScheduler::default(),
            TestOutput::default(),
            TestFinalizer::default(),
        );
        coordinator
            .transition_to(LiveRuntimePhase::Active, "active")
            .expect("transition should succeed");
        coordinator
            .on_capture_chunk(sample_chunk())
            .expect("capture chunk should schedule");
        coordinator
            .on_capture_event(sample_event())
            .expect("capture event should emit");
        let queued_job = coordinator.pop_next_job().expect("one queued job expected");
        coordinator
            .on_asr_result(LiveAsrResult {
                job: queued_job,
                transcript_text: "hello".to_string(),
            })
            .expect("asr result should emit");

        let (_, output, _, _) = coordinator.finalize().expect("finalize should succeed");
        assert!(!output.events.is_empty());
        let mut previous = 0u64;
        for event in output.events {
            let current = event.emit_seq();
            assert!(current > previous, "emit_seq must be strictly increasing");
            previous = current;
        }
    }

    #[test]
    fn finalize_calls_finalizer_with_shutdown_summary() {
        let mut coordinator = LiveStreamCoordinator::new(
            TestScheduler::default(),
            TestOutput::default(),
            TestFinalizer::default(),
        );
        coordinator
            .transition_to(LiveRuntimePhase::Active, "active")
            .expect("transition should succeed");
        coordinator
            .on_capture_chunk(sample_chunk())
            .expect("capture chunk should succeed");
        coordinator
            .on_capture_event(sample_event())
            .expect("capture event should succeed");

        let (_, _, finalizer, summary) = coordinator.finalize().expect("finalize should succeed");
        let stored = finalizer.summary.expect("finalizer should receive summary");
        assert_eq!(summary, stored);
        assert_eq!(summary.final_phase, LiveRuntimePhase::Shutdown);
        assert_eq!(summary.capture_chunks_seen, 1);
        assert_eq!(summary.capture_events_seen, 1);
        assert_eq!(summary.asr_jobs_queued, 1);
    }
}
