use recordit::live_asr_pool::{
    LiveAsrExecutor, LiveAsrJob, LiveAsrJobClass, LiveAsrJobResult, LiveAsrPoolConfig,
    LiveAsrRequest, LiveAsrService, TempAudioPolicy,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn class_label(class: LiveAsrJobClass) -> &'static str {
    match class {
        LiveAsrJobClass::Partial => "partial",
        LiveAsrJobClass::Final => "final",
        LiveAsrJobClass::Reconcile => "reconcile",
    }
}

#[derive(Default)]
struct GateState {
    first_call_blocked: bool,
    released: bool,
}

struct DeterministicFaultExecutor {
    gate_first_call: bool,
    gate: Arc<(Mutex<GateState>, Condvar)>,
    failure_budgets: Mutex<HashMap<String, usize>>,
    attempts: Mutex<HashMap<String, usize>>,
    invocations: Mutex<Vec<String>>,
}

impl DeterministicFaultExecutor {
    fn new(gate_first_call: bool, failure_budgets: &[(&str, usize)]) -> Self {
        let budgets = failure_budgets
            .iter()
            .map(|(segment, count)| (segment.to_string(), *count))
            .collect();
        Self {
            gate_first_call,
            gate: Arc::new((Mutex::new(GateState::default()), Condvar::new())),
            failure_budgets: Mutex::new(budgets),
            attempts: Mutex::new(HashMap::new()),
            invocations: Mutex::new(Vec::new()),
        }
    }

    fn wait_until_first_call_blocks(&self, timeout: Duration) {
        if !self.gate_first_call {
            return;
        }
        let (lock, notify) = &*self.gate;
        let mut state = lock.lock().expect("gate lock should not be poisoned");
        let started = Instant::now();
        while !state.first_call_blocked {
            let remaining = timeout.checked_sub(started.elapsed()).expect(
                "fault executor did not block first transcribe call before timeout elapsed",
            );
            let (next_state, _) = notify
                .wait_timeout(state, remaining)
                .expect("gate wait should not be poisoned");
            state = next_state;
        }
    }

    fn release_gate(&self) {
        if !self.gate_first_call {
            return;
        }
        let (lock, notify) = &*self.gate;
        let mut state = lock.lock().expect("gate lock should not be poisoned");
        state.released = true;
        notify.notify_all();
    }

    fn invocation_rows(&self) -> Vec<String> {
        self.invocations
            .lock()
            .expect("invocations lock should not be poisoned")
            .clone()
    }

    fn attempts_for(&self, segment_id: &str) -> usize {
        *self
            .attempts
            .lock()
            .expect("attempts lock should not be poisoned")
            .get(segment_id)
            .unwrap_or(&0)
    }
}

impl LiveAsrExecutor for DeterministicFaultExecutor {
    fn prewarm(&self) -> Result<(), String> {
        Ok(())
    }

    fn transcribe(&self, request: &LiveAsrRequest) -> Result<String, String> {
        let path = request
            .audio_input
            .as_path()
            .map(|value| value.display().to_string())
            .unwrap_or_else(|| "<pcm-window>".to_string());

        let mut attempts = self
            .attempts
            .lock()
            .expect("attempts lock should not be poisoned");
        let attempt = attempts
            .entry(request.segment_id.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);
        let attempt = *attempt;
        drop(attempts);

        self.invocations
            .lock()
            .expect("invocations lock should not be poisoned")
            .push(format!(
                "segment={} class={} attempt={} path={}",
                request.segment_id,
                class_label(request.class),
                attempt,
                path
            ));

        if self.gate_first_call {
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
        }

        let mut failure_budgets = self
            .failure_budgets
            .lock()
            .expect("failure budget lock should not be poisoned");
        if let Some(remaining) = failure_budgets.get_mut(&request.segment_id) {
            if *remaining > 0 {
                *remaining -= 1;
                return Err(format!(
                    "injected failure segment={} attempt={}",
                    request.segment_id, attempt
                ));
            }
        }

        Ok(format!(
            "ok segment={} class={} attempt={}",
            request.segment_id,
            class_label(request.class),
            attempt
        ))
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
    fs::create_dir_all(&dir).expect("temp test directory should be created");
    dir
}

fn write_temp_audio_file(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"fault-injection").expect("temp audio fixture should be created");
    path
}

