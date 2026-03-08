use super::*;

pub(super) fn run_representative_offline_pipeline(
    config: &TranscribeConfig,
) -> Result<LiveRunReport, CliError> {
    let mut offline_config = config.clone();
    offline_config.live_chunked = false;
    offline_config.live_stream = false;
    run_standard_pipeline(&offline_config)
}

pub(super) fn run_representative_chunked_pipeline(
    config: &TranscribeConfig,
) -> Result<LiveRunReport, CliError> {
    let mut chunked_config = config.clone();
    chunked_config.live_chunked = true;
    chunked_config.live_stream = false;
    run_standard_pipeline(&chunked_config)
}

pub(super) fn run_standard_pipeline(config: &TranscribeConfig) -> Result<LiveRunReport, CliError> {
    let mut lifecycle = LiveLifecycleTelemetry::new();
    let mut jsonl_stream = RuntimeJsonlStream::open(&config.out_jsonl)?;
    lifecycle.transition(
        LiveLifecyclePhase::Warmup,
        "preparing model, capture input, and channel routing",
    );
    emit_latest_lifecycle_transition_jsonl(&mut jsonl_stream, &lifecycle)?;
    let resolved_model = validate_model_path_for_backend(config)?;
    prepare_runtime_input_wav(config)?;
    if !config.live_stream {
        materialize_out_wav(&config.input_wav, &config.out_wav)?;
    }

    let generated_at_utc = runtime_timestamp_utc();
    let stamp = command_stdout("date", &["-u", "+%Y%m%dT%H%M%SZ"])
        .unwrap_or_else(|_| "unknown".to_string());
    let backend_id = backend_id_for_asr_backend(config.asr_backend);
    let channel_plan = prepare_channel_inputs(config, &stamp)?;
    let refresh_channel_inputs_per_run =
        channel_plan.inputs.iter().any(|input| input.is_temp_audio);
    lifecycle.transition(
        LiveLifecyclePhase::Active,
        "capture/model warmup complete; transcript chunks may emit now",
    );
    emit_latest_lifecycle_transition_jsonl(&mut jsonl_stream, &lifecycle)?;
    let mut wall_ms_runs = Vec::with_capacity(config.benchmark_runs);
    let mut first_channel_transcripts = Vec::new();
    let mut asr_worker_pool = LiveAsrPoolTelemetry {
        prewarm_ok: true,
        ..LiveAsrPoolTelemetry::default()
    };
    let mut final_buffering = FinalBufferingTelemetry::default();
    for run_idx in 0..config.benchmark_runs {
        let run_inputs = if run_idx == 0 {
            channel_plan.inputs.clone()
        } else if refresh_channel_inputs_per_run {
            let run_stamp = format!("{stamp}-run-{run_idx:02}");
            prepare_channel_inputs(config, &run_stamp)?.inputs
        } else {
            channel_plan.inputs.clone()
        };
        let started_at = Instant::now();
        let run = transcribe_channels_once(
            config,
            &resolved_model.path,
            &run_inputs,
            run_idx == 0 && (config.live_chunked || config.live_stream),
        )?;
        wall_ms_runs.push(started_at.elapsed().as_secs_f64() * 1_000.0);
        absorb_live_asr_pool_telemetry(&mut asr_worker_pool, &run.asr_worker_pool);
        absorb_final_buffering_telemetry(&mut final_buffering, &run.final_buffering);
        if first_channel_transcripts.is_empty() {
            first_channel_transcripts = run.summaries;
        }
    }
    let vad_boundaries = detect_vad_boundaries_from_wav(
        &config.input_wav,
        config.vad_threshold,
        config.vad_min_speech_ms,
        config.vad_min_silence_ms,
    )?;
    for boundary in &vad_boundaries {
        jsonl_stream.write_line(&jsonl_vad_boundary_line(boundary, config))?;
    }
    jsonl_stream.checkpoint()?;
    let mut degradation_events = channel_plan.degradation_events;
    let (mut events, chunk_queue) = if config.live_chunked {
        let live_chunked = build_live_chunked_events_with_queue(
            &first_channel_transcripts,
            &vad_boundaries,
            config.chunk_window_ms,
            config.chunk_stride_ms,
            config.chunk_queue_cap,
        );
        if live_chunked.telemetry.dropped_oldest > 0 {
            degradation_events.push(ModeDegradationEvent {
                code: LIVE_CHUNK_QUEUE_DROP_OLDEST_CODE,
                detail: format!(
                    "near-live ASR chunk queue dropped {} oldest task(s) under pressure (cap={}, submitted={}, processed={})",
                    live_chunked.telemetry.dropped_oldest,
                    live_chunked.telemetry.max_queue,
                    live_chunked.telemetry.submitted,
                    live_chunked.telemetry.processed
                ),
            });
            if chunk_queue_backpressure_is_severe(&live_chunked.telemetry) {
                degradation_events.push(ModeDegradationEvent {
                    code: LIVE_CHUNK_QUEUE_BACKPRESSURE_SEVERE_CODE,
                    detail: format!(
                        "near-live ASR queue entered severe backpressure (dropped={}, submitted={}, cap={}, high_water={})",
                        live_chunked.telemetry.dropped_oldest,
                        live_chunked.telemetry.submitted,
                        live_chunked.telemetry.max_queue,
                        live_chunked.telemetry.high_water
                    ),
                });
            }
        }
        (
            merge_transcript_events(live_chunked.events),
            live_chunked.telemetry,
        )
    } else {
        (
            merge_transcript_events(
                first_channel_transcripts
                    .iter()
                    .flat_map(|transcript| {
                        build_transcript_events(
                            &transcript.text,
                            &vad_boundaries,
                            &transcript.label,
                            transcript.role,
                            false,
                            config.chunk_window_ms,
                            config.chunk_stride_ms,
                        )
                    })
                    .collect(),
            ),
            LiveChunkQueueTelemetry::disabled(config),
        )
    };
    maybe_emit_live_terminal_stream(config, &events);
    for event in &events {
        jsonl_stream.write_line(&jsonl_transcript_event_line(
            event,
            backend_id,
            vad_boundaries.len(),
        ))?;
    }
    jsonl_stream.checkpoint()?;
    degradation_events.extend(collect_live_capture_continuity_events(config));
    let mut reconciliation = if config.live_chunked {
        build_reconciliation_matrix(&vad_boundaries, &degradation_events)
    } else {
        ReconciliationMatrix::none()
    };
    lifecycle.transition(
        LiveLifecyclePhase::Draining,
        "finalizing queue cleanup, reconciliation, and transcript assembly",
    );
    emit_latest_lifecycle_transition_jsonl(&mut jsonl_stream, &lifecycle)?;
    let cleanup_run = run_cleanup_queue(config, &events);
    let mut post_live_events = Vec::new();
    if config.live_chunked && reconciliation.required {
        let reconciliation_events = build_targeted_reconciliation_events(
            &first_channel_transcripts,
            &vad_boundaries,
            &events,
            &reconciliation,
        );
        if !reconciliation_events.is_empty() {
            for event in &reconciliation_events {
                jsonl_stream.write_line(&jsonl_transcript_event_line(
                    event,
                    backend_id,
                    vad_boundaries.len(),
                ))?;
            }
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
    for event in &cleanup_run.llm_events {
        jsonl_stream.write_line(&jsonl_transcript_event_line(
            event,
            backend_id,
            vad_boundaries.len(),
        ))?;
    }
    post_live_events.extend(cleanup_run.llm_events);
    if !post_live_events.is_empty() {
        jsonl_stream.checkpoint()?;
        events.extend(post_live_events);
    }
    let events = merge_transcript_events(events);
    let cleanup_queue = cleanup_run.telemetry;
    let trust_notices = build_trust_notices(
        config.channel_mode,
        channel_plan.active_mode,
        &degradation_events,
        &cleanup_queue,
        &chunk_queue,
    );
    let transcript_text = reconstruct_transcript(&events);
    let (benchmark_summary_csv, benchmark_runs_csv, benchmark) = write_benchmark_artifact(
        &stamp,
        backend_id,
        benchmark_track(channel_plan.active_mode),
        &wall_ms_runs,
        config.out_manifest.parent(),
    )?;
    if config.live_stream {
        materialize_out_wav(&config.input_wav, &config.out_wav)?;
    }
    lifecycle.transition(
        LiveLifecyclePhase::Shutdown,
        "runtime work finished; writing session artifacts and summary output",
    );
    emit_latest_lifecycle_transition_jsonl(&mut jsonl_stream, &lifecycle)?;
    for degradation in &degradation_events {
        jsonl_stream.write_line(&jsonl_mode_degradation_line(
            config.channel_mode,
            channel_plan.active_mode,
            degradation,
        ))?;
    }
    for notice in &trust_notices {
        jsonl_stream.write_line(&jsonl_trust_notice_line(notice))?;
    }
    jsonl_stream.write_line(&jsonl_reconciliation_matrix_line(&reconciliation))?;
    jsonl_stream.write_line(&jsonl_asr_worker_pool_line(&asr_worker_pool))?;
    jsonl_stream.write_line(&jsonl_chunk_queue_line(&chunk_queue))?;
    jsonl_stream.write_line(&jsonl_cleanup_queue_line(&cleanup_queue))?;
    jsonl_stream.checkpoint()?;
    jsonl_stream.finalize()?;

    let report = LiveRunReport {
        generated_at_utc,
        backend_id,
        resolved_model_path: resolved_model.path,
        resolved_model_source: resolved_model.source,
        channel_mode: config.channel_mode,
        active_channel_mode: channel_plan.active_mode,
        transcript_text,
        channel_transcripts: first_channel_transcripts,
        vad_boundaries,
        events,
        degradation_events,
        trust_notices,
        lifecycle,
        reconciliation,
        asr_worker_pool,
        final_buffering,
        chunk_queue,
        cleanup_queue,
        hot_path_diagnostics: HotPathDiagnostics::default(),
        benchmark,
        benchmark_summary_csv,
        benchmark_runs_csv,
    };

    write_runtime_manifest(config, &report)?;
    Ok(report)
}

pub(super) fn prepare_channel_inputs(
    config: &TranscribeConfig,
    stamp: &str,
) -> Result<ChannelInputPlanResult, CliError> {
    match config.channel_mode {
        ChannelMode::Mixed => Ok(ChannelInputPlanResult {
            inputs: vec![ChannelInputPlan {
                role: "mixed",
                label: "merged".to_string(),
                audio_path: config.input_wav.clone(),
                is_temp_audio: false,
            }],
            active_mode: ChannelMode::Mixed,
            degradation_events: Vec::new(),
        }),
        ChannelMode::Separate => Ok(ChannelInputPlanResult {
            inputs: prepare_separate_channel_inputs(
                &config.input_wav,
                &config.speaker_labels,
                stamp,
            )?,
            active_mode: ChannelMode::Separate,
            degradation_events: Vec::new(),
        }),
        ChannelMode::MixedFallback => {
            let channel_count = wav_channel_count(&config.input_wav)?;
            if channel_count < 2 {
                Ok(ChannelInputPlanResult {
                    inputs: vec![ChannelInputPlan {
                        role: "mixed",
                        label: "merged".to_string(),
                        audio_path: config.input_wav.clone(),
                        is_temp_audio: false,
                    }],
                    active_mode: ChannelMode::Mixed,
                    degradation_events: vec![ModeDegradationEvent {
                        code: "fallback_to_mixed",
                        detail: format!(
                            "requested mixed-fallback but input had {channel_count} channel(s); using merged mixed mode"
                        ),
                    }],
                })
            } else {
                Ok(ChannelInputPlanResult {
                    inputs: prepare_separate_channel_inputs(
                        &config.input_wav,
                        &config.speaker_labels,
                        stamp,
                    )?,
                    active_mode: ChannelMode::Separate,
                    degradation_events: Vec::new(),
                })
            }
        }
    }
}

pub(super) fn prepare_separate_channel_inputs(
    input_wav: &Path,
    speaker_labels: &SpeakerLabels,
    stamp: &str,
) -> Result<Vec<ChannelInputPlan>, CliError> {
    let channel_count = wav_channel_count(input_wav)?;
    if channel_count < 2 {
        return Ok(vec![
            ChannelInputPlan {
                role: "mic",
                label: speaker_labels.mic.clone(),
                audio_path: input_wav.to_path_buf(),
                is_temp_audio: false,
            },
            ChannelInputPlan {
                role: "system",
                label: speaker_labels.system.clone(),
                audio_path: input_wav.to_path_buf(),
                is_temp_audio: false,
            },
        ]);
    }

    let slice_dir = PathBuf::from("artifacts")
        .join("transcribe-live-channel-slices")
        .join(stamp);
    fs::create_dir_all(&slice_dir).map_err(|err| {
        CliError::new(format!(
            "failed to create channel slice directory {}: {err}",
            slice_dir.display()
        ))
    })?;

    let mic_path = slice_dir.join("mic.wav");
    let system_path = slice_dir.join("system.wav");
    extract_channel_wav(input_wav, 0, &mic_path)?;
    extract_channel_wav(input_wav, 1, &system_path)?;

    Ok(vec![
        ChannelInputPlan {
            role: "mic",
            label: speaker_labels.mic.clone(),
            audio_path: mic_path,
            is_temp_audio: true,
        },
        ChannelInputPlan {
            role: "system",
            label: speaker_labels.system.clone(),
            audio_path: system_path,
            is_temp_audio: true,
        },
    ])
}

pub(super) fn wav_channel_count(path: &Path) -> Result<u16, CliError> {
    let reader = WavReader::open(path).map_err(|err| {
        CliError::new(format!(
            "failed to inspect WAV {}: {err}",
            display_path(path)
        ))
    })?;
    Ok(reader.spec().channels)
}

pub(super) fn extract_channel_wav(
    input_wav: &Path,
    channel_index: usize,
    output_wav: &Path,
) -> Result<(), CliError> {
    if let Some(parent) = output_wav.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CliError::new(format!(
                "failed to create channel output directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let mut reader = WavReader::open(input_wav).map_err(|err| {
        CliError::new(format!(
            "failed to open WAV {} for channel extraction: {err}",
            display_path(input_wav)
        ))
    })?;
    let spec = reader.spec();
    let channel_count = spec.channels as usize;
    if channel_index >= channel_count {
        return Err(CliError::new(format!(
            "cannot extract channel {} from {} channel WAV {}",
            channel_index,
            channel_count,
            display_path(input_wav)
        )));
    }

    let mono_spec = hound::WavSpec {
        channels: 1,
        sample_rate: spec.sample_rate,
        bits_per_sample: spec.bits_per_sample,
        sample_format: spec.sample_format,
    };
    let mut writer = hound::WavWriter::create(output_wav, mono_spec).map_err(|err| {
        CliError::new(format!(
            "failed to create channel WAV {}: {err}",
            display_path(output_wav)
        ))
    })?;

    match spec.sample_format {
        SampleFormat::Float => {
            for (idx, sample) in reader.samples::<f32>().enumerate() {
                let sample = sample
                    .map_err(|err| CliError::new(format!("failed to read float sample: {err}")))?;
                if idx % channel_count == channel_index {
                    writer.write_sample(sample).map_err(|err| {
                        CliError::new(format!("failed to write float sample: {err}"))
                    })?;
                }
            }
        }
        SampleFormat::Int => {
            if spec.bits_per_sample <= 16 {
                for (idx, sample) in reader.samples::<i16>().enumerate() {
                    let sample = sample.map_err(|err| {
                        CliError::new(format!("failed to read i16 sample: {err}"))
                    })?;
                    if idx % channel_count == channel_index {
                        writer.write_sample(sample).map_err(|err| {
                            CliError::new(format!("failed to write i16 sample: {err}"))
                        })?;
                    }
                }
            } else {
                for (idx, sample) in reader.samples::<i32>().enumerate() {
                    let sample = sample.map_err(|err| {
                        CliError::new(format!("failed to read i32 sample: {err}"))
                    })?;
                    if idx % channel_count == channel_index {
                        writer.write_sample(sample).map_err(|err| {
                            CliError::new(format!("failed to write i32 sample: {err}"))
                        })?;
                    }
                }
            }
        }
    }

    writer
        .finalize()
        .map_err(|err| CliError::new(format!("failed to finalize channel WAV: {err}")))?;
    Ok(())
}

pub(super) fn transcribe_channels_once(
    config: &TranscribeConfig,
    resolved_model_path: &Path,
    channel_inputs: &[ChannelInputPlan],
    prewarm_enabled: bool,
) -> Result<ChannelTranscriptionRun, CliError> {
    let worker_count = config
        .live_asr_workers
        .max(1)
        .min(channel_inputs.len().max(1));
    let mut ordered_inputs = channel_inputs.to_vec();
    ordered_inputs.sort_by(|a, b| {
        channel_sort_key(a.role)
            .cmp(&channel_sort_key(b.role))
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.audio_path.cmp(&b.audio_path))
    });

    let jobs = ordered_inputs
        .iter()
        .enumerate()
        .map(|(idx, input)| LiveAsrJob {
            job_id: idx,
            class: LiveAsrJobClass::Final,
            role: input.role,
            label: input.label.clone(),
            segment_id: format!("{}-{idx:04}", input.role),
            audio_path: input.audio_path.clone(),
            is_temp_audio: input.is_temp_audio,
        })
        .collect::<Vec<_>>();
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
        prewarm_enabled,
    });
    let (results, telemetry, final_buffering) = run_live_asr_pool_with_final_buffering(
        executor,
        jobs,
        LiveAsrPoolConfig {
            worker_count,
            queue_capacity: worker_count.max(config.chunk_queue_cap),
            retries: 0,
            temp_audio_policy,
        },
    );

    let mut summaries = Vec::with_capacity(results.len());
    let mut errors = Vec::new();
    for result in results {
        if let Some(err) = result.error {
            errors.push(format!(
                "{}:{}:{}",
                result.job.role,
                result.job.label,
                clean_field(&err)
            ));
            continue;
        }
        summaries.push(ChannelTranscriptSummary {
            role: result.job.role,
            label: result.job.label,
            text: result
                .transcript_text
                .unwrap_or_else(|| "<no speech detected>".to_string()),
        });
    }
    if !errors.is_empty() {
        return Err(CliError::new(format!(
            "ASR worker pool failed for {} task(s): {}",
            errors.len(),
            errors.join(" | ")
        )));
    }

    summaries.sort_by(|a, b| {
        channel_sort_key(a.role)
            .cmp(&channel_sort_key(b.role))
            .then_with(|| a.label.cmp(&b.label))
    });
    Ok(ChannelTranscriptionRun {
        summaries,
        asr_worker_pool: telemetry,
        final_buffering,
    })
}

