use super::*;

struct LiveStreamRuntimeOutcome {
    output_events: Vec<RuntimeOutputEvent>,
    runtime_summary: LiveRuntimeSummary,
    asr_worker_pool: LiveAsrPoolTelemetry,
}

struct LiveStreamRuntimeExecution {
    channel_mode: ChannelMode,
    speaker_labels: SpeakerLabels,
    segment_dir: PathBuf,
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

impl LiveStreamRuntimeExecution {
    fn new(
        config: &TranscribeConfig,
        resolved_model_path: &Path,
        stamp: &str,
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

        let helper_program = resolve_backend_program(config.asr_backend, resolved_model_path);
        let executor = Arc::new(PooledAsrExecutor {
            backend: config.asr_backend,
            helper_program,
            model_path: resolved_model_path.to_path_buf(),
            language: config.asr_language.clone(),
            threads: config.asr_threads,
            prewarm_enabled: true,
        });
        let temp_audio_policy = if config.keep_temp_audio {
            TempAudioPolicy::RetainAlways
        } else {
            TempAudioPolicy::RetainOnFailure
        };
        let asr_service = LiveAsrService::start(
            executor,
            LiveAsrPoolConfig {
                worker_count: config.live_asr_workers.max(1),
                queue_capacity: config.chunk_queue_cap.max(config.live_asr_workers).max(1),
                retries: 0,
                temp_audio_policy,
            },
        );

        let segment_dir = PathBuf::from("artifacts")
            .join("transcribe-live-runtime-segments")
            .join(stamp);
        fs::create_dir_all(&segment_dir).map_err(|err| {
            CliError::new(format!(
                "failed to create live-stream segment directory {}: {err}",
                segment_dir.display()
            ))
        })?;

        let submit_window = asr_service.queue_capacity().max(1);
        Ok(Self {
            channel_mode: config.channel_mode,
            speaker_labels: config.speaker_labels.clone(),
            segment_dir,
            terminal_stream: LiveTerminalStream::new(terminal_render_mode()),
            coordinator,
            asr_service,
            submit_window,
            pending_specs: VecDeque::new(),
            in_flight: 0,
            next_job_id: 0,
            submitted_specs: HashMap::new(),
            audio_by_channel: HashMap::new(),
        })
    }

    fn finish(mut self) -> Result<LiveStreamRuntimeOutcome, CliError> {
        self.pump_once()?;
        self.coordinator
            .transition_to(
                LiveRuntimePhase::Draining,
                "finalizing queue cleanup, reconciliation, and transcript assembly",
            )
            .map_err(|err| CliError::new(format!("failed to enter drain phase: {err}")))?;
        self.drain_until_idle()?;
        self.coordinator
            .transition_to(
                LiveRuntimePhase::Shutdown,
                "runtime work finished; writing session artifacts and summary output",
            )
            .map_err(|err| CliError::new(format!("failed to enter shutdown phase: {err}")))?;
        self.drain_until_idle()?;
        self.asr_service.close();
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

    fn pump_once(&mut self) -> Result<(), CliError> {
        self.collect_pending_specs();
        self.submit_pending_specs()?;
        self.drain_ready_results()?;
        Ok(())
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

    fn submit_pending_specs(&mut self) -> Result<(), CliError> {
        while self.in_flight < self.submit_window {
            let Some(spec) = self.pending_specs.pop_front() else {
                break;
            };
            let job_id = self.next_job_id;
            self.next_job_id += 1;
            let job = self.build_asr_job(job_id, &spec)?;
            if self.asr_service.submit(job).is_ok() {
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

    fn build_asr_job(
        &mut self,
        job_id: usize,
        spec: &RuntimeAsrJobSpec,
    ) -> Result<LiveAsrJob, CliError> {
        let audio_path = self.materialize_job_audio(job_id, spec)?;
        Ok(LiveAsrJob {
            job_id,
            class: runtime_job_class_to_pool_job_class(spec.job_class),
            role: runtime_channel_role(spec.channel.as_str()),
            label: runtime_channel_label(
                self.channel_mode,
                &self.speaker_labels,
                spec.channel.as_str(),
            ),
            segment_id: spec.segment_id.clone(),
            audio_path,
            is_temp_audio: true,
        })
    }

    fn materialize_job_audio(
        &mut self,
        job_id: usize,
        spec: &RuntimeAsrJobSpec,
    ) -> Result<PathBuf, CliError> {
        let channel_audio = self
            .audio_by_channel
            .get(spec.channel.as_str())
            .ok_or_else(|| {
                CliError::new(format!(
                    "live-stream runtime missing buffered audio for channel `{}`",
                    spec.channel
                ))
            })?;
        let mut samples = channel_audio.render_window(spec.start_ms, spec.end_ms);
        if samples.is_empty() {
            samples.push(0.0);
        }
        let audio_path = self.segment_dir.join(format!("job-{job_id:06}.wav"));
        write_runtime_job_wav(&audio_path, channel_audio.sample_rate_hz.max(1), &samples)?;
        Ok(audio_path)
    }
}

impl CaptureSink for LiveStreamRuntimeExecution {
    fn on_chunk(&mut self, chunk: CaptureChunk) -> Result<(), String> {
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
        self.pump_once()
            .map_err(|err| format!("live runtime pump failed after capture chunk: {err}"))?;
        Ok(())
    }

    fn on_event(&mut self, event: CaptureEvent) -> Result<(), String> {
        self.coordinator
            .on_capture_event(event)
            .map_err(|err| format!("live runtime rejected capture event: {err}"))?;
        self.pump_once()
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
    let chunk_queue = live_stream_chunk_queue_telemetry(
        config,
        &runtime_outcome.runtime_summary,
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
        benchmark,
        benchmark_summary_csv,
        benchmark_runs_csv,
    };

    write_runtime_jsonl(config, &report)?;
    write_runtime_manifest(config, &report)?;
    Ok(report)
}
