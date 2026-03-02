use super::*;

struct AsrRequest<'a> {
    model_path: &'a Path,
    audio_path: &'a Path,
    language: &'a str,
    threads: usize,
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
        let output = Command::new(&self.program)
            .args([
                "-m",
                &request.model_path.to_string_lossy(),
                "-f",
                &request.audio_path.to_string_lossy(),
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
        let output = Command::new(&self.program)
            .args([
                "transcribe",
                "--audio-path",
                &request.audio_path.to_string_lossy(),
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

    fn transcribe(&self, audio_path: &Path) -> Result<String, String> {
        let adapter = select_adapter(self.backend, self.helper_program.clone())
            .map_err(|err| err.to_string())?;
        adapter
            .transcribe(&AsrRequest {
                model_path: &self.model_path,
                audio_path,
                language: &self.language,
                threads: self.threads,
            })
            .map_err(|err| err.to_string())
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
    env::var("HOME").ok().map(|home| {
        PathBuf::from(home).join("Library/Containers/com.recordit.sequoiatranscribe/Data/models")
    })
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
