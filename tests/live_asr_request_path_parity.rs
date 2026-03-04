use recordit::live_asr_pool::{
    LiveAsrExecutor, LiveAsrJob, LiveAsrJobClass, LiveAsrPoolConfig, LiveAsrPoolTelemetry,
    LiveAsrRequest, LiveAsrService, TempAudioPolicy,
};
use std::collections::HashSet;
use std::fs;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum SubmitMode {
    LegacyPath,
    RequestPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelemetryDigest {
    submitted: usize,
    enqueued: usize,
    dropped_queue_full: usize,
    processed: usize,
    succeeded: usize,
    failed: usize,
    retry_attempts: usize,
    temp_audio_retained: usize,
    temp_audio_deleted: usize,
}

impl From<LiveAsrPoolTelemetry> for TelemetryDigest {
    fn from(value: LiveAsrPoolTelemetry) -> Self {
        Self {
            submitted: value.submitted,
            enqueued: value.enqueued,
            dropped_queue_full: value.dropped_queue_full,
            processed: value.processed,
            succeeded: value.succeeded,
            failed: value.failed,
            retry_attempts: value.retry_attempts,
            temp_audio_retained: value.temp_audio_retained,
            temp_audio_deleted: value.temp_audio_deleted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TraceSnapshot {
    trace_id: String,
    submission_errors: Vec<String>,
    completion_order: Vec<String>,
    results_by_job: Vec<String>,
    telemetry: TelemetryDigest,
}

#[derive(Default)]
struct GateState {
    first_call_blocked: bool,
    released: bool,
}

struct GatedTraceExecutor {
    fail_segments: HashSet<String>,
    gate: Arc<(Mutex<GateState>, Condvar)>,
}

impl GatedTraceExecutor {
    fn new(fail_segments: &[&str]) -> Self {
        Self {
            fail_segments: fail_segments
                .iter()
                .map(|value| value.to_string())
                .collect(),
            gate: Arc::new((Mutex::new(GateState::default()), Condvar::new())),
        }
    }

    fn wait_until_first_call_blocks(&self, timeout: Duration) {
        let (lock, notify) = &*self.gate;
        let mut state = lock.lock().expect("gate lock should not be poisoned");
        let start = Instant::now();
        while !state.first_call_blocked {
            let remaining = timeout
                .checked_sub(start.elapsed())
                .expect("executor did not block first transcribe call before timeout");
            let (next_state, _) = notify
                .wait_timeout(state, remaining)
                .expect("gate wait should not be poisoned");
            state = next_state;
        }
    }

    fn release(&self) {
        let (lock, notify) = &*self.gate;
        let mut state = lock.lock().expect("gate lock should not be poisoned");
        state.released = true;
        notify.notify_all();
    }
}

impl LiveAsrExecutor for GatedTraceExecutor {
    fn prewarm(&self) -> Result<(), String> {
        Ok(())
    }

    fn transcribe(&self, request: &LiveAsrRequest) -> Result<String, String> {
        let audio_path = request
            .audio_input
            .as_path()
            .ok_or_else(|| "parity executor expects path-backed request".to_string())?;
        let (lock, notify) = &*self.gate;
        let mut state = lock.lock().expect("gate lock should not be poisoned");
        if !state.first_call_blocked {
            state.first_call_blocked = true;
            notify.notify_all();
        }
        while !state.released {
            state = notify
                .wait(state)
                .expect("gate wait should not be poisoned");
        }
        drop(state);

        if self.fail_segments.contains(request.segment_id.as_str()) {
            Err(format!(
                "executor failure for segment={} path={}",
                request.segment_id,
                audio_path.display()
            ))
        } else {
            Ok(format!(
                "ok segment={} path={}",
                request.segment_id,
                audio_path.display()
            ))
        }
    }
}

fn class_label(class: LiveAsrJobClass) -> &'static str {
    match class {
        LiveAsrJobClass::Partial => "partial",
        LiveAsrJobClass::Final => "final",
        LiveAsrJobClass::Reconcile => "reconcile",
    }
}

fn build_job(trace_id: &str, job_id: usize, class: LiveAsrJobClass) -> LiveAsrJob {
    let dir = std::env::temp_dir()
        .join("recordit-live-asr-path-parity")
        .join(trace_id);
    fs::create_dir_all(&dir).expect("temp parity directory should be created");
    let audio_path = dir.join(format!("job-{job_id:03}.wav"));
    fs::write(&audio_path, b"parity").expect("temp parity wav should be created");
    LiveAsrJob {
        job_id,
        class,
        role: "mic",
        label: "mic".to_string(),
        segment_id: format!("seg-{job_id:03}"),
        audio_path,
        is_temp_audio: true,
    }
}

fn completion_row(job_id: usize, class: LiveAsrJobClass, success: bool, detail: &str) -> String {
    format!(
        "job={job_id} class={} status={} detail={detail}",
        class_label(class),
        if success { "ok" } else { "err" }
    )
}

fn run_trace(
    trace_id: &str,
    submit_mode: SubmitMode,
    jobs: &[LiveAsrJob],
    fail_segments: &[&str],
) -> TraceSnapshot {
    let executor = Arc::new(GatedTraceExecutor::new(fail_segments));
    let mut service = LiveAsrService::start(
        Arc::clone(&executor) as Arc<dyn LiveAsrExecutor>,
        LiveAsrPoolConfig {
            worker_count: 1,
            queue_capacity: 2,
            retries: 0,
            temp_audio_policy: TempAudioPolicy::DeleteAlways,
        },
    );
    let mut submission_errors = Vec::new();

    for (index, job) in jobs.iter().cloned().enumerate() {
        if let Some(parent) = job.audio_path.parent() {
            fs::create_dir_all(parent).expect("parity audio parent directory should exist");
        }
        fs::write(&job.audio_path, b"parity")
            .expect("parity audio fixture should be restored before submit");
        let result = match submit_mode {
            SubmitMode::LegacyPath => service.submit(job),
            SubmitMode::RequestPath => service.submit_request(job.into_request()),
        };
        if let Err(err) = result {
            submission_errors.push(format!("job={} err={err}", jobs[index].job_id));
        }
        if index == 0 {
            executor.wait_until_first_call_blocks(Duration::from_secs(1));
        }
    }

    executor.release();
    service.close();

    let mut results = Vec::with_capacity(jobs.len());
    let deadline = Instant::now() + Duration::from_secs(5);
    while results.len() < jobs.len() && Instant::now() < deadline {
        if let Some(result) = service.recv_result_timeout(Duration::from_millis(100)) {
            results.push(result);
        }
    }
    service.join();

    assert_eq!(
        results.len(),
        jobs.len(),
        "trace={trace_id} did not produce expected result count; expected={} actual={}",
        jobs.len(),
        results.len()
    );

    let completion_order = results
        .iter()
        .map(|result| {
            completion_row(
                result.job.job_id,
                result.job.class,
                result.success(),
                result
                    .error
                    .as_deref()
                    .unwrap_or_else(|| result.transcript_text.as_deref().unwrap_or_default()),
            )
        })
        .collect::<Vec<_>>();

    let mut by_job = results
        .into_iter()
        .map(|result| {
            (
                result.job.job_id,
                completion_row(
                    result.job.job_id,
                    result.job.class,
                    result.success(),
                    result
                        .error
                        .as_deref()
                        .unwrap_or_else(|| result.transcript_text.as_deref().unwrap_or_default()),
                ),
            )
        })
        .collect::<Vec<_>>();
    by_job.sort_by_key(|(job_id, _)| *job_id);

    TraceSnapshot {
        trace_id: trace_id.to_string(),
        submission_errors,
        completion_order,
        results_by_job: by_job.into_iter().map(|(_, row)| row).collect(),
        telemetry: service.telemetry().into(),
    }
}

fn telemetry_delta_report(legacy: &TelemetryDigest, request: &TelemetryDigest) -> String {
    let submitted = request.submitted as isize - legacy.submitted as isize;
    let enqueued = request.enqueued as isize - legacy.enqueued as isize;
    let dropped = request.dropped_queue_full as isize - legacy.dropped_queue_full as isize;
    let processed = request.processed as isize - legacy.processed as isize;
    let succeeded = request.succeeded as isize - legacy.succeeded as isize;
    let failed = request.failed as isize - legacy.failed as isize;
    let retries = request.retry_attempts as isize - legacy.retry_attempts as isize;
    let retained = request.temp_audio_retained as isize - legacy.temp_audio_retained as isize;
    let deleted = request.temp_audio_deleted as isize - legacy.temp_audio_deleted as isize;
    format!(
        "delta(request-legacy): submitted={submitted}, enqueued={enqueued}, dropped_queue_full={dropped}, processed={processed}, succeeded={succeeded}, failed={failed}, retry_attempts={retries}, temp_audio_retained={retained}, temp_audio_deleted={deleted}"
    )
}

fn assert_trace_parity(trace_id: &str, legacy: &TraceSnapshot, request: &TraceSnapshot) {
    assert_eq!(
        legacy,
        request,
        "trace parity mismatch for {trace_id}\nartifact=legacy_trace::{trace_id}::legacy\nartifact=request_trace::{trace_id}::request\n{}",
        telemetry_delta_report(&legacy.telemetry, &request.telemetry)
    );
}

fn assert_job_order(snapshot: &TraceSnapshot, first_job: usize, second_job: usize) {
    let first_key = format!("job={first_job} ");
    let second_key = format!("job={second_job} ");
    let first_pos = snapshot
        .completion_order
        .iter()
        .position(|row| row.contains(&first_key))
        .expect("expected first job in completion order");
    let second_pos = snapshot
        .completion_order
        .iter()
        .position(|row| row.contains(&second_key))
        .expect("expected second job in completion order");
    assert!(
        first_pos < second_pos,
        "trace={} expected job {} before job {} in completion order; order={:#?}",
        snapshot.trace_id,
        first_job,
        second_job,
        snapshot.completion_order
    );
}

#[test]
fn path_flow_parity_matches_request_path_mode_for_success_trace() {
    let trace_id = "success-trace";
    let jobs = vec![
        build_job(trace_id, 1, LiveAsrJobClass::Partial),
        build_job(trace_id, 2, LiveAsrJobClass::Final),
        build_job(trace_id, 3, LiveAsrJobClass::Reconcile),
    ];

    let legacy = run_trace(trace_id, SubmitMode::LegacyPath, &jobs, &[]);
    let request = run_trace(trace_id, SubmitMode::RequestPath, &jobs, &[]);
    assert_trace_parity(trace_id, &legacy, &request);
    assert!(legacy.submission_errors.is_empty());
    assert_job_order(&legacy, 2, 3);
}

#[test]
fn path_flow_parity_preserves_priority_and_telemetry_under_pressure_failures() {
    let trace_id = "pressure-failure-trace";
    let jobs = vec![
        build_job(trace_id, 1, LiveAsrJobClass::Partial),
        build_job(trace_id, 2, LiveAsrJobClass::Partial),
        build_job(trace_id, 3, LiveAsrJobClass::Reconcile),
        build_job(trace_id, 4, LiveAsrJobClass::Final),
        build_job(trace_id, 5, LiveAsrJobClass::Reconcile),
    ];

    let legacy = run_trace(trace_id, SubmitMode::LegacyPath, &jobs, &["seg-001"]);
    let request = run_trace(trace_id, SubmitMode::RequestPath, &jobs, &["seg-001"]);
    assert_trace_parity(trace_id, &legacy, &request);

    assert_eq!(legacy.telemetry.submitted, 5);
    assert_eq!(legacy.telemetry.enqueued, 4);
    assert_eq!(legacy.telemetry.dropped_queue_full, 2);
    assert_eq!(legacy.telemetry.processed, 3);
    assert_eq!(legacy.telemetry.succeeded, 2);
    assert_eq!(legacy.telemetry.failed, 3);
    assert_eq!(legacy.telemetry.temp_audio_deleted, 5);

    assert_job_order(&legacy, 4, 3);
}
