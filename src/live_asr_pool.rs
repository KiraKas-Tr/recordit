use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, sync_channel, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveAsrJobClass {
    Partial,
    Final,
    Reconcile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempAudioPolicy {
    DeleteAlways,
    RetainOnFailure,
    RetainAlways,
}

#[derive(Debug, Clone)]
pub struct LiveAsrPoolConfig {
    pub worker_count: usize,
    pub queue_capacity: usize,
    pub retries: usize,
    pub temp_audio_policy: TempAudioPolicy,
}

impl Default for LiveAsrPoolConfig {
    fn default() -> Self {
        Self {
            worker_count: 2,
            queue_capacity: 8,
            retries: 0,
            temp_audio_policy: TempAudioPolicy::RetainOnFailure,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveAsrJob {
    pub job_id: usize,
    pub class: LiveAsrJobClass,
    pub role: &'static str,
    pub label: String,
    pub segment_id: String,
    pub audio_path: PathBuf,
    pub is_temp_audio: bool,
}

#[derive(Debug, Clone)]
pub struct LiveAsrJobResult {
    pub job: LiveAsrJob,
    pub transcript_text: Option<String>,
    pub error: Option<String>,
    pub retry_attempts: usize,
    pub temp_audio_retained: bool,
    pub temp_audio_deleted: bool,
}

impl LiveAsrJobResult {
    pub fn success(&self) -> bool {
        self.error.is_none()
    }
}

#[derive(Debug, Clone, Default)]
pub struct LiveAsrPoolTelemetry {
    pub prewarm_ok: bool,
    pub submitted: usize,
    pub enqueued: usize,
    pub dropped_queue_full: usize,
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub retry_attempts: usize,
    pub temp_audio_retained: usize,
    pub temp_audio_deleted: usize,
}

pub trait LiveAsrExecutor: Send + Sync + 'static {
    fn prewarm(&self) -> Result<(), String>;
    fn transcribe(&self, audio_path: &Path) -> Result<String, String>;
}

pub fn run_live_asr_pool(
    executor: Arc<dyn LiveAsrExecutor>,
    jobs: Vec<LiveAsrJob>,
    config: LiveAsrPoolConfig,
) -> (Vec<LiveAsrJobResult>, LiveAsrPoolTelemetry) {
    let mut telemetry = LiveAsrPoolTelemetry {
        submitted: jobs.len(),
        ..LiveAsrPoolTelemetry::default()
    };

    if let Err(err) = executor.prewarm() {
        telemetry.prewarm_ok = false;
        telemetry.failed = jobs.len();
        let results = jobs
            .into_iter()
            .map(|job| LiveAsrJobResult {
                job,
                transcript_text: None,
                error: Some(format!("asr prewarm failed: {err}")),
                retry_attempts: 0,
                temp_audio_retained: true,
                temp_audio_deleted: false,
            })
            .collect();
        return (results, telemetry);
    }
    telemetry.prewarm_ok = true;

    let worker_count = config.worker_count.max(1);
    let queue_capacity = config.queue_capacity.max(1);
    let (job_tx, job_rx) = sync_channel::<LiveAsrJob>(queue_capacity);
    let (result_tx, result_rx) = mpsc::channel::<LiveAsrJobResult>();

    let shared_rx = Arc::new(Mutex::new(job_rx));
    let mut handles = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let rx = Arc::clone(&shared_rx);
        let tx = result_tx.clone();
        let exec = Arc::clone(&executor);
        let policy = config.temp_audio_policy;
        let retries = config.retries;
        handles.push(thread::spawn(move || loop {
            let maybe_job = {
                let guard = rx.lock().ok();
                guard.and_then(|locked| locked.recv().ok())
            };
            let Some(job) = maybe_job else {
                break;
            };

            let mut attempts = 0usize;
            let (transcript, error) = loop {
                match exec.transcribe(&job.audio_path) {
                    Ok(text) => break (Some(text), None),
                    Err(err) => {
                        if attempts >= retries {
                            break (None, Some(err));
                        }
                        attempts += 1;
                    }
                }
            };

            let success = error.is_none();
            let (retained, deleted) =
                finalize_temp_audio_path(&job.audio_path, job.is_temp_audio, success, policy);
            let _ = tx.send(LiveAsrJobResult {
                job,
                transcript_text: transcript,
                error,
                retry_attempts: attempts,
                temp_audio_retained: retained,
                temp_audio_deleted: deleted,
            });
        }));
    }
    drop(result_tx);

    let mut results = Vec::with_capacity(jobs.len());
    for job in jobs {
        match job_tx.try_send(job) {
            Ok(()) => telemetry.enqueued += 1,
            Err(TrySendError::Full(job)) => {
                telemetry.dropped_queue_full += 1;
                telemetry.failed += 1;
                results.push(LiveAsrJobResult {
                    job,
                    transcript_text: None,
                    error: Some("asr queue full; dropped non-blocking submission".to_string()),
                    retry_attempts: 0,
                    temp_audio_retained: true,
                    temp_audio_deleted: false,
                });
            }
            Err(TrySendError::Disconnected(job)) => {
                telemetry.failed += 1;
                results.push(LiveAsrJobResult {
                    job,
                    transcript_text: None,
                    error: Some("asr queue disconnected".to_string()),
                    retry_attempts: 0,
                    temp_audio_retained: true,
                    temp_audio_deleted: false,
                });
            }
        }
    }
    drop(job_tx);

    for _ in 0..telemetry.enqueued {
        if let Ok(result) = result_rx.recv() {
            telemetry.processed += 1;
            telemetry.retry_attempts += result.retry_attempts;
            if result.success() {
                telemetry.succeeded += 1;
            } else {
                telemetry.failed += 1;
            }
            if result.temp_audio_retained {
                telemetry.temp_audio_retained += 1;
            }
            if result.temp_audio_deleted {
                telemetry.temp_audio_deleted += 1;
            }
            results.push(result);
        }
    }

    for handle in handles {
        let _ = handle.join();
    }

    results.sort_by_key(|result| result.job.job_id);
    (results, telemetry)
}

fn finalize_temp_audio_path(
    path: &Path,
    is_temp_audio: bool,
    success: bool,
    policy: TempAudioPolicy,
) -> (bool, bool) {
    if !is_temp_audio {
        return (false, false);
    }

    let retain = match policy {
        TempAudioPolicy::DeleteAlways => false,
        TempAudioPolicy::RetainOnFailure => !success,
        TempAudioPolicy::RetainAlways => true,
    };
    if retain {
        return (true, false);
    }

    match fs::remove_file(path) {
        Ok(()) => (false, true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (false, false),
        Err(_) => (true, false),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        run_live_asr_pool, LiveAsrExecutor, LiveAsrJob, LiveAsrJobClass, LiveAsrPoolConfig,
        TempAudioPolicy,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    struct MockExecutor {
        prewarm_ok: bool,
        fail_text: bool,
        sleep_ms: u64,
        attempts: AtomicUsize,
    }

    impl LiveAsrExecutor for MockExecutor {
        fn prewarm(&self) -> Result<(), String> {
            if self.prewarm_ok {
                Ok(())
            } else {
                Err("mock prewarm failure".to_string())
            }
        }

        fn transcribe(&self, audio_path: &Path) -> Result<String, String> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            if self.sleep_ms > 0 {
                thread::sleep(Duration::from_millis(self.sleep_ms));
            }
            if self.fail_text {
                Err(format!("failed: {}", audio_path.display()))
            } else {
                Ok(format!("ok:{}", audio_path.display()))
            }
        }
    }

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("recordit-live-asr-pool-tests");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(name);
        let _ = fs::write(&path, b"tmp");
        path
    }

    #[test]
    fn queue_stays_non_blocking_and_drops_on_full_capacity() {
        let executor = Arc::new(MockExecutor {
            prewarm_ok: true,
            fail_text: false,
            sleep_ms: 30,
            attempts: AtomicUsize::new(0),
        });
        let jobs = (0..6)
            .map(|idx| LiveAsrJob {
                job_id: idx,
                class: LiveAsrJobClass::Final,
                role: "mic",
                label: "mic".to_string(),
                segment_id: format!("s-{idx}"),
                audio_path: temp_file(&format!("queue-{idx}.wav")),
                is_temp_audio: true,
            })
            .collect::<Vec<_>>();

        let (results, telemetry) = run_live_asr_pool(
            executor,
            jobs,
            LiveAsrPoolConfig {
                worker_count: 1,
                queue_capacity: 1,
                retries: 0,
                temp_audio_policy: TempAudioPolicy::RetainOnFailure,
            },
        );

        assert_eq!(telemetry.submitted, 6);
        assert!(telemetry.dropped_queue_full > 0);
        assert_eq!(results.len(), 6);
    }

    #[test]
    fn delete_always_policy_removes_temp_audio_on_success() {
        let executor = Arc::new(MockExecutor {
            prewarm_ok: true,
            fail_text: false,
            sleep_ms: 0,
            attempts: AtomicUsize::new(0),
        });
        let tmp = temp_file("delete-success.wav");
        let (results, telemetry) = run_live_asr_pool(
            executor,
            vec![LiveAsrJob {
                job_id: 1,
                class: LiveAsrJobClass::Final,
                role: "mic",
                label: "mic".to_string(),
                segment_id: "s1".to_string(),
                audio_path: tmp.clone(),
                is_temp_audio: true,
            }],
            LiveAsrPoolConfig {
                worker_count: 1,
                queue_capacity: 2,
                retries: 0,
                temp_audio_policy: TempAudioPolicy::DeleteAlways,
            },
        );

        assert_eq!(telemetry.temp_audio_deleted, 1);
        assert!(results[0].success());
        assert!(!tmp.exists());
    }

    #[test]
    fn retain_on_failure_keeps_temp_audio_for_debugging() {
        let executor = Arc::new(MockExecutor {
            prewarm_ok: true,
            fail_text: true,
            sleep_ms: 0,
            attempts: AtomicUsize::new(0),
        });
        let tmp = temp_file("retain-failure.wav");
        let (results, telemetry) = run_live_asr_pool(
            executor,
            vec![LiveAsrJob {
                job_id: 1,
                class: LiveAsrJobClass::Final,
                role: "mic",
                label: "mic".to_string(),
                segment_id: "s1".to_string(),
                audio_path: tmp.clone(),
                is_temp_audio: true,
            }],
            LiveAsrPoolConfig {
                worker_count: 1,
                queue_capacity: 2,
                retries: 0,
                temp_audio_policy: TempAudioPolicy::RetainOnFailure,
            },
        );

        assert_eq!(telemetry.failed, 1);
        assert_eq!(telemetry.temp_audio_retained, 1);
        assert!(!results[0].success());
        assert!(tmp.exists());
        let _ = fs::remove_file(tmp);
    }

    #[test]
    fn prewarm_failure_short_circuits_jobs() {
        let executor = Arc::new(MockExecutor {
            prewarm_ok: false,
            fail_text: false,
            sleep_ms: 0,
            attempts: AtomicUsize::new(0),
        });
        let (results, telemetry) = run_live_asr_pool(
            executor,
            vec![LiveAsrJob {
                job_id: 1,
                class: LiveAsrJobClass::Final,
                role: "mic",
                label: "mic".to_string(),
                segment_id: "s1".to_string(),
                audio_path: temp_file("prewarm-failure.wav"),
                is_temp_audio: true,
            }],
            LiveAsrPoolConfig::default(),
        );

        assert!(!telemetry.prewarm_ok);
        assert_eq!(telemetry.failed, 1);
        assert!(results[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("prewarm failed"));
    }
}
