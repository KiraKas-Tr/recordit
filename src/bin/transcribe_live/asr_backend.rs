use super::*;
use recordit::live_asr_pool::{LiveAsrAudioInput, TempAudioPolicy};
use std::cell::RefCell;

thread_local! {
    static PCM_SCRATCH_CONTEXT: RefCell<Option<PcmScratchContext>> = const { RefCell::new(None) };
}

#[derive(Debug)]
struct PcmScratchContext {
    path: PathBuf,
    temp_audio_policy: TempAudioPolicy,
    sample_buffer: Vec<f32>,
    sticky_retain_for_review: bool,
    policy_retain_for_review: bool,
}

impl PcmScratchContext {
    fn new(temp_audio_policy: TempAudioPolicy) -> Result<Self, CliError> {
        let scratch_dir = env::temp_dir()
            .join("recordit-live-asr-pcm")
            .join(format!("pid-{}", std::process::id()));
        fs::create_dir_all(&scratch_dir).map_err(|err| {
            CliError::new(format!(
                "failed to create live ASR PCM scratch dir {}: {err}",
                display_path(&scratch_dir)
            ))
        })?;
        Ok(Self {
            path: scratch_dir.join(format!("worker-{}.wav", current_thread_token())),
            temp_audio_policy,
            sample_buffer: Vec::new(),
            sticky_retain_for_review: false,
            policy_retain_for_review: matches!(temp_audio_policy, TempAudioPolicy::RetainAlways),
        })
    }

    fn materialize(&mut self, request: &LiveAsrRequest) -> Result<PathBuf, CliError> {
        let LiveAsrAudioInput::PcmWindow {
            sample_rate_hz,
            mono_samples,
            ..
        } = &request.audio_input
        else {
            return Err(CliError::new(format!(
                "worker scratch materialization requires pcm_window input for segment `{}`",
                request.segment_id
            )));
        };

        if let Err(err) = validate_pcm_scratch_target(&self.path) {
            self.sticky_retain_for_review = true;
            return Err(err);
        }
        self.sample_buffer.clear();
        self.sample_buffer.extend_from_slice(mono_samples);
        if self.sample_buffer.is_empty() {
            self.sample_buffer.push(0.0);
        }

        if let Err(err) =
            write_runtime_job_wav(&self.path, (*sample_rate_hz).max(1), &self.sample_buffer)
        {
            self.sticky_retain_for_review = true;
            return Err(err);
        }
        Ok(self.path.clone())
    }

    fn record_outcome(&mut self, success: bool) {
        self.policy_retain_for_review = match self.temp_audio_policy {
            TempAudioPolicy::DeleteAlways => false,
            TempAudioPolicy::RetainOnFailure => !success,
            TempAudioPolicy::RetainAlways => true,
        };
    }
}

impl Drop for PcmScratchContext {
    fn drop(&mut self) {
        if self.sticky_retain_for_review || self.policy_retain_for_review {
            return;
        }
        safe_delete_pcm_scratch_path(&self.path);
    }
}

struct AsrRequest<'a> {
    model_path: &'a Path,
    live_request: &'a LiveAsrRequest,
    audio_path_override: Option<&'a Path>,
    language: &'a str,
    threads: usize,
}

impl AsrRequest<'_> {
    fn audio_path(&self) -> Result<&Path, CliError> {
        self.audio_path_override
            .or_else(|| self.live_request.audio_input.as_path())
            .ok_or_else(|| {
                CliError::new(format!(
                    "backend adapter requires path-backed audio input for segment `{}`",
                    self.live_request.segment_id
                ))
            })
    }
}

trait AsrAdapter {
    fn transcribe(&self, request: &AsrRequest<'_>) -> Result<String, CliError>;
}

fn backend_helper_program_label(backend: AsrBackend) -> &'static str {
    match backend {
        AsrBackend::WhisperCpp => "whisper-cli",
        AsrBackend::WhisperKit => "whisperkit-cli",
        AsrBackend::Moonshine => "moonshine",
    }
}

fn backend_helper_env_var(backend: AsrBackend) -> Option<&'static str> {
    match backend {
        AsrBackend::WhisperCpp => Some("RECORDIT_WHISPERCPP_CLI_PATH"),
        AsrBackend::WhisperKit => Some("RECORDIT_WHISPERKIT_CLI_PATH"),
        AsrBackend::Moonshine => None,
    }
}

