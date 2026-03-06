use super::*;
use recordit::live_asr_pool::LiveAsrAudioInput;

struct LiveStreamRuntimeOutcome {
    output_events: Vec<RuntimeOutputEvent>,
    runtime_summary: LiveRuntimeSummary,
    asr_worker_pool: LiveAsrPoolTelemetry,
}

struct LiveStreamRuntimeExecution {
    channel_mode: ChannelMode,
    speaker_labels: SpeakerLabels,
    terminal_stream: LiveTerminalStream,
    coordinator: LiveStreamCoordinator<
        StreamingVadScheduler,
        CollectingRuntimeOutputSink,
        CollectingRuntimeFinalizer,
    >,
    asr_service: LiveAsrService,
    submit_window: usize,
    pending_specs: VecDeque<RuntimeAsrJobSpec>,
    in_flight: usize,
    next_job_id: usize,
    submitted_specs: HashMap<usize, RuntimeAsrJobSpec>,
    audio_by_channel: HashMap<String, RuntimeChannelAudio>,
    pump_cadence: PumpCadenceController,
}

fn graceful_stop_request_path(config: &TranscribeConfig) -> Option<PathBuf> {
    config
        .out_manifest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join("session.stop.request"))
}

#[derive(Debug, Default)]
struct RuntimeChannelAudio {
    sample_rate_hz: u32,
    base_pts_ms: Option<u64>,
    samples: Vec<f32>,
}

impl RuntimeChannelAudio {
    fn ingest_chunk(&mut self, chunk: &CaptureChunk) {
        let start_ms = if chunk.pts_seconds.is_finite() && chunk.pts_seconds > 0.0 {
            (chunk.pts_seconds * 1_000.0).round() as u64
        } else {
            0
        };
        if self.base_pts_ms.is_none() {
            self.base_pts_ms = Some(start_ms);
            self.sample_rate_hz = chunk.sample_rate_hz;
        }
        if self.sample_rate_hz == 0 {
            self.sample_rate_hz = chunk.sample_rate_hz;
        }

        let base_pts_ms = self.base_pts_ms.unwrap_or(start_ms);
        let start_offset_ms = start_ms.saturating_sub(base_pts_ms);
        let start_index = ms_to_sample_index(start_offset_ms, self.sample_rate_hz);
        let end_index = start_index.saturating_add(chunk.mono_samples.len());
        if self.samples.len() < end_index {
            self.samples.resize(end_index, 0.0);
        }
        self.samples[start_index..end_index].copy_from_slice(&chunk.mono_samples);
    }