fn build_job(job_id: usize, class: LiveAsrJobClass, path: PathBuf) -> LiveAsrJob {
    LiveAsrJob {
        job_id,
        class,
        role: "mic",
        label: "mic".to_string(),
        segment_id: format!("seg-{job_id:03}"),
        audio_path: path,
        is_temp_audio: true,
    }
}

fn collect_results(
    service: &mut LiveAsrService,
    expected: usize,
    timeout: Duration,
    scenario: &str,
) -> Vec<LiveAsrJobResult> {
    let deadline = Instant::now() + timeout;
    let mut results = Vec::with_capacity(expected);
    while results.len() < expected && Instant::now() < deadline {
        if let Some(result) = service.recv_result_timeout(Duration::from_millis(100)) {
            results.push(result);
        }
    }
    assert_eq!(
        results.len(),
        expected,
        "scenario={scenario} expected {expected} results but got {}",
        results.len()
    );
    results
}

fn render_result_rows(results: &[LiveAsrJobResult]) -> Vec<String> {
    let mut rows = results
        .iter()
        .map(|result| {
            format!(
                "job={} class={} status={} retries={} retained={} deleted={} detail={}",
                result.job.job_id,
                class_label(result.job.class),
                if result.success() { "ok" } else { "err" },
                result.retry_attempts,
                result.temp_audio_retained,
                result.temp_audio_deleted,
                result
                    .error
                    .as_deref()
                    .unwrap_or_else(|| result.transcript_text.as_deref().unwrap_or("<empty>"))
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

#[test]
fn fault_injection_queue_pressure_preserves_final_priority_and_breadcrumbs() {
    let scenario = "queue-pressure-priority";
    let dir = unique_temp_dir("recordit-fault-injection-queue-priority");
    let executor = Arc::new(DeterministicFaultExecutor::new(true, &[]));
    let mut service = LiveAsrService::start(
        Arc::clone(&executor) as Arc<dyn LiveAsrExecutor>,
        LiveAsrPoolConfig {
            worker_count: 1,
            queue_capacity: 2,
            retries: 0,
            temp_audio_policy: TempAudioPolicy::DeleteAlways,
        },
    );

    let job1 = build_job(
        1,
        LiveAsrJobClass::Partial,
        write_temp_audio_file(&dir, "job-001-partial.wav"),
    );
    let job2 = build_job(
        2,
        LiveAsrJobClass::Partial,
        write_temp_audio_file(&dir, "job-002-partial.wav"),
    );
    let job3 = build_job(
        3,
        LiveAsrJobClass::Reconcile,
        write_temp_audio_file(&dir, "job-003-reconcile.wav"),
    );
    let job4 = build_job(
        4,
        LiveAsrJobClass::Final,
        write_temp_audio_file(&dir, "job-004-final.wav"),
    );

    assert!(service.submit(job1).is_ok());
    executor.wait_until_first_call_blocks(Duration::from_secs(2));
    assert!(service.submit(job2).is_ok());
    assert!(service.submit(job3).is_ok());
    assert!(service.submit(job4).is_ok());

    executor.release_gate();
    service.close();
    let results = collect_results(&mut service, 4, Duration::from_secs(5), scenario);
    service.join();

    let rows = render_result_rows(&results);
    let invocations = executor.invocation_rows();
    let telemetry = service.telemetry();
    let mut by_job = HashMap::new();
    for result in results {
        by_job.insert(result.job.job_id, result);
    }

    assert!(
        by_job.get(&1).expect("job 1 result should exist").success(),
        "scenario={scenario} expected initial partial to complete successfully; rows={rows:#?}; invocations={invocations:#?}"
    );
    assert!(
        by_job.get(&4).expect("job 4 result should exist").success(),
        "scenario={scenario} expected final job to survive queue pressure; rows={rows:#?}; invocations={invocations:#?}"
    );
    assert!(
        !by_job.get(&2).expect("job 2 result should exist").success(),
        "scenario={scenario} expected second partial to be evicted under pressure; rows={rows:#?}; invocations={invocations:#?}"
    );
    assert!(
        by_job.get(&3).expect("job 3 result should exist").success(),
        "scenario={scenario} expected reconcile to survive while final evicts partial first; rows={rows:#?}; invocations={invocations:#?}"
    );

    assert!(
        invocations.iter().all(|row| !row.contains("seg-002")),
        "scenario={scenario} queue-pressure eviction should prevent seg-002 execution; invocations={invocations:#?}"
    );
    let final_pos = invocations
        .iter()
        .position(|row| row.contains("seg-004"))
        .expect("final invocation should exist");
    let reconcile_pos = invocations
        .iter()
        .position(|row| row.contains("seg-003"))
        .expect("reconcile invocation should exist");
    assert!(
        final_pos < reconcile_pos,
        "scenario={scenario} final should execute before reconcile under pressure; invocations={invocations:#?}"
    );

    assert_eq!(
        telemetry.dropped_queue_full, 1,
        "scenario={scenario} expected one queue-pressure drop (partial eviction); rows={rows:#?}; invocations={invocations:#?}"
    );
    assert_eq!(
        telemetry.processed, 3,
        "scenario={scenario} only surviving jobs should be processed by workers; rows={rows:#?}; invocations={invocations:#?}"
    );
    assert_eq!(telemetry.failed, 1);
    assert_eq!(telemetry.succeeded, 3);
    assert_eq!(telemetry.temp_audio_deleted, 4);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn fault_injection_retries_are_bounded_and_visible_in_telemetry() {
    let scenario = "retry-bounds";
    let dir = unique_temp_dir("recordit-fault-injection-retry");
    let executor = Arc::new(DeterministicFaultExecutor::new(
        false,
        &[("seg-011", 2), ("seg-012", 3)],
    ));
    let mut service = LiveAsrService::start(
        Arc::clone(&executor) as Arc<dyn LiveAsrExecutor>,
        LiveAsrPoolConfig {
            worker_count: 1,
            queue_capacity: 4,
            retries: 2,
            temp_audio_policy: TempAudioPolicy::DeleteAlways,
        },
    );

    let retry_then_success = build_job(
        11,
        LiveAsrJobClass::Final,
        write_temp_audio_file(&dir, "job-011-final.wav"),
    );
    let hard_fail = build_job(
        12,
        LiveAsrJobClass::Reconcile,
        write_temp_audio_file(&dir, "job-012-reconcile.wav"),
    );
    assert!(service.submit(retry_then_success).is_ok());
    assert!(service.submit(hard_fail).is_ok());
    service.close();

    let results = collect_results(&mut service, 2, Duration::from_secs(5), scenario);
    service.join();

    let telemetry = service.telemetry();
    let rows = render_result_rows(&results);
    let invocations = executor.invocation_rows();
    let mut by_job = HashMap::new();
    for result in results {
        by_job.insert(result.job.job_id, result);
    }
    let success = by_job.get(&11).expect("job 11 result should exist");
    let fail = by_job.get(&12).expect("job 12 result should exist");

    assert!(
        success.success(),
        "scenario={scenario} expected seg-011 to recover after retries; rows={rows:#?}; invocations={invocations:#?}"
    );
    assert_eq!(
        success.retry_attempts, 2,
        "scenario={scenario} seg-011 should report two retries before success; rows={rows:#?}; invocations={invocations:#?}"
    );
    assert!(
        !fail.success(),
        "scenario={scenario} expected seg-012 to fail after exhausting retries; rows={rows:#?}; invocations={invocations:#?}"
    );
    assert_eq!(
        fail.retry_attempts, 2,
        "scenario={scenario} seg-012 should stop after configured retry budget; rows={rows:#?}; invocations={invocations:#?}"
    );
    assert_eq!(executor.attempts_for("seg-011"), 3);
    assert_eq!(executor.attempts_for("seg-012"), 3);
    assert_eq!(telemetry.retry_attempts, 4);
    assert_eq!(telemetry.succeeded, 1);
    assert_eq!(telemetry.failed, 1);
    assert_eq!(telemetry.temp_audio_deleted, 2);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn fault_injection_temp_audio_safety_paths_are_retained_for_triage() {
    let scenario = "temp-audio-safety";
    let dir = unique_temp_dir("recordit-fault-injection-temp-audio");
    let executor = Arc::new(DeterministicFaultExecutor::new(false, &[]));
    let mut service = LiveAsrService::start(
        Arc::clone(&executor) as Arc<dyn LiveAsrExecutor>,
        LiveAsrPoolConfig {
            worker_count: 1,
            queue_capacity: 4,
            retries: 0,
            temp_audio_policy: TempAudioPolicy::DeleteAlways,
        },
    );

    let directory_path = dir.join("non-file-temp-audio");
    fs::create_dir_all(&directory_path).expect("non-file temp-audio directory should exist");
    let directory_job = build_job(21, LiveAsrJobClass::Final, directory_path.clone());

    #[cfg(unix)]
    let (symlink_job, symlink_path, symlink_target) = {
        use std::os::unix::fs::symlink;
        let target = write_temp_audio_file(&dir, "symlink-target.wav");
        let link = dir.join("temp-symlink.wav");
        symlink(&target, &link).expect("symlink temp-audio path should be created");
        (
            Some(build_job(22, LiveAsrJobClass::Reconcile, link.clone())),
            Some(link),
            Some(target),
        )
    };
    #[cfg(not(unix))]
    let (symlink_job, symlink_path, symlink_target): (
        Option<LiveAsrJob>,
        Option<PathBuf>,
        Option<PathBuf>,
    ) = (None, None, None);

    assert!(service.submit(directory_job).is_ok());
    if let Some(job) = symlink_job {
        assert!(service.submit(job).is_ok());
    }
    service.close();

    let expected_results = if symlink_path.is_some() { 2 } else { 1 };
    let results = collect_results(
        &mut service,
        expected_results,
        Duration::from_secs(5),
        scenario,
    );
    service.join();

    let telemetry = service.telemetry();
    let rows = render_result_rows(&results);
    let mut by_job = HashMap::new();
    for result in results {
        by_job.insert(result.job.job_id, result);
    }
    let directory_result = by_job.get(&21).expect("directory job result should exist");
    assert!(
        directory_result.success(),
        "scenario={scenario} expected directory path case to complete; rows={rows:#?}"
    );
    assert!(
        directory_result.temp_audio_retained && !directory_result.temp_audio_deleted,
        "scenario={scenario} expected non-file temp path to be retained for manual review; rows={rows:#?}"
    );
    assert!(
        directory_path.is_dir(),
        "scenario={scenario} expected directory path to remain for triage: {}",
        directory_path.display()
    );

    if let Some(symlink_result) = by_job.get(&22) {
        assert!(
            symlink_result.success(),
            "scenario={scenario} expected symlink case to complete; rows={rows:#?}"
        );
        assert!(
            symlink_result.temp_audio_retained && !symlink_result.temp_audio_deleted,
            "scenario={scenario} expected symlink temp path to be retained for safe review; rows={rows:#?}"
        );
    }
    if let Some(link) = symlink_path {
        assert!(
            fs::symlink_metadata(&link)
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(false),
            "scenario={scenario} expected symlink temp path to remain: {}",
            link.display()
        );
    }
    if let Some(target) = symlink_target {
        assert!(
            target.is_file(),
            "scenario={scenario} expected symlink target to remain: {}",
            target.display()
        );
    }

    assert_eq!(
        telemetry.temp_audio_retained, expected_results,
        "scenario={scenario} retained-temp counter drift; rows={rows:#?}"
    );
    assert_eq!(
        telemetry.temp_audio_deleted, 0,
        "scenario={scenario} delete counter should remain zero for safety paths; rows={rows:#?}"
    );

    let _ = fs::remove_dir_all(dir);
}