pub(super) fn bundled_backend_program_from_exe(
    backend: AsrBackend,
    current_exe: &Path,
) -> Option<String> {
    let helper_name = backend_helper_program_label(backend);
    let macos_dir = current_exe.parent()?;
    let contents_dir = macos_dir.parent()?;
    let candidates = [
        contents_dir.join("Resources").join("bin").join(helper_name),
        contents_dir.join("Helpers").join(helper_name),
    ];

    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn resolve_bundled_backend_program(backend: AsrBackend) -> Option<String> {
    let current_exe = env::current_exe().ok()?;
    bundled_backend_program_from_exe(backend, &current_exe)
}

pub(super) fn resolve_backend_program(backend: AsrBackend, model_path: &Path) -> String {
    if let Some(env_name) = backend_helper_env_var(backend) {
        if let Ok(value) = env::var(env_name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    if let Some(program) = resolve_bundled_backend_program(backend) {
        return program;
    }

    let helper_name = backend_helper_program_label(backend);
    if let Some(parent) = model_path.parent() {
        let sibling = parent.join(helper_name);
        if sibling.is_file() {
            return sibling.to_string_lossy().to_string();
        }

        let nested = parent.join("bin").join(helper_name);
        if nested.is_file() {
            return nested.to_string_lossy().to_string();
        }
    }

    helper_name.to_string()
}

struct WhisperCppAdapter {
    program: String,
}

impl AsrAdapter for WhisperCppAdapter {
    fn transcribe(&self, request: &AsrRequest<'_>) -> Result<String, CliError> {
        let audio_path = request.audio_path()?;
        let output = Command::new(&self.program)
            .args([
                "-m",
                &request.model_path.to_string_lossy(),
                "-f",
                &audio_path.to_string_lossy(),
                "-l",
                request.language,
                "-t",
                &request.threads.to_string(),
                "-nt",
                "-np",
            ])
            .output()
            .map_err(|err| {
                CliError::new(format!(
                    "failed to execute `{}`: {err}",
                    backend_helper_program_label(AsrBackend::WhisperCpp)
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CliError::new(format!(
                "`whisper-cli` exited with status {}: {}",
                output.status,
                clean_field(stderr.trim())
            )));
        }

        let transcript = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if transcript.is_empty() {
            return Ok("<no speech detected>".to_string());
        }
        Ok(transcript)
    }
}

struct WhisperKitAdapter {
    program: String,
}

impl AsrAdapter for WhisperKitAdapter {
    fn transcribe(&self, request: &AsrRequest<'_>) -> Result<String, CliError> {
        let audio_path = request.audio_path()?;
        let output = Command::new(&self.program)
            .args([
                "transcribe",
                "--audio-path",
                &audio_path.to_string_lossy(),
                "--model-path",
                &request.model_path.to_string_lossy(),
                "--language",
                request.language,
                "--task",
                "transcribe",
                "--without-timestamps",
            ])
            .output()
            .map_err(|err| {
                CliError::new(format!(
                    "failed to execute `{}`: {err}",
                    backend_helper_program_label(AsrBackend::WhisperKit)
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CliError::new(format!(
                "`whisperkit-cli` exited with status {}: {}",
                output.status,
                clean_field(stderr.trim())
            )));
        }

        let transcript = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if transcript.is_empty() {
            return Ok("<no speech detected>".to_string());
        }
        Ok(transcript)
    }
}

fn select_adapter(backend: AsrBackend, program: String) -> Result<Box<dyn AsrAdapter>, CliError> {
    match backend {
        AsrBackend::WhisperCpp => Ok(Box::new(WhisperCppAdapter { program })),
        AsrBackend::WhisperKit => Ok(Box::new(WhisperKitAdapter { program })),
        AsrBackend::Moonshine => Err(CliError::new(
            "moonshine adapter is not wired in this phase; use `--asr-backend whispercpp` or `--asr-backend whisperkit`",
        )),
    }
}

pub(super) struct PooledAsrExecutor {
    pub(super) backend: AsrBackend,
    pub(super) helper_program: String,
    pub(super) model_path: PathBuf,
    pub(super) language: String,
    pub(super) threads: usize,
    pub(super) temp_audio_policy: TempAudioPolicy,
    pub(super) prewarm_enabled: bool,
}

impl LiveAsrExecutor for PooledAsrExecutor {
    fn prewarm(&self) -> Result<(), String> {
        if !self.prewarm_enabled {
            return Ok(());
        }
        let _ = select_adapter(self.backend, self.helper_program.clone())
            .map_err(|err| err.to_string())?;
        prewarm_backend_binary(self.backend, &self.helper_program)
    }

    fn transcribe(&self, request: &LiveAsrRequest) -> Result<String, String> {
        let adapter = select_adapter(self.backend, self.helper_program.clone())
            .map_err(|err| err.to_string())?;
        let scratch_audio = materialize_pcm_request_audio(request, self.temp_audio_policy)
            .map_err(|err| err.to_string())?;
        let result = adapter.transcribe(&AsrRequest {
            model_path: &self.model_path,
            live_request: request,
            audio_path_override: scratch_audio.as_deref(),
            language: &self.language,
            threads: self.threads,
        });
        if scratch_audio.is_some() {
            record_pcm_request_outcome(result.is_ok());
        }
        result.map_err(|err| err.to_string())
    }
}

fn materialize_pcm_request_audio(
    request: &LiveAsrRequest,
    temp_audio_policy: TempAudioPolicy,
) -> Result<Option<PathBuf>, CliError> {
    if !matches!(request.audio_input, LiveAsrAudioInput::PcmWindow { .. }) {
        return Ok(None);
    }

    PCM_SCRATCH_CONTEXT.with(|cell| {
        let mut context = cell.borrow_mut();
        if context
            .as_ref()
            .map(|ctx| ctx.temp_audio_policy != temp_audio_policy)
            .unwrap_or(true)
        {
            *context = Some(PcmScratchContext::new(temp_audio_policy)?);
        }
        let context = context
            .as_mut()
            .expect("pcm scratch context should exist after initialization");
        context.materialize(request).map(Some)
    })
}

fn record_pcm_request_outcome(success: bool) {
    PCM_SCRATCH_CONTEXT.with(|cell| {
        if let Some(context) = cell.borrow_mut().as_mut() {
            context.record_outcome(success);
        }
    });
}

fn validate_pcm_scratch_target(path: &Path) -> Result<(), CliError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CliError::new(format!(
            "refusing to overwrite unsafe PCM scratch symlink {}",
            display_path(path)
        ))),
        Ok(metadata) if !metadata.is_file() => Err(CliError::new(format!(
            "refusing to overwrite non-file PCM scratch target {}",
            display_path(path)
        ))),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(CliError::new(format!(
            "failed to inspect PCM scratch target {}: {err}",
            display_path(path)
        ))),
    }
}

fn safe_delete_pcm_scratch_path(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => return,
        Ok(metadata) if !metadata.is_file() => return,
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => return,
    }
    let _ = fs::remove_file(path);
}

fn current_thread_token() -> String {
    let thread_id_dbg = format!("{:?}", std::thread::current().id());
    let digits: String = thread_id_dbg
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        "unknown".to_string()
    } else {
        digits
    }
}

fn prewarm_backend_binary(backend: AsrBackend, program: &str) -> Result<(), String> {
    let args: &[&str] = match backend {
        AsrBackend::WhisperCpp => &["-h"],
        AsrBackend::WhisperKit => &["--help"],
        AsrBackend::Moonshine => {
            return Err(
                "moonshine adapter is not wired in this phase; use whispercpp/whisperkit"
                    .to_string(),
            );
        }
    };
    let helper_label = backend_helper_program_label(backend);

    Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("failed to execute `{helper_label}` prewarm probe: {err}"))?;
    Ok(())
}

pub(super) fn validate_model_path_for_backend(
    config: &TranscribeConfig,
) -> Result<ResolvedModelPath, CliError> {
    let resolved = resolve_model_path(config)?;
    match config.asr_backend {
        AsrBackend::WhisperCpp => {
            if !resolved.path.is_file() {
                return Err(CliError::new(format!(
                    "`--asr-backend whispercpp` expects a model file path, got {} (resolved via {}). Remediation: pass a valid file path{}",
                    display_path(&resolved.path),
                    resolved.source,
                    if resolved.source == "cli --asr-model" {
                        " or omit `--asr-model` to allow default resolution"
                    } else {
                        ""
                    }
                )));
            }
        }
        AsrBackend::WhisperKit => {
            if !resolved.path.is_dir() {
                return Err(CliError::new(format!(
                    "`--asr-backend whisperkit` expects a model directory path, got {} (resolved via {}). Remediation: pass a valid directory path{}",
                    display_path(&resolved.path),
                    resolved.source,
                    if resolved.source == "cli --asr-model" {
                        " or omit `--asr-model` to allow default resolution"
                    } else {
                        ""
                    }
                )));
            }
        }
        AsrBackend::Moonshine => {}
    }
    Ok(resolved)
}

pub(super) fn resolve_model_path(config: &TranscribeConfig) -> Result<ResolvedModelPath, CliError> {
    if !config.asr_model.as_os_str().is_empty() {
        let resolved = absolutize_candidate(config.asr_model.clone());
        if !resolved.exists() {
            return Err(CliError::new(format!(
                "explicit `--asr-model` path does not exist: {}. Expected {} for backend `{}`. Remediation: pass a valid {} path or omit `--asr-model` to allow default resolution.",
                display_path(&resolved),
                expected_model_kind(config.asr_backend),
                config.asr_backend,
                expected_model_kind(config.asr_backend)
            )));
        }
        return Ok(ResolvedModelPath {
            path: resolved,
            source: "cli --asr-model".to_string(),
        });
    }

    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
    if let Ok(env_model) = env::var("RECORDIT_ASR_MODEL") {
        if !env_model.trim().is_empty() {
            candidates.push((
                PathBuf::from(env_model),
                "env RECORDIT_ASR_MODEL".to_string(),
            ));
        }
    }
    candidates.extend(default_model_candidates(config.asr_backend));

    let mut seen = HashSet::new();
    let mut checked = Vec::new();
    for (candidate, source) in candidates {
        let resolved = absolutize_candidate(candidate);
        let normalized = resolved.to_string_lossy().to_string();
        if !seen.insert(normalized) {
            continue;
        }
        checked.push(display_path(&resolved));
        if resolved.exists() {
            return Ok(ResolvedModelPath {
                path: resolved,
                source,
            });
        }
    }

    Err(CliError::new(format!(
        "unable to resolve ASR model for backend `{}`. Precedence: `--asr-model` > `RECORDIT_ASR_MODEL` > backend defaults. Expected {}. Checked: {}. Remediation: pass `--asr-model <path>` or set `RECORDIT_ASR_MODEL` to a valid {} path.",
        config.asr_backend,
        expected_model_kind(config.asr_backend),
        checked.join(" | "),
        expected_model_kind(config.asr_backend)
    )))
}

fn default_model_candidates(backend: AsrBackend) -> Vec<(PathBuf, String)> {
    let mut candidates = Vec::new();
    let sandbox_root = sandbox_model_root();
    match backend {
        AsrBackend::WhisperCpp => {
            if let Some(root) = &sandbox_root {
                candidates.push((
                    root.join("whispercpp").join("ggml-tiny.en.bin"),
                    "sandbox default".to_string(),
                ));
            }
            candidates.push((
                PathBuf::from("artifacts/bench/models/whispercpp/ggml-tiny.en.bin"),
                "repo benchmark default".to_string(),
            ));
            candidates.push((
                PathBuf::from("models/ggml-tiny.en.bin"),
                "repo local models default".to_string(),
            ));
        }
        AsrBackend::WhisperKit => {
            if let Some(root) = &sandbox_root {
                candidates.push((
                    root.join("whisperkit")
                        .join("models/argmaxinc/whisperkit-coreml/openai_whisper-tiny"),
                    "sandbox default".to_string(),
                ));
            }
            candidates.push((
                PathBuf::from(
                    "artifacts/bench/models/whisperkit/models/argmaxinc/whisperkit-coreml/openai_whisper-tiny",
                ),
                "repo benchmark default".to_string(),
            ));
            candidates.push((
                PathBuf::from("models/whisperkit/openai_whisper-tiny"),
                "repo local models default".to_string(),
            ));
        }
        AsrBackend::Moonshine => {
            if let Some(root) = &sandbox_root {
                candidates.push((
                    root.join("moonshine").join("base"),
                    "sandbox default".to_string(),
                ));
            }
            candidates.push((
                PathBuf::from("artifacts/bench/models/moonshine/base"),
                "repo benchmark default".to_string(),
            ));
            candidates.push((
                PathBuf::from("models/moonshine/base"),
                "repo local models default".to_string(),
            ));
        }
    }
    candidates
}

fn sandbox_model_root() -> Option<PathBuf> {
    recordit::storage_roots::resolve_canonical_storage_roots()
        .ok()
        .map(|roots| roots.models_root)
}

pub(super) fn absolutize_candidate(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    match env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path,
    }
}