pub(super) fn run_live_asr_pool_with_final_buffering(
    executor: Arc<dyn LiveAsrExecutor>,
    jobs: Vec<LiveAsrJob>,
    config: LiveAsrPoolConfig,
) -> (
    Vec<LiveAsrJobResult>,
    LiveAsrPoolTelemetry,
    FinalBufferingTelemetry,
) {
    let expected_results = jobs.len();
    let mut service = LiveAsrService::start(executor, config);

    // Keep accepted in-flight finals bounded by service queue capacity so the
    // coordinator can defer remaining finals and retry submission as results drain.
    let submit_window = service.queue_capacity().max(1);
    let mut pending_jobs: VecDeque<LiveAsrJob> = jobs.into_iter().collect();
    let mut in_flight = 0usize;
    let mut results = Vec::with_capacity(expected_results);
    let mut final_buffering = FinalBufferingTelemetry {
        submit_window,
        deferred_final_submissions: expected_results.saturating_sub(submit_window),
        max_pending_final_backlog: 0,
    };

    while results.len() < expected_results {
        while in_flight < submit_window {
            let Some(job) = pending_jobs.pop_front() else {
                break;
            };
            if service.submit_request(job.into_request()).is_ok() {
                in_flight += 1;
            }
        }
        final_buffering.max_pending_final_backlog = final_buffering
            .max_pending_final_backlog
            .max(pending_jobs.len());

        match service.recv_result_timeout(Duration::from_millis(25)) {
            Some(result) => {
                in_flight = in_flight.saturating_sub(1);
                results.push(result);
            }
            None if pending_jobs.is_empty() && in_flight == 0 => break,
            None => {}
        }
    }

    service.close();
    while results.len() < expected_results {
        match service.recv_result_timeout(Duration::from_millis(25)) {
            Some(result) => {
                in_flight = in_flight.saturating_sub(1);
                results.push(result);
            }
            None => break,
        }
    }
    service.join();

    let telemetry = service.telemetry();
    results.sort_by_key(|result| result.job.job_id);
    (results, telemetry, final_buffering)
}

pub(super) fn absorb_live_asr_pool_telemetry(
    aggregate: &mut LiveAsrPoolTelemetry,
    run: &LiveAsrPoolTelemetry,
) {
    aggregate.prewarm_ok &= run.prewarm_ok;
    aggregate.submitted += run.submitted;
    aggregate.enqueued += run.enqueued;
    aggregate.dropped_queue_full += run.dropped_queue_full;
    aggregate.processed += run.processed;
    aggregate.succeeded += run.succeeded;
    aggregate.failed += run.failed;
    aggregate.retry_attempts += run.retry_attempts;
    aggregate.temp_audio_retained += run.temp_audio_retained;
    aggregate.temp_audio_deleted += run.temp_audio_deleted;
}

pub(super) fn absorb_final_buffering_telemetry(
    aggregate: &mut FinalBufferingTelemetry,
    run: &FinalBufferingTelemetry,
) {
    aggregate.submit_window = aggregate.submit_window.max(run.submit_window);
    aggregate.deferred_final_submissions += run.deferred_final_submissions;
    aggregate.max_pending_final_backlog = aggregate
        .max_pending_final_backlog
        .max(run.max_pending_final_backlog);
}
