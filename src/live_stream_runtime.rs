use crate::capture_api::{CaptureChunk, CaptureEvent, CaptureEventCode, CaptureStream};
use std::collections::{BTreeMap, VecDeque};
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
    pub duration_ms: u64,
    pub activity_level_per_mille: u16,
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
            duration_ms: frames_to_millis(chunk.mono_samples.len(), chunk.sample_rate_hz),
            activity_level_per_mille: average_abs_level_per_mille(&chunk.mono_samples),
        }
    }

    pub fn end_ms(&self) -> u64 {
        self.pts_ms.saturating_add(self.duration_ms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingVadConfig {
    pub rolling_window_ms: u64,
    pub min_speech_ms: u64,
    pub min_silence_ms: u64,
    pub open_threshold_per_mille: u16,
    pub close_threshold_per_mille: u16,
}

impl Default for StreamingVadConfig {
    fn default() -> Self {
        Self {
            rolling_window_ms: 3_000,
            min_speech_ms: 220,
            min_silence_ms: 250,
            open_threshold_per_mille: 40,
            close_threshold_per_mille: 20,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadBoundaryKind {
    Open,
    Close,
    Flush,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VadBoundary {
    pub channel: String,
    pub segment_id: String,
    pub segment_ord: u64,
    pub kind: VadBoundaryKind,
    pub start_ms: u64,
    pub end_ms: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelVadSnapshot {
    pub channel: String,
    pub rolling_chunk_count: usize,
    pub rolling_frame_count: usize,
    pub rolling_duration_ms: u64,
    pub open_segment_id: Option<String>,
    pub open_segment_ord: Option<u64>,
    pub open_segment_start_ms: Option<u64>,
    pub open_segment_end_ms: Option<u64>,
    pub speech_run_ms: u64,
    pub silence_run_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RollingChunk {
    start_ms: u64,
    end_ms: u64,
    frame_count: usize,
}

impl RollingChunk {
    fn duration_ms(self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSegmentState {
    segment_id: String,
    segment_ord: u64,
    start_ms: u64,
    end_ms: u64,
}

#[derive(Debug, Clone)]
struct ChannelVadState {
    rolling_chunks: VecDeque<RollingChunk>,
    rolling_frame_count: usize,
    rolling_duration_ms: u64,
    speech_run_start_ms: Option<u64>,
    speech_run_ms: u64,
    silence_run_ms: u64,
    was_speech_active: bool,
    open_segment: Option<OpenSegmentState>,
    next_segment_ord: u64,
}

impl Default for ChannelVadState {
    fn default() -> Self {
        Self {
            rolling_chunks: VecDeque::new(),
            rolling_frame_count: 0,
            rolling_duration_ms: 0,
            speech_run_start_ms: None,
            speech_run_ms: 0,
            silence_run_ms: 0,
            was_speech_active: false,
            open_segment: None,
            next_segment_ord: 1,
        }
    }
}

impl ChannelVadState {
    fn push_rolling_chunk(&mut self, chunk: RollingChunk, max_window_ms: u64) {
        self.rolling_frame_count = self.rolling_frame_count.saturating_add(chunk.frame_count);
        self.rolling_duration_ms = self.rolling_duration_ms.saturating_add(chunk.duration_ms());
        self.rolling_chunks.push_back(chunk);

        while self.rolling_duration_ms > max_window_ms && self.rolling_chunks.len() > 1 {
            if let Some(evicted) = self.rolling_chunks.pop_front() {
                self.rolling_duration_ms = self
                    .rolling_duration_ms
                    .saturating_sub(evicted.duration_ms());
                self.rolling_frame_count =
                    self.rolling_frame_count.saturating_sub(evicted.frame_count);
            }
        }
    }

    fn snapshot(&self, channel: &str) -> ChannelVadSnapshot {
        ChannelVadSnapshot {
            channel: channel.to_string(),
            rolling_chunk_count: self.rolling_chunks.len(),
            rolling_frame_count: self.rolling_frame_count,
            rolling_duration_ms: self.rolling_duration_ms,
            open_segment_id: self
                .open_segment
                .as_ref()
                .map(|segment| segment.segment_id.clone()),
            open_segment_ord: self
                .open_segment
                .as_ref()
                .map(|segment| segment.segment_ord),
            open_segment_start_ms: self.open_segment.as_ref().map(|segment| segment.start_ms),
            open_segment_end_ms: self.open_segment.as_ref().map(|segment| segment.end_ms),
            speech_run_ms: self.speech_run_ms,
            silence_run_ms: self.silence_run_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamingVadTracker {
    config: StreamingVadConfig,
    channels: BTreeMap<String, ChannelVadState>,
    pending_boundaries: VecDeque<VadBoundary>,
}

impl Default for StreamingVadTracker {
    fn default() -> Self {
        Self::new(StreamingVadConfig::default())
    }
}

impl StreamingVadTracker {
    pub fn new(config: StreamingVadConfig) -> Self {
        Self {
            config,
            channels: BTreeMap::new(),
            pending_boundaries: VecDeque::new(),
        }
    }

    pub fn ingest(&mut self, input: SchedulingInput) {
        let channel = input.channel.clone();
        let chunk_start = input.pts_ms;
        let chunk_end = input.end_ms();
        let channel_state = self.channels.entry(channel.clone()).or_default();
        channel_state.push_rolling_chunk(
            RollingChunk {
                start_ms: chunk_start,
                end_ms: chunk_end,
                frame_count: input.frame_count,
            },
            self.config.rolling_window_ms,
        );

        let speech_active = classify_speech_activity(
            input.activity_level_per_mille,
            self.config.open_threshold_per_mille,
            self.config.close_threshold_per_mille,
            channel_state.was_speech_active,
        );
        channel_state.was_speech_active = speech_active;

        if speech_active {
            if channel_state.speech_run_start_ms.is_none() {
                channel_state.speech_run_start_ms = Some(chunk_start);
            }
            channel_state.speech_run_ms = channel_state
                .speech_run_ms
                .saturating_add(input.duration_ms);
            channel_state.silence_run_ms = 0;

            if let Some(open_segment) = channel_state.open_segment.as_mut() {
                open_segment.end_ms = chunk_end;
                return;
            }

            if channel_state.speech_run_ms >= self.config.min_speech_ms {
                let segment_ord = channel_state.next_segment_ord;
                channel_state.next_segment_ord = channel_state.next_segment_ord.saturating_add(1);
                let start_ms = channel_state.speech_run_start_ms.unwrap_or(chunk_start);
                let segment_id = format!("{channel}-seg-{segment_ord:04}");

                channel_state.open_segment = Some(OpenSegmentState {
                    segment_id: segment_id.clone(),
                    segment_ord,
                    start_ms,
                    end_ms: chunk_end,
                });
                self.pending_boundaries.push_back(VadBoundary {
                    channel,
                    segment_id,
                    segment_ord,
                    kind: VadBoundaryKind::Open,
                    start_ms,
                    end_ms: start_ms,
                    reason: "vad-open-threshold".to_string(),
                });
            }
            return;
        }

        channel_state.speech_run_ms = 0;
        channel_state.speech_run_start_ms = None;
        channel_state.silence_run_ms = channel_state
            .silence_run_ms
            .saturating_add(input.duration_ms);

        let should_close_segment = channel_state.open_segment.is_some()
            && channel_state.silence_run_ms >= self.config.min_silence_ms;
        if !should_close_segment {
            return;
        }

        if let Some(open_segment) = channel_state.open_segment.take() {
            channel_state.silence_run_ms = 0;
            self.pending_boundaries.push_back(VadBoundary {
                channel,
                segment_id: open_segment.segment_id,
                segment_ord: open_segment.segment_ord,
                kind: VadBoundaryKind::Close,
                start_ms: open_segment.start_ms,
                end_ms: open_segment.end_ms,
                reason: "vad-close-silence".to_string(),
            });
        }
    }

    pub fn flush_open_segments(&mut self, reason: &str) {
        for (channel, channel_state) in &mut self.channels {
            if let Some(open_segment) = channel_state.open_segment.take() {
                self.pending_boundaries.push_back(VadBoundary {
                    channel: channel.clone(),
                    segment_id: open_segment.segment_id,
                    segment_ord: open_segment.segment_ord,
                    kind: VadBoundaryKind::Flush,
                    start_ms: open_segment.start_ms,
                    end_ms: open_segment.end_ms,
                    reason: reason.to_string(),
                });
            }
            channel_state.speech_run_ms = 0;
            channel_state.speech_run_start_ms = None;
            channel_state.silence_run_ms = 0;
            channel_state.was_speech_active = false;
        }
    }

    pub fn drain_boundaries(&mut self) -> Vec<VadBoundary> {
        self.pending_boundaries.drain(..).collect()
    }

    pub fn channel_snapshot(&self, channel: &str) -> Option<ChannelVadSnapshot> {
        self.channels
            .get(channel)
            .map(|state| state.snapshot(channel))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingSchedulerConfig {
    pub partial_window_ms: u64,
    pub partial_stride_ms: u64,
    pub min_partial_span_ms: u64,
}

impl Default for StreamingSchedulerConfig {
    fn default() -> Self {
        Self {
            partial_window_ms: 2_000,
            partial_stride_ms: 500,
            min_partial_span_ms: 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SegmentKey {
    channel: String,
    segment_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PartialCursor {
    last_partial_emit_end_ms: u64,
}

#[derive(Debug, Clone)]
pub struct StreamingVadScheduler {
    tracker: StreamingVadTracker,
    scheduler_config: StreamingSchedulerConfig,
    partial_cursors: BTreeMap<SegmentKey, PartialCursor>,
    pending_reconcile_jobs: VecDeque<LiveAsrJobDraft>,
}

impl Default for StreamingVadScheduler {
    fn default() -> Self {
        Self::new(StreamingVadConfig::default())
    }
}

impl StreamingVadScheduler {
    pub fn new(config: StreamingVadConfig) -> Self {
        Self::with_configs(config, StreamingSchedulerConfig::default())
    }

    pub fn with_configs(
        vad_config: StreamingVadConfig,
        scheduler_config: StreamingSchedulerConfig,
    ) -> Self {
        Self {
            tracker: StreamingVadTracker::new(vad_config),
            scheduler_config,
            partial_cursors: BTreeMap::new(),
            pending_reconcile_jobs: VecDeque::new(),
        }
    }

    pub fn drain_boundaries(&mut self) -> Vec<VadBoundary> {
        self.tracker.drain_boundaries()
    }

    pub fn channel_snapshot(&self, channel: &str) -> Option<ChannelVadSnapshot> {
        self.tracker.channel_snapshot(channel)
    }

    pub fn queue_reconcile_job(&mut self, job: LiveAsrJobDraft) {
        self.pending_reconcile_jobs.push_back(job);
    }

    fn handle_boundary(
        &mut self,
        boundary: VadBoundary,
        allow_job_emit: bool,
        out_jobs: &mut Vec<LiveAsrJobDraft>,
    ) {
        let key = SegmentKey {
            channel: boundary.channel.clone(),
            segment_id: boundary.segment_id.clone(),
        };
        match boundary.kind {
            VadBoundaryKind::Open => {
                self.partial_cursors.entry(key).or_insert(PartialCursor {
                    last_partial_emit_end_ms: boundary.start_ms,
                });
            }
            VadBoundaryKind::Close | VadBoundaryKind::Flush => {
                self.partial_cursors.remove(&key);
                if allow_job_emit {
                    out_jobs.push(LiveAsrJobDraft {
                        class: LiveAsrJobClass::Final,
                        channel: boundary.channel,
                        segment_id: boundary.segment_id,
                        start_ms: boundary.start_ms,
                        end_ms: boundary.end_ms.max(boundary.start_ms),
                    });
                }
            }
        }
    }

    fn maybe_emit_partial(
        &mut self,
        snapshot: &ChannelVadSnapshot,
        allow_job_emit: bool,
        out_jobs: &mut Vec<LiveAsrJobDraft>,
    ) {
        if !allow_job_emit {
            return;
        }

        let (segment_id, segment_start_ms, segment_end_ms) = match (
            snapshot.open_segment_id.as_ref(),
            snapshot.open_segment_start_ms,
            snapshot.open_segment_end_ms,
        ) {
            (Some(segment_id), Some(start_ms), Some(end_ms)) => {
                (segment_id.clone(), start_ms, end_ms)
            }
            _ => return,
        };

        if segment_end_ms <= segment_start_ms {
            return;
        }

        let key = SegmentKey {
            channel: snapshot.channel.clone(),
            segment_id: segment_id.clone(),
        };
        let cursor = self.partial_cursors.entry(key).or_insert(PartialCursor {
            last_partial_emit_end_ms: segment_start_ms,
        });
        let stride_ready = segment_end_ms
            >= cursor
                .last_partial_emit_end_ms
                .saturating_add(self.scheduler_config.partial_stride_ms);
        if !stride_ready {
            return;
        }

        let window_start = segment_end_ms
            .saturating_sub(self.scheduler_config.partial_window_ms)
            .max(segment_start_ms);
        let span_ms = segment_end_ms.saturating_sub(window_start);
        if span_ms < self.scheduler_config.min_partial_span_ms {
            return;
        }

        out_jobs.push(LiveAsrJobDraft {
            class: LiveAsrJobClass::Partial,
            channel: snapshot.channel.clone(),
            segment_id,
            start_ms: window_start,
            end_ms: segment_end_ms,
        });
        cursor.last_partial_emit_end_ms = segment_end_ms;
    }

    fn drain_pending_reconcile_jobs(&mut self, out_jobs: &mut Vec<LiveAsrJobDraft>) {
        out_jobs.extend(self.pending_reconcile_jobs.drain(..));
    }
}

impl CaptureScheduler for StreamingVadScheduler {
    fn on_capture(
        &mut self,
        input: SchedulingInput,
        phase: LiveRuntimePhase,
    ) -> Vec<LiveAsrJobDraft> {
        if phase == LiveRuntimePhase::Shutdown {
            return Vec::new();
        }

        let channel = input.channel.clone();
        self.tracker.ingest(input);
        let mut jobs = Vec::new();
        let allow_job_emit = phase != LiveRuntimePhase::Warmup;
        for boundary in self.tracker.drain_boundaries() {
            self.handle_boundary(boundary, allow_job_emit, &mut jobs);
        }
        if let Some(snapshot) = self.tracker.channel_snapshot(&channel) {
            self.maybe_emit_partial(&snapshot, phase == LiveRuntimePhase::Active, &mut jobs);
        }
        self.drain_pending_reconcile_jobs(&mut jobs);
        jobs
    }

    fn on_phase_change(&mut self, phase: LiveRuntimePhase) -> Vec<LiveAsrJobDraft> {
        if matches!(
            phase,
            LiveRuntimePhase::Draining | LiveRuntimePhase::Shutdown
        ) {
            self.tracker.flush_open_segments("phase-flush");
        }
        let mut jobs = Vec::new();
        for boundary in self.tracker.drain_boundaries() {
            self.handle_boundary(boundary, true, &mut jobs);
        }
        self.drain_pending_reconcile_jobs(&mut jobs);
        jobs
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

fn frames_to_millis(frame_count: usize, sample_rate_hz: u32) -> u64 {
    if frame_count == 0 || sample_rate_hz == 0 {
        return 0;
    }
    let rounded = (frame_count as u128 * 1_000) + (sample_rate_hz as u128 / 2);
    let ms = (rounded / sample_rate_hz as u128) as u64;
    if ms == 0 {
        1
    } else {
        ms
    }
}

fn average_abs_level_per_mille(samples: &[f32]) -> u16 {
    if samples.is_empty() {
        return 0;
    }
    let mut sum = 0.0f32;
    for sample in samples {
        sum += sample.abs();
    }
    let avg = (sum / samples.len() as f32).clamp(0.0, 1.0);
    (avg * 1_000.0).round() as u16
}

fn classify_speech_activity(
    activity_level_per_mille: u16,
    open_threshold_per_mille: u16,
    close_threshold_per_mille: u16,
    was_speech_active: bool,
) -> bool {
    if activity_level_per_mille >= open_threshold_per_mille {
        return true;
    }
    if activity_level_per_mille <= close_threshold_per_mille {
        return false;
    }
    was_speech_active
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
            rolling_window_ms: 60,
            min_speech_ms: 40,
            min_silence_ms: 30,
            open_threshold_per_mille: 60,
            close_threshold_per_mille: 20,
        }
    }

    fn test_scheduler_config() -> StreamingSchedulerConfig {
        StreamingSchedulerConfig {
            partial_window_ms: 40,
            partial_stride_ms: 25,
            min_partial_span_ms: 20,
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

    #[test]
    fn scheduling_input_derives_duration_and_activity_level() {
        let chunk = CaptureChunk {
            stream: CaptureStream::Microphone,
            pts_seconds: 0.5,
            sample_rate_hz: 48_000,
            mono_samples: vec![0.1, -0.2, 0.3, -0.4],
        };
        let input = SchedulingInput::from_capture_chunk(&chunk);
        assert_eq!(input.channel, "microphone");
        assert_eq!(input.pts_ms, 500);
        assert_eq!(input.frame_count, 4);
        assert_eq!(input.duration_ms, 1);
        assert_eq!(input.activity_level_per_mille, 250);
        assert_eq!(input.end_ms(), 501);
    }

    #[test]
    fn streaming_vad_tracker_trims_rolling_window_deterministically() {
        let mut tracker = StreamingVadTracker::new(test_vad_config());
        for chunk_idx in 0..4 {
            tracker.ingest(manual_input("microphone", chunk_idx * 20, 20, 960, 100));
        }

        let snapshot = tracker
            .channel_snapshot("microphone")
            .expect("snapshot should exist");
        assert_eq!(snapshot.rolling_chunk_count, 3);
        assert_eq!(snapshot.rolling_duration_ms, 60);
        assert_eq!(snapshot.rolling_frame_count, 960 * 3);
    }

    #[test]
    fn streaming_vad_tracker_emits_open_and_close_boundaries() {
        let mut tracker = StreamingVadTracker::new(test_vad_config());
        tracker.ingest(manual_input("microphone", 0, 20, 960, 110));
        tracker.ingest(manual_input("microphone", 20, 20, 960, 110));
        tracker.ingest(manual_input("microphone", 40, 20, 960, 110));
        tracker.ingest(manual_input("microphone", 60, 20, 960, 0));
        tracker.ingest(manual_input("microphone", 80, 20, 960, 0));

        let boundaries = tracker.drain_boundaries();
        assert_eq!(boundaries.len(), 2);

        let open = &boundaries[0];
        assert_eq!(open.kind, VadBoundaryKind::Open);
        assert_eq!(open.channel, "microphone");
        assert_eq!(open.start_ms, 0);
        assert_eq!(open.end_ms, 0);

        let close = &boundaries[1];
        assert_eq!(close.kind, VadBoundaryKind::Close);
        assert_eq!(close.channel, "microphone");
        assert_eq!(close.start_ms, 0);
        assert_eq!(close.end_ms, 60);
        assert_eq!(close.segment_id, open.segment_id);
        assert_eq!(close.segment_ord, open.segment_ord);
    }

    #[test]
    fn streaming_vad_tracker_flushes_open_segments_in_channel_sort_order() {
        let mut tracker = StreamingVadTracker::new(StreamingVadConfig {
            min_speech_ms: 20,
            ..test_vad_config()
        });
        tracker.ingest(manual_input("system-audio", 0, 20, 960, 110));
        tracker.ingest(manual_input("microphone", 5, 20, 960, 110));
        let open_boundaries = tracker.drain_boundaries();
        assert_eq!(open_boundaries.len(), 2);

        tracker.flush_open_segments("shutdown-flush");
        let flushed = tracker.drain_boundaries();
        assert_eq!(flushed.len(), 2);
        assert_eq!(flushed[0].kind, VadBoundaryKind::Flush);
        assert_eq!(flushed[1].kind, VadBoundaryKind::Flush);
        assert_eq!(flushed[0].channel, "microphone");
        assert_eq!(flushed[1].channel, "system-audio");
        assert_eq!(flushed[0].reason, "shutdown-flush");
        assert_eq!(flushed[1].reason, "shutdown-flush");
    }

    #[test]
    fn streaming_vad_scheduler_flushes_on_draining_phase_transition() {
        let mut scheduler = StreamingVadScheduler::with_configs(
            StreamingVadConfig {
                min_speech_ms: 20,
                ..test_vad_config()
            },
            test_scheduler_config(),
        );

        let open_jobs = scheduler.on_capture(
            manual_input("microphone", 0, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        assert!(open_jobs.is_empty());
        assert!(scheduler.drain_boundaries().is_empty());

        let draining_jobs = scheduler.on_phase_change(LiveRuntimePhase::Draining);
        assert_eq!(draining_jobs.len(), 1);
        assert_eq!(draining_jobs[0].class, LiveAsrJobClass::Final);
        assert_eq!(draining_jobs[0].channel, "microphone");
        assert_eq!(draining_jobs[0].segment_id, "microphone-seg-0001");
    }

    #[test]
    fn streaming_scheduler_emits_partial_jobs_on_stride_cadence() {
        let mut scheduler = StreamingVadScheduler::with_configs(
            StreamingVadConfig {
                min_speech_ms: 20,
                ..test_vad_config()
            },
            test_scheduler_config(),
        );

        let jobs_1 = scheduler.on_capture(
            manual_input("microphone", 0, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        assert!(jobs_1.is_empty());

        let jobs_2 = scheduler.on_capture(
            manual_input("microphone", 20, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        assert_eq!(jobs_2.len(), 1);
        assert_eq!(jobs_2[0].class, LiveAsrJobClass::Partial);
        assert_eq!(jobs_2[0].start_ms, 0);
        assert_eq!(jobs_2[0].end_ms, 40);

        let jobs_3 = scheduler.on_capture(
            manual_input("microphone", 40, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        assert!(jobs_3.is_empty());

        let jobs_4 = scheduler.on_capture(
            manual_input("microphone", 60, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        assert_eq!(jobs_4.len(), 1);
        assert_eq!(jobs_4[0].class, LiveAsrJobClass::Partial);
        assert_eq!(jobs_4[0].start_ms, 40);
        assert_eq!(jobs_4[0].end_ms, 80);
    }

    #[test]
    fn streaming_scheduler_emits_final_on_close_boundary() {
        let mut scheduler = StreamingVadScheduler::with_configs(
            test_vad_config(),
            StreamingSchedulerConfig {
                partial_window_ms: 40,
                partial_stride_ms: 1000,
                min_partial_span_ms: 20,
            },
        );
        scheduler.on_capture(
            manual_input("microphone", 0, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        scheduler.on_capture(
            manual_input("microphone", 20, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        scheduler.on_capture(
            manual_input("microphone", 40, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        let silence_1 = scheduler.on_capture(
            manual_input("microphone", 60, 20, 960, 0),
            LiveRuntimePhase::Active,
        );
        assert!(silence_1.is_empty());

        let silence_2 = scheduler.on_capture(
            manual_input("microphone", 80, 20, 960, 0),
            LiveRuntimePhase::Active,
        );
        assert_eq!(silence_2.len(), 1);
        assert_eq!(silence_2[0].class, LiveAsrJobClass::Final);
        assert_eq!(silence_2[0].channel, "microphone");
        assert_eq!(silence_2[0].segment_id, "microphone-seg-0001");
        assert_eq!(silence_2[0].start_ms, 0);
        assert_eq!(silence_2[0].end_ms, 60);
    }

    #[test]
    fn streaming_scheduler_reconcile_jobs_require_explicit_trigger() {
        let mut scheduler = StreamingVadScheduler::new(test_vad_config());
        let none = scheduler.on_phase_change(LiveRuntimePhase::Active);
        assert!(none.is_empty());

        scheduler.queue_reconcile_job(LiveAsrJobDraft {
            class: LiveAsrJobClass::Reconcile,
            channel: "microphone".to_string(),
            segment_id: "microphone-seg-0001".to_string(),
            start_ms: 0,
            end_ms: 120,
        });

        let reconciles = scheduler.on_phase_change(LiveRuntimePhase::Active);
        assert_eq!(reconciles.len(), 1);
        assert_eq!(reconciles[0].class, LiveAsrJobClass::Reconcile);
        assert_eq!(reconciles[0].channel, "microphone");
        assert_eq!(reconciles[0].segment_id, "microphone-seg-0001");
        assert!(scheduler
            .on_phase_change(LiveRuntimePhase::Active)
            .is_empty());
    }

    #[test]
    fn streaming_scheduler_emits_final_before_reconcile_when_draining() {
        let mut scheduler = StreamingVadScheduler::with_configs(
            StreamingVadConfig {
                min_speech_ms: 20,
                ..test_vad_config()
            },
            test_scheduler_config(),
        );
        scheduler.on_capture(
            manual_input("microphone", 0, 20, 960, 110),
            LiveRuntimePhase::Active,
        );
        scheduler.queue_reconcile_job(LiveAsrJobDraft {
            class: LiveAsrJobClass::Reconcile,
            channel: "microphone".to_string(),
            segment_id: "microphone-seg-0001".to_string(),
            start_ms: 0,
            end_ms: 20,
        });

        let jobs = scheduler.on_phase_change(LiveRuntimePhase::Draining);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].class, LiveAsrJobClass::Final);
        assert_eq!(jobs[1].class, LiveAsrJobClass::Reconcile);
    }
}