pub(super) fn expected_model_kind(backend: AsrBackend) -> &'static str {
    match backend {
        AsrBackend::WhisperCpp => "file",
        AsrBackend::WhisperKit => "directory",
        AsrBackend::Moonshine => "file/directory",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recordit::live_asr_pool::LiveAsrAudioInput;
    use std::fs;
    use std::path::PathBuf;

    fn reset_pcm_scratch_context_for_test() -> Option<PathBuf> {
        PCM_SCRATCH_CONTEXT
            .with(|cell| cell.borrow_mut().take().map(|context| context.path.clone()))
    }

    fn request_with_input(audio_input: LiveAsrAudioInput) -> LiveAsrRequest {
        LiveAsrRequest {
            job_id: 1,
            class: LiveAsrJobClass::Final,
            role: "mic",
            label: "mic".to_string(),
            segment_id: "seg-0001".to_string(),
            audio_input,
        }
    }

    #[test]
    fn adapter_request_extracts_path_from_path_variant() {
        let live_request = request_with_input(LiveAsrAudioInput::path(
            PathBuf::from("/tmp/request-path.wav"),
            true,
        ));
        let request = AsrRequest {
            model_path: Path::new("/tmp/model.bin"),
            live_request: &live_request,
            audio_path_override: None,
            language: "en",
            threads: 2,
        };
        let path = request.audio_path().expect("expected path-backed input");
        assert_eq!(path, Path::new("/tmp/request-path.wav"));
    }

    #[test]
    fn adapter_request_rejects_pcm_window_until_backend_pcm_is_wired() {
        let live_request = request_with_input(LiveAsrAudioInput::pcm_window(
            16_000,
            0,
            500,
            vec![0.0, 0.1, -0.1],
        ));
        let request = AsrRequest {
            model_path: Path::new("/tmp/model.bin"),
            live_request: &live_request,
            audio_path_override: None,
            language: "en",
            threads: 2,
        };
        let err = request
            .audio_path()
            .expect_err("pcm window should surface explicit adapter error");
        assert!(err.to_string().contains("path-backed audio input"));
        assert!(err.to_string().contains("seg-0001"));
    }

    #[test]
    fn pcm_scratch_materialization_reuses_worker_local_path_and_overwrites_contents() {
        let _ = reset_pcm_scratch_context_for_test();

        let first = materialize_pcm_request_audio(
            &request_with_input(LiveAsrAudioInput::pcm_window(
                16_000,
                0,
                20,
                vec![0.1, -0.1],
            )),
            TempAudioPolicy::RetainOnFailure,
        )
        .expect("first materialization should succeed")
        .expect("pcm path should be created");
        let second = materialize_pcm_request_audio(
            &request_with_input(LiveAsrAudioInput::pcm_window(
                16_000,
                20,
                60,
                vec![0.2, 0.3, 0.4, 0.5],
            )),
            TempAudioPolicy::RetainOnFailure,
        )
        .expect("second materialization should succeed")
        .expect("pcm path should be created");

        assert_eq!(first, second, "worker-local scratch path should be reused");

        let reader = hound::WavReader::open(&second).expect("scratch wav should be readable");
        assert_eq!(
            reader.duration(),
            4,
            "scratch wav should reflect latest overwrite"
        );

        let path = reset_pcm_scratch_context_for_test().expect("scratch path should exist");
        assert!(
            !path.exists(),
            "clean scratch context should delete reusable file"
        );
    }

    #[test]
    fn pcm_scratch_cleanup_retains_failed_worker_artifact_for_review() {
        let _ = reset_pcm_scratch_context_for_test();

        let scratch = materialize_pcm_request_audio(
            &request_with_input(LiveAsrAudioInput::pcm_window(
                16_000,
                0,
                20,
                vec![0.1, -0.1],
            )),
            TempAudioPolicy::RetainOnFailure,
        )
        .expect("materialization should succeed")
        .expect("pcm path should be created");
        record_pcm_request_outcome(false);

        let path = reset_pcm_scratch_context_for_test().expect("scratch path should exist");
        assert_eq!(path, scratch);
        assert!(path.exists(), "failed worker scratch should be retained");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn pcm_scratch_retry_flow_reuses_path_and_keeps_latest_overwrite() {
        let _ = reset_pcm_scratch_context_for_test();

        let first = materialize_pcm_request_audio(
            &request_with_input(LiveAsrAudioInput::pcm_window(
                16_000,
                0,
                20,
                vec![0.1, -0.1],
            )),
            TempAudioPolicy::RetainOnFailure,
        )
        .expect("first attempt should materialize")
        .expect("first scratch path should exist");
        record_pcm_request_outcome(false);

        let second = materialize_pcm_request_audio(
            &request_with_input(LiveAsrAudioInput::pcm_window(
                16_000,
                20,
                80,
                vec![0.2, 0.3, 0.4, 0.5],
            )),
            TempAudioPolicy::RetainOnFailure,
        )
        .expect("retry attempt should materialize")
        .expect("retry scratch path should exist");
        record_pcm_request_outcome(true);

        assert_eq!(
            first, second,
            "retry flow should reuse worker-local scratch path"
        );
        let reader = hound::WavReader::open(&second).expect("scratch wav should stay readable");
        assert_eq!(
            reader.duration(),
            4,
            "retry write should overwrite with latest samples"
        );

        let path = reset_pcm_scratch_context_for_test().expect("scratch path should exist");
        assert!(
            !path.exists(),
            "success after retry should clear RetainOnFailure scratch"
        );
    }

    #[test]
    fn pcm_scratch_retain_always_keeps_success_artifact() {
        let _ = reset_pcm_scratch_context_for_test();

        let scratch = materialize_pcm_request_audio(
            &request_with_input(LiveAsrAudioInput::pcm_window(
                16_000,
                0,
                20,
                vec![0.1, -0.1],
            )),
            TempAudioPolicy::RetainAlways,
        )
        .expect("materialization should succeed")
        .expect("pcm path should be created");
        record_pcm_request_outcome(true);

        let path = reset_pcm_scratch_context_for_test().expect("scratch path should exist");
        assert_eq!(path, scratch);
        assert!(path.exists(), "retain-always should keep scratch path");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn pcm_scratch_delete_always_removes_failed_artifact() {
        let _ = reset_pcm_scratch_context_for_test();

        let scratch = materialize_pcm_request_audio(
            &request_with_input(LiveAsrAudioInput::pcm_window(
                16_000,
                0,
                20,
                vec![0.1, -0.1],
            )),
            TempAudioPolicy::DeleteAlways,
        )
        .expect("materialization should succeed")
        .expect("pcm path should be created");
        record_pcm_request_outcome(false);

        let path = reset_pcm_scratch_context_for_test().expect("scratch path should exist");
        assert_eq!(path, scratch);
        assert!(
            !path.exists(),
            "delete-always should remove scratch even after failure"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pcm_scratch_refuses_to_overwrite_symlink_target() {
        use std::os::unix::fs::symlink;

        let _ = reset_pcm_scratch_context_for_test();
        let scratch = PcmScratchContext::new(TempAudioPolicy::RetainOnFailure)
            .expect("scratch context should initialize");
        let path = scratch.path.clone();
        drop(scratch);

        let target = path.with_extension("target.wav");
        let _ = fs::remove_file(&target);
        let _ = fs::remove_file(&path);
        fs::write(&target, b"tmp").expect("target file should be writable");
        symlink(&target, &path).expect("scratch symlink should be creatable");

        let err = materialize_pcm_request_audio(
            &request_with_input(LiveAsrAudioInput::pcm_window(16_000, 0, 20, vec![0.1])),
            TempAudioPolicy::RetainOnFailure,
        )
        .expect_err("symlink scratch path should be rejected");
        assert!(err
            .to_string()
            .contains("refusing to overwrite unsafe PCM scratch symlink"));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&target);
        let _ = reset_pcm_scratch_context_for_test();
    }
}