    fn render_window(&self, start_ms: u64, end_ms: u64) -> Vec<f32> {
        let Some(base_pts_ms) = self.base_pts_ms else {
            return Vec::new();
        };
        if self.sample_rate_hz == 0 {
            return Vec::new();
        }
        let start_offset_ms = start_ms.saturating_sub(base_pts_ms);
        let end_offset_ms = end_ms.saturating_sub(base_pts_ms);
        let start_index = ms_to_sample_index(start_offset_ms, self.sample_rate_hz);
        let end_index = ms_to_sample_index(end_offset_ms, self.sample_rate_hz)
            .max(start_index.saturating_add(1))
            .min(self.samples.len());
        if start_index >= end_index || start_index >= self.samples.len() {
            return Vec::new();
        }
        self.samples[start_index..end_index].to_vec()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PumpDecisionReason {
    CadenceDue,
    CadenceDeferred,
    ForcedCaptureEvent,
    ForcedPhaseTransition,
    ForcedCaptureEnd,
    ForcedFlush,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PumpDecision {
    Run(PumpDecisionReason),
    Skip(PumpDecisionReason),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PumpCadenceTelemetry {
    cadence_ms: u64,
    decisions: u64,
    full_pumps: u64,
    deferred_pumps: u64,
    forced_pumps: u64,
    last_reason: Option<PumpDecisionReason>,
}

#[derive(Debug, Clone, Copy)]
struct PumpCadenceController {
    cadence_ms: u64,
    next_due_pts_ms: Option<u64>,
    fallback_pts_ms: u64,
    telemetry: PumpCadenceTelemetry,
}

impl PumpCadenceController {
    fn new(cadence_ms: u64) -> Self {
        let cadence_ms = cadence_ms.max(1);
        Self {
            cadence_ms,
            next_due_pts_ms: None,
            fallback_pts_ms: 0,
            telemetry: PumpCadenceTelemetry {
                cadence_ms,
                ..PumpCadenceTelemetry::default()
            },
        }
    }

    #[cfg(test)]
    fn telemetry(&self) -> PumpCadenceTelemetry {
        self.telemetry
    }

    fn on_chunk(&mut self, pts_ms: Option<u64>, phase: LiveRuntimePhase) -> PumpDecision {
        self.telemetry.decisions += 1;
        if phase != LiveRuntimePhase::Active {
            self.next_due_pts_ms = None;
            self.telemetry.full_pumps += 1;
            self.telemetry.forced_pumps += 1;
            self.telemetry.last_reason = Some(PumpDecisionReason::ForcedPhaseTransition);
            return PumpDecision::Run(PumpDecisionReason::ForcedPhaseTransition);
        }

        let observed_pts_ms = self.normalize_pts_ms(pts_ms);
        let should_run = self
            .next_due_pts_ms
            .map(|next_due| observed_pts_ms >= next_due)
            .unwrap_or(true);
        if should_run {
            self.next_due_pts_ms = Some(observed_pts_ms.saturating_add(self.cadence_ms));
            self.telemetry.full_pumps += 1;
            self.telemetry.last_reason = Some(PumpDecisionReason::CadenceDue);
            PumpDecision::Run(PumpDecisionReason::CadenceDue)
        } else {
            self.telemetry.deferred_pumps += 1;
            self.telemetry.last_reason = Some(PumpDecisionReason::CadenceDeferred);
            PumpDecision::Skip(PumpDecisionReason::CadenceDeferred)
        }
    }

    fn force(&mut self, reason: PumpDecisionReason) -> PumpDecision {
        self.telemetry.decisions += 1;
        self.telemetry.full_pumps += 1;
        self.telemetry.forced_pumps += 1;
        self.telemetry.last_reason = Some(reason);
        if matches!(
            reason,
            PumpDecisionReason::ForcedPhaseTransition
                | PumpDecisionReason::ForcedCaptureEnd
                | PumpDecisionReason::ForcedFlush
        ) {
            self.next_due_pts_ms = None;
        }
        PumpDecision::Run(reason)
    }

    fn normalize_pts_ms(&mut self, pts_ms: Option<u64>) -> u64 {
        match pts_ms {
            Some(ms) => {
                self.fallback_pts_ms = self.fallback_pts_ms.max(ms);
                ms
            }
            _ => {
                self.fallback_pts_ms = self.fallback_pts_ms.saturating_add(self.cadence_ms);
                self.fallback_pts_ms
            }
        }
    }
}

#[derive(Debug)]
struct LiveTerminalStream {
    mode: TerminalRenderMode,
    partial_visible: bool,
    last_partial_by_segment: HashMap<(String, String), String>,
}

impl LiveTerminalStream {
    fn new(mode: TerminalRenderMode) -> Self {
        Self {
            mode,
            partial_visible: false,
            last_partial_by_segment: HashMap::new(),
        }
    }

    fn emit_event(&mut self, event: &TranscriptEvent) {
        match self.mode {
            TerminalRenderMode::DeterministicNonTty => {
                if !is_stable_terminal_event(event.event_type) {
                    return;
                }
                let Some(line) = format_stable_transcript_line(event) else {
                    return;
                };
                let _ = writeln!(std::io::stdout(), "{line}");
            }
            TerminalRenderMode::InteractiveTty => self.emit_interactive_event(event),
        }
    }

    fn finish(&mut self) {
        if self.mode == TerminalRenderMode::InteractiveTty && self.partial_visible {
            let _ = writeln!(std::io::stdout());
            self.partial_visible = false;
        }
        self.last_partial_by_segment.clear();
    }

    fn emit_interactive_event(&mut self, event: &TranscriptEvent) {
        let mut stdout = std::io::stdout();
        match event.event_type {
            "partial" => {
                let key = (event.channel.clone(), event.segment_id.clone());
                let Some(line) = format_partial_transcript_line(event) else {
                    return;
                };
                if self.last_partial_by_segment.get(&key) == Some(&line) {
                    return;
                }
                self.last_partial_by_segment.insert(key, line.clone());
                let _ = write!(stdout, "\r\x1b[2K{line}");
                let _ = stdout.flush();
                self.partial_visible = true;
            }
            "final" | "llm_final" | "reconciled_final" => {
                self.last_partial_by_segment
                    .remove(&(event.channel.clone(), event.segment_id.clone()));
                let Some(line) = format_stable_transcript_line(event) else {
                    return;
                };
                if self.partial_visible {
                    let _ = write!(stdout, "\r\x1b[2K");
                    self.partial_visible = false;
                }
                let _ = writeln!(stdout, "{line}");
            }
            _ => {}
        }
    }
}

pub(super) fn live_stream_vad_thresholds_per_mille(vad_threshold: f32) -> (u16, u16) {
    // StreamingVadScheduler activity is measured as average absolute PCM energy per-mille.
    // Keep CLI vad_threshold (0..1) aligned with scheduler defaults (40/20) at 0.50.
    let normalized = vad_threshold.clamp(0.0, 1.0);
    let open = ((normalized * 80.0).round() as u16).max(1);
    let close = open.saturating_div(2).max(1);
    (open, close)
}

fn live_stream_backpressure_config(
    config: &TranscribeConfig,
) -> recordit::live_stream_runtime::AdaptiveBackpressureConfig {
    let mut backpressure = recordit::live_stream_runtime::AdaptiveBackpressureConfig::default();
    if !config.adaptive_backpressure_enabled {
        // Kill-switch path: keep scheduler in normal mode by making pressure thresholds unreachable.
        backpressure.pressure_pending_jobs_threshold = u64::MAX;
        backpressure.severe_pending_jobs_threshold = u64::MAX;
        backpressure.severe_pending_final_jobs_threshold = u64::MAX;
        backpressure.pressure_min_observations = usize::MAX;
        backpressure.severe_min_observations = usize::MAX;
        backpressure.pressure_stride_multiplier = 1;
    }
    backpressure
}

fn build_live_asr_request_for_spec(
    channel_mode: ChannelMode,
    speaker_labels: &SpeakerLabels,
    channel_audio: &RuntimeChannelAudio,
    job_id: usize,
    spec: &RuntimeAsrJobSpec,
) -> LiveAsrRequest {
    let mut mono_samples = channel_audio.render_window(spec.start_ms, spec.end_ms);
    if mono_samples.is_empty() {
        // Keep zero-sample fallback semantics deterministic for empty windows.
        mono_samples.push(0.0);
    }
    LiveAsrRequest {
        job_id,
        class: runtime_job_class_to_pool_job_class(spec.job_class),
        role: runtime_channel_role(spec.channel.as_str()),
        label: runtime_channel_label(channel_mode, speaker_labels, spec.channel.as_str()),
        segment_id: spec.segment_id.clone(),
        audio_input: LiveAsrAudioInput::pcm_window(
            channel_audio.sample_rate_hz.max(1),
            spec.start_ms,
            spec.end_ms,
            mono_samples,
        ),
    }
}

impl LiveStreamRuntimeExecution {
    fn new(
        config: &TranscribeConfig,
        resolved_model_path: &Path,
        _stamp: &str,
    ) -> Result<Self, CliError> {
        let (open_threshold_per_mille, close_threshold_per_mille) =
            live_stream_vad_thresholds_per_mille(config.vad_threshold);
        let scheduler = StreamingVadScheduler::with_configs(
            StreamingVadConfig {
                rolling_window_ms: config
                    .chunk_window_ms
                    .max(config.chunk_stride_ms)
                    .max(config.vad_min_speech_ms as u64),
                min_speech_ms: config.vad_min_speech_ms as u64,
                min_silence_ms: config.vad_min_silence_ms as u64,
                open_threshold_per_mille,
                close_threshold_per_mille,
            },
            StreamingSchedulerConfig {
                partial_window_ms: config.chunk_window_ms,
                partial_stride_ms: config.chunk_stride_ms,
                min_partial_span_ms: config.chunk_stride_ms.min(config.chunk_window_ms).max(1),
                backpressure: live_stream_backpressure_config(config),
            },
        );
        let incremental_jsonl = LiveStreamIncrementalJsonlWriter::open(config).map_err(|err| {
            CliError::new(format!(
                "failed to initialize live-stream incremental JSONL writer: {err}"
            ))
        })?;
        let mut coordinator = LiveStreamCoordinator::new(
            scheduler,
            CollectingRuntimeOutputSink::with_incremental_jsonl(incremental_jsonl),
            CollectingRuntimeFinalizer::default(),
        );
        coordinator
            .transition_to(
                LiveRuntimePhase::Warmup,
                "preparing model, capture input, and channel routing",
            )
            .map_err(|err| CliError::new(format!("failed to initialize live runtime: {err}")))?;
        coordinator
            .transition_to(
                LiveRuntimePhase::Active,
                "capture/model warmup complete; transcript chunks may emit now",
            )
            .map_err(|err| CliError::new(format!("failed to activate live runtime: {err}")))?;

        let temp_audio_policy = if config.keep_temp_audio {
            TempAudioPolicy::RetainAlways
        } else {
            TempAudioPolicy::RetainOnFailure
        };
        let helper_program = resolve_backend_program(config.asr_backend, resolved_model_path);
        let executor = Arc::new(PooledAsrExecutor {
            backend: config.asr_backend,
            helper_program,
            model_path: resolved_model_path.to_path_buf(),
            language: config.asr_language.clone(),
            threads: config.asr_threads,
            temp_audio_policy,
            prewarm_enabled: true,
        });
        let asr_service = LiveAsrService::start(
            executor,
            LiveAsrPoolConfig {
                worker_count: config.live_asr_workers.max(1),
                queue_capacity: config.chunk_queue_cap.max(config.live_asr_workers).max(1),
                retries: 0,
                temp_audio_policy,
            },
        );

        let submit_window = asr_service.queue_capacity().max(1);
        Ok(Self {
            channel_mode: config.channel_mode,
            speaker_labels: config.speaker_labels.clone(),
            terminal_stream: LiveTerminalStream::new(terminal_render_mode()),
            coordinator,
            asr_service,
            submit_window,
            pending_specs: VecDeque::new(),
            in_flight: 0,
            next_job_id: 0,
            submitted_specs: HashMap::new(),
            audio_by_channel: HashMap::new(),
            pump_cadence: PumpCadenceController::new(config.chunk_stride_ms.max(1)),
        })
    }

    fn finish(mut self) -> Result<LiveStreamRuntimeOutcome, CliError> {
        self.force_pump(PumpDecisionReason::ForcedCaptureEnd)?;
        self.coordinator
            .transition_to(
                LiveRuntimePhase::Draining,
                "finalizing queue cleanup, reconciliation, and transcript assembly",
            )
            .map_err(|err| CliError::new(format!("failed to enter drain phase: {err}")))?;
        self.force_pump(PumpDecisionReason::ForcedPhaseTransition)?;
        self.drain_until_idle()?;
        self.coordinator
            .transition_to(
                LiveRuntimePhase::Shutdown,
                "runtime work finished; writing session artifacts and summary output",
            )
            .map_err(|err| CliError::new(format!("failed to enter shutdown phase: {err}")))?;
        self.force_pump(PumpDecisionReason::ForcedPhaseTransition)?;
        self.drain_until_idle()?;
        self.asr_service.close();
        self.force_pump(PumpDecisionReason::ForcedFlush)?;
        self.drain_until_idle()?;
        self.asr_service.join();
        self.terminal_stream.finish();

        let (_, mut output_sink, finalizer, runtime_summary) = self
            .coordinator
            .finalize()
            .map_err(|err| CliError::new(format!("failed to finalize live runtime: {err}")))?;
        output_sink.finalize_incremental_jsonl().map_err(|err| {
            CliError::new(format!(
                "failed to finalize live-stream incremental JSONL writer: {err}"
            ))
        })?;
        let runtime_summary = finalizer.summary.unwrap_or(runtime_summary);

        Ok(LiveStreamRuntimeOutcome {
            output_events: output_sink.events,
            runtime_summary,
            asr_worker_pool: self.asr_service.telemetry(),
        })
    }

    fn run_full_pump(&mut self) -> Result<(), CliError> {
        self.collect_pending_specs();
        self.submit_pending_specs()?;
        self.drain_ready_results()?;
        Ok(())
    }

    fn apply_pump_decision(&mut self, decision: PumpDecision) -> Result<(), CliError> {
        match decision {
            PumpDecision::Run(_) => self.run_full_pump(),
            PumpDecision::Skip(_) => Ok(()),
        }
    }

    fn force_pump(&mut self, reason: PumpDecisionReason) -> Result<(), CliError> {
        let decision = self.pump_cadence.force(reason);
        self.apply_pump_decision(decision)
    }

    fn drain_until_idle(&mut self) -> Result<(), CliError> {
        for _ in 0..4_096 {
            self.collect_pending_specs();
            self.submit_pending_specs()?;
            self.drain_ready_results()?;
            if self.pending_specs.is_empty() && self.in_flight == 0 {
                return Ok(());
            }
            if let Some(result) = self
                .asr_service
                .recv_result_timeout(Duration::from_millis(25))
            {
                self.handle_asr_result(result)?;
            }
        }
        Err(CliError::new(
            "live-stream runtime did not drain queued ASR work before shutdown",
        ))
    }

    fn collect_pending_specs(&mut self) {
        while let Some(spec) = self.coordinator.pop_next_job() {
            self.pending_specs.push_back(spec);
        }
    }

    fn pump_for_chunk(&mut self, chunk: &CaptureChunk) -> Result<(), CliError> {
        let pts_ms = if chunk.pts_seconds.is_finite() && chunk.pts_seconds >= 0.0 {
            Some((chunk.pts_seconds * 1_000.0).round() as u64)
        } else {
            None
        };
        let phase = self.coordinator.state().current_phase;
        let decision = self.pump_cadence.on_chunk(pts_ms, phase);
        self.apply_pump_decision(decision)
    }

    fn submit_pending_specs(&mut self) -> Result<(), CliError> {
        while self.in_flight < self.submit_window {
            let Some(spec) = self.pending_specs.pop_front() else {
                break;
            };
            let job_id = self.next_job_id;
            self.next_job_id += 1;
            let request = self.build_asr_request(job_id, &spec)?;
            if self.asr_service.submit_request(request).is_ok() {
                self.submitted_specs.insert(job_id, spec);
                self.in_flight += 1;
            }
        }
        Ok(())
    }

    fn drain_ready_results(&mut self) -> Result<(), CliError> {
        while let Some(result) = self.asr_service.try_recv_result() {
            self.handle_asr_result(result)?;
        }
        Ok(())
    }

    fn handle_asr_result(&mut self, result: LiveAsrJobResult) -> Result<(), CliError> {
        let Some(spec) = self.submitted_specs.remove(&result.job.job_id) else {
            return Ok(());
        };
        self.in_flight = self.in_flight.saturating_sub(1);
        let transcript_text = result.transcript_text.unwrap_or_default();
        let asr_result = LiveAsrResult {
            job: spec,
            transcript_text,
        };
        self.coordinator
            .on_asr_result(asr_result.clone())
            .map_err(|err| CliError::new(format!("failed to emit live ASR result: {err}")))?;
        if let Some(transcript_event) = transcript_event_from_runtime_asr_result(
            self.channel_mode,
            &self.speaker_labels,
            &asr_result,
        ) {
            self.terminal_stream.emit_event(&transcript_event);
        }
        Ok(())
    }

    fn build_asr_request(
        &mut self,
        job_id: usize,
        spec: &RuntimeAsrJobSpec,
    ) -> Result<LiveAsrRequest, CliError> {
        let channel_audio = self
            .audio_by_channel
            .get(spec.channel.as_str())
            .ok_or_else(|| {
                CliError::new(format!(
                    "live-stream runtime missing buffered audio for channel `{}`",
                    spec.channel
                ))
            })?;
        Ok(build_live_asr_request_for_spec(
            self.channel_mode,
            &self.speaker_labels,
            channel_audio,
            job_id,
            spec,
        ))
    }
}

impl CaptureSink for LiveStreamRuntimeExecution {
    fn on_chunk(&mut self, chunk: CaptureChunk) -> Result<(), String> {
        let pump_chunk = chunk.clone();
        let channel_key = match chunk.stream {
            recordit::capture_api::CaptureStream::Microphone => "microphone",
            recordit::capture_api::CaptureStream::SystemAudio => "system-audio",
        };
        self.audio_by_channel
            .entry(channel_key.to_string())
            .or_default()
            .ingest_chunk(&chunk);
        self.coordinator
            .on_capture_chunk(chunk)
            .map_err(|err| format!("live runtime rejected capture chunk: {err}"))?;
        self.pump_for_chunk(&pump_chunk)
            .map_err(|err| format!("live runtime pump failed after capture chunk: {err}"))?;
        Ok(())
    }

    fn on_event(&mut self, event: CaptureEvent) -> Result<(), String> {
        self.coordinator
            .on_capture_event(event)
            .map_err(|err| format!("live runtime rejected capture event: {err}"))?;
        self.force_pump(PumpDecisionReason::ForcedCaptureEvent)
            .map_err(|err| format!("live runtime pump failed after capture event: {err}"))?;
        Ok(())
    }
}

pub(super) fn run_live_stream_pipeline(
    config: &TranscribeConfig,
) -> Result<LiveRunReport, CliError> {
    let started_at = Instant::now();
    let resolved_model = validate_model_path_for_backend(config)?;
    let generated_at_utc = runtime_timestamp_utc();
    let stamp = command_stdout("date", &["-u", "+%Y%m%dT%H%M%SZ"])
        .unwrap_or_else(|_| "unknown".to_string());
    let backend_id = backend_id_for_asr_backend(config.asr_backend);

    let mut runtime = LiveStreamRuntimeExecution::new(config, &resolved_model.path, &stamp)?;
    let capture_output = live_capture_output_path(config).to_path_buf();
    let live_capture_config = LiveCaptureConfig {
        duration_secs: config.duration_sec,
        output: capture_output.clone(),
        target_rate_hz: config.sample_rate_hz,
        mismatch_policy: LiveCaptureSampleRateMismatchPolicy::AdaptStreamRate,
        callback_contract_mode: LiveCaptureCallbackMode::Warn,
        stop_request_path: graceful_stop_request_path(config),
    };
    let capture_result = run_streaming_capture_session(&live_capture_config, &mut runtime)
        .map_err(|err| CliError::new(format!("live capture session failed: {err}")))?;
    ensure_live_capture_output_exists(&capture_output)?;
    let (materialize_from, materialize_to) = live_capture_materialization_paths(config);
    materialize_out_wav(materialize_from, materialize_to)?;

    let runtime_outcome = {
        let _capture_summary = capture_result.summary;
        runtime.finish()?
    };
    let mut lifecycle = lifecycle_from_runtime_output_events(&runtime_outcome.output_events);
    if lifecycle.transitions.is_empty() {
        lifecycle.transition(
            LiveLifecyclePhase::Warmup,
            "preparing model, capture input, and channel routing",
        );
        lifecycle.transition(
            LiveLifecyclePhase::Active,
            "capture/model warmup complete; transcript chunks may emit now",
        );
        lifecycle.transition(
            LiveLifecyclePhase::Draining,
            "finalizing queue cleanup, reconciliation, and transcript assembly",
        );
        lifecycle.transition(
            LiveLifecyclePhase::Shutdown,
            "runtime work finished; writing session artifacts and summary output",
        );
    }

    let mut vad_boundaries =
        vad_boundaries_from_runtime_output_events(&runtime_outcome.output_events);
    let mut events =
        transcript_events_from_runtime_output_events(config, &runtime_outcome.output_events);
    events = merge_transcript_events(merge_live_transcript_events_for_display(events));

    let mut degradation_events = collect_live_capture_continuity_events(config);
    let final_buffering = FinalBufferingTelemetry {
        submit_window: config.chunk_queue_cap.max(config.live_asr_workers).max(1),
        deferred_final_submissions: 0,
        max_pending_final_backlog: 0,
    };
    let runtime_summary = runtime_outcome.runtime_summary;
    let chunk_queue = live_stream_chunk_queue_telemetry(
        config,
        &runtime_summary,
        &runtime_outcome.asr_worker_pool,
    );
    if chunk_queue.dropped_oldest > 0 {
        degradation_events.push(ModeDegradationEvent {
            code: LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE,
            detail: format!(
                "live-stream ASR queue dropped {} background task(s) under pressure (cap={}, submitted={}, processed={})",
                chunk_queue.dropped_oldest,
                chunk_queue.max_queue,
                chunk_queue.submitted,
                chunk_queue.processed
            ),
        });
        if chunk_queue_backpressure_is_severe(&chunk_queue) {
            degradation_events.push(ModeDegradationEvent {
                code: LIVE_CHUNK_QUEUE_BACKPRESSURE_SEVERE_CODE,
                detail: format!(
                    "live-stream ASR queue entered severe backpressure (dropped={}, submitted={}, cap={}, high_water={})",
                    chunk_queue.dropped_oldest,
                    chunk_queue.submitted,
                    chunk_queue.max_queue,
                    chunk_queue.high_water
                ),
            });
        }
    }

    let mut channel_transcripts = channel_transcript_summaries_from_events(config, &events);
    let mut reconciliation = build_reconciliation_matrix(&vad_boundaries, &degradation_events);
    let mut post_live_events = Vec::new();
    if reconciliation.required {
        let reconciliation_events = build_targeted_reconciliation_events(
            &channel_transcripts,
            &vad_boundaries,
            &events,
            &reconciliation,
        );
        if !reconciliation_events.is_empty() {
            post_live_events.extend(reconciliation_events);
            reconciliation.applied = true;
            degradation_events.push(ModeDegradationEvent {
                code: RECONCILIATION_APPLIED_CODE,
                detail: format!(
                    "targeted reconciliation emitted `reconciled_final` events for affected segments (triggers={})",
                    reconciliation.trigger_codes_csv()
                ),
            });
        }
    }

    let cleanup_run = run_cleanup_queue(config, &events);
    post_live_events.extend(cleanup_run.llm_events);
    if !post_live_events.is_empty() {
        events.extend(post_live_events);
        events = merge_transcript_events(events);
    }
    if vad_boundaries.is_empty() {
        vad_boundaries = fallback_vad_boundaries_from_events(&events);
    }
    channel_transcripts = channel_transcript_summaries_from_events(config, &events);
    let active_channel_mode = active_channel_mode_from_transcripts(config, &channel_transcripts);
    let hot_path_diagnostics = build_hot_path_diagnostics(
        config,
        &events,
        &channel_transcripts,
        &chunk_queue,
        &runtime_summary,
        &runtime_outcome.asr_worker_pool,
    );

    let cleanup_queue = cleanup_run.telemetry;
    let trust_notices = build_trust_notices(
        config.channel_mode,
        active_channel_mode,
        &degradation_events,
        &cleanup_queue,
        &chunk_queue,
    );
    let transcript_text = reconstruct_transcript(&events);

    let wall_ms_runs = vec![started_at.elapsed().as_secs_f64() * 1_000.0];
    let (benchmark_summary_csv, benchmark_runs_csv, benchmark) = write_benchmark_artifact(
        &stamp,
        backend_id,
        benchmark_track(active_channel_mode),
        &wall_ms_runs,
        config.out_manifest.parent(),
    )?;

    let report = LiveRunReport {
        generated_at_utc,
        backend_id,
        resolved_model_path: resolved_model.path,
        resolved_model_source: resolved_model.source,
        channel_mode: config.channel_mode,
        active_channel_mode,
        transcript_text,
        channel_transcripts,
        vad_boundaries,
        events,
        degradation_events,
        trust_notices,
        lifecycle,
        reconciliation,
        asr_worker_pool: runtime_outcome.asr_worker_pool,
        final_buffering,
        chunk_queue,
        cleanup_queue,
        hot_path_diagnostics,
        benchmark,
        benchmark_summary_csv,
        benchmark_runs_csv,
    };

    write_runtime_jsonl(config, &report)?;
    write_runtime_manifest(config, &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use recordit::live_stream_runtime::{
        BackpressureMode, CaptureScheduler, LiveAsrJobClass as RuntimeAsrJobClass,
        SchedulerPressureSnapshot,
    };

    fn test_speaker_labels() -> SpeakerLabels {
        SpeakerLabels::parse("mic,system").expect("valid default speaker labels")
    }

    #[test]
    fn pump_cadence_gate_defers_active_chunks_until_due() {
        let mut cadence = PumpCadenceController::new(200);
        assert_eq!(
            cadence.on_chunk(Some(0), LiveRuntimePhase::Active),
            PumpDecision::Run(PumpDecisionReason::CadenceDue)
        );
        assert_eq!(
            cadence.on_chunk(Some(50), LiveRuntimePhase::Active),
            PumpDecision::Skip(PumpDecisionReason::CadenceDeferred)
        );
        assert_eq!(
            cadence.on_chunk(Some(199), LiveRuntimePhase::Active),
            PumpDecision::Skip(PumpDecisionReason::CadenceDeferred)
        );
        assert_eq!(
            cadence.on_chunk(Some(200), LiveRuntimePhase::Active),
            PumpDecision::Run(PumpDecisionReason::CadenceDue)
        );

        let telemetry = cadence.telemetry();
        assert_eq!(telemetry.cadence_ms, 200);
        assert_eq!(telemetry.decisions, 4);
        assert_eq!(telemetry.full_pumps, 2);
        assert_eq!(telemetry.deferred_pumps, 2);
        assert_eq!(telemetry.forced_pumps, 0);
    }

    #[test]
    fn pump_cadence_gate_forces_drain_for_events_and_non_active_phases() {
        let mut cadence = PumpCadenceController::new(200);
        assert_eq!(
            cadence.on_chunk(Some(0), LiveRuntimePhase::Active),
            PumpDecision::Run(PumpDecisionReason::CadenceDue)
        );
        assert_eq!(
            cadence.on_chunk(Some(60), LiveRuntimePhase::Active),
            PumpDecision::Skip(PumpDecisionReason::CadenceDeferred)
        );
        assert_eq!(
            cadence.force(PumpDecisionReason::ForcedCaptureEvent),
            PumpDecision::Run(PumpDecisionReason::ForcedCaptureEvent)
        );
        assert_eq!(
            cadence.on_chunk(Some(120), LiveRuntimePhase::Draining),
            PumpDecision::Run(PumpDecisionReason::ForcedPhaseTransition)
        );
        assert_eq!(
            cadence.force(PumpDecisionReason::ForcedFlush),
            PumpDecision::Run(PumpDecisionReason::ForcedFlush)
        );

        let telemetry = cadence.telemetry();
        assert_eq!(telemetry.decisions, 5);
        assert_eq!(telemetry.full_pumps, 4);
        assert_eq!(telemetry.deferred_pumps, 1);
        assert_eq!(telemetry.forced_pumps, 3);
        assert_eq!(telemetry.last_reason, Some(PumpDecisionReason::ForcedFlush));
    }

    #[test]
    fn build_live_asr_request_for_spec_preserves_pcm_window_metadata() {
        let channel_audio = RuntimeChannelAudio {
            sample_rate_hz: 1_000,
            base_pts_ms: Some(0),
            samples: vec![0.1, 0.2, 0.3, 0.4],
        };
        let spec = RuntimeAsrJobSpec {
            emit_seq: 9,
            job_class: RuntimeAsrJobClass::Final,
            channel: "microphone".to_string(),
            segment_id: "seg-9".to_string(),
            segment_ord: 3,
            window_ord: 5,
            start_ms: 1,
            end_ms: 3,
        };

        let request = build_live_asr_request_for_spec(
            ChannelMode::Separate,
            &test_speaker_labels(),
            &channel_audio,
            77,
            &spec,
        );

        assert_eq!(request.job_id, 77);
        assert_eq!(request.class, LiveAsrJobClass::Final);
        assert_eq!(request.role, "mic");
        assert_eq!(request.label, "mic");
        assert_eq!(request.segment_id, "seg-9");
        match request.audio_input {
            LiveAsrAudioInput::PcmWindow {
                sample_rate_hz,
                start_ms,
                end_ms,
                mono_samples,
            } => {
                assert_eq!(sample_rate_hz, 1_000);
                assert_eq!(start_ms, 1);
                assert_eq!(end_ms, 3);
                assert_eq!(mono_samples, vec![0.2, 0.3]);
            }
            _ => assert!(false, "expected pcm window request"),
        }
    }

    #[test]
    fn build_live_asr_request_for_spec_keeps_zero_sample_fallback_for_empty_windows() {
        let channel_audio = RuntimeChannelAudio::default();
        let spec = RuntimeAsrJobSpec {
            emit_seq: 3,
            job_class: RuntimeAsrJobClass::Partial,
            channel: "system-audio".to_string(),
            segment_id: "seg-empty".to_string(),
            segment_ord: 1,
            window_ord: 2,
            start_ms: 10,
            end_ms: 20,
        };

        let request = build_live_asr_request_for_spec(
            ChannelMode::Separate,
            &test_speaker_labels(),
            &channel_audio,
            11,
            &spec,
        );

        match request.audio_input {
            LiveAsrAudioInput::PcmWindow {
                sample_rate_hz,
                start_ms,
                end_ms,
                mono_samples,
            } => {
                assert_eq!(sample_rate_hz, 1);
                assert_eq!(start_ms, 10);
                assert_eq!(end_ms, 20);
                assert_eq!(mono_samples, vec![0.0]);
            }
            _ => assert!(false, "expected pcm window request"),
        }
    }

    #[test]
    fn live_stream_backpressure_kill_switch_sets_unreachable_thresholds() {
        let mut config = TranscribeConfig::default();
        config.live_stream = true;
        config.adaptive_backpressure_enabled = false;

        let backpressure = live_stream_backpressure_config(&config);
        assert_eq!(backpressure.pressure_pending_jobs_threshold, u64::MAX);
        assert_eq!(backpressure.severe_pending_jobs_threshold, u64::MAX);
        assert_eq!(backpressure.severe_pending_final_jobs_threshold, u64::MAX);
        assert_eq!(backpressure.pressure_min_observations, usize::MAX);
        assert_eq!(backpressure.severe_min_observations, usize::MAX);
        assert_eq!(backpressure.pressure_stride_multiplier, 1);
    }

    #[test]
    fn kill_switch_keeps_scheduler_in_normal_mode_under_extreme_pressure() {
        let mut config = TranscribeConfig::default();
        config.live_stream = true;
        config.adaptive_backpressure_enabled = false;

        let scheduler_config = StreamingSchedulerConfig {
            partial_window_ms: config.chunk_window_ms,
            partial_stride_ms: config.chunk_stride_ms,
            min_partial_span_ms: config.chunk_stride_ms.min(config.chunk_window_ms).max(1),
            backpressure: live_stream_backpressure_config(&config),
        };
        let mut scheduler =
            StreamingVadScheduler::with_configs(StreamingVadConfig::default(), scheduler_config);

        for idx in 0..4 {
            let input = recordit::live_stream_runtime::SchedulingInput {
                channel: "microphone".to_string(),
                pts_ms: idx * 120,
                frame_count: 1_920,
                duration_ms: 120,
                activity_level_per_mille: 80,
            };
            let _ = scheduler.on_capture(
                input,
                LiveRuntimePhase::Active,
                SchedulerPressureSnapshot {
                    pending_jobs: u64::MAX,
                    pending_final_jobs: u64::MAX,
                },
            );
        }

        assert_eq!(scheduler.backpressure_mode(), BackpressureMode::Normal);
        assert!(scheduler.drain_backpressure_transitions().is_empty());
    }

    #[test]
    fn kill_switch_preserves_normal_job_emission_under_pressure() {
        let mut kill_switch_config = TranscribeConfig::default();
        kill_switch_config.live_stream = true;
        kill_switch_config.adaptive_backpressure_enabled = false;

        let kill_switch_scheduler_config = StreamingSchedulerConfig {
            partial_window_ms: kill_switch_config.chunk_window_ms,
            partial_stride_ms: kill_switch_config.chunk_stride_ms,
            min_partial_span_ms: kill_switch_config
                .chunk_stride_ms
                .min(kill_switch_config.chunk_window_ms)
                .max(1),
            backpressure: live_stream_backpressure_config(&kill_switch_config),
        };
        let baseline_scheduler_config = StreamingSchedulerConfig {
            partial_window_ms: kill_switch_config.chunk_window_ms,
            partial_stride_ms: kill_switch_config.chunk_stride_ms,
            min_partial_span_ms: kill_switch_config
                .chunk_stride_ms
                .min(kill_switch_config.chunk_window_ms)
                .max(1),
            backpressure: recordit::live_stream_runtime::AdaptiveBackpressureConfig::default(),
        };

        let mut kill_switch_scheduler = StreamingVadScheduler::with_configs(
            StreamingVadConfig::default(),
            kill_switch_scheduler_config,
        );
        let mut baseline_scheduler = StreamingVadScheduler::with_configs(
            StreamingVadConfig::default(),
            baseline_scheduler_config,
        );
        let mut kill_switch_jobs = Vec::new();
        let mut baseline_jobs = Vec::new();

        for idx in 0..8 {
            let input = recordit::live_stream_runtime::SchedulingInput {
                channel: "microphone".to_string(),
                pts_ms: idx * 120,
                frame_count: 1_920,
                duration_ms: 120,
                activity_level_per_mille: 80,
            };
            kill_switch_jobs.extend(kill_switch_scheduler.on_capture(
                input.clone(),
                LiveRuntimePhase::Active,
                SchedulerPressureSnapshot {
                    pending_jobs: u64::MAX,
                    pending_final_jobs: u64::MAX,
                },
            ));
            baseline_jobs.extend(baseline_scheduler.on_capture(
                input,
                LiveRuntimePhase::Active,
                SchedulerPressureSnapshot::default(),
            ));
        }

        assert_eq!(
            kill_switch_scheduler.backpressure_mode(),
            BackpressureMode::Normal
        );
        assert_eq!(kill_switch_jobs, baseline_jobs);
        assert!(
            !kill_switch_jobs.is_empty(),
            "expected partial/final job emission to remain available with kill-switch active"
        );
    }
    #[test]
    fn graceful_stop_request_path_uses_manifest_parent() {
        let mut config = TranscribeConfig::default();
        config.out_manifest = PathBuf::from("artifacts/sessions/live-case/session.manifest.json");

        let path = graceful_stop_request_path(&config)
            .expect("manifest parent should produce stop marker path");
        assert_eq!(
            path,
            PathBuf::from("artifacts/sessions/live-case/session.stop.request")
        );
    }

    #[test]
    fn graceful_stop_request_path_returns_none_without_parent() {
        let mut config = TranscribeConfig::default();
        config.out_manifest = PathBuf::from("session.manifest.json");

        assert_eq!(graceful_stop_request_path(&config), None);
    }
}
