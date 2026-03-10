use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter, State};

// ── Path resolution helpers ────────────────────────────────────────────────

/// Resolve the data root for recordit sessions.
/// Priority:
///   1. RECORDIT_CONTAINER_DATA_ROOT env var
///   2. %USERPROFILE%\AppData\Local\recordit  (Windows default for the GUI)
fn data_root() -> PathBuf {
    if let Ok(v) = std::env::var("RECORDIT_CONTAINER_DATA_ROOT") {
        let p = PathBuf::from(v.trim());
        if p.is_absolute() {
            return p;
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return PathBuf::from(profile)
            .join("AppData")
            .join("Local")
            .join("recordit");
    }
    PathBuf::from("C:\\recordit-data")
}

fn sessions_root() -> PathBuf {
    data_root().join("artifacts").join("sessions")
}

fn models_root() -> PathBuf {
    data_root().join("models")
}

/// Return the directory containing the current executable.
/// In dev mode that's  …/gui/src-tauri/target/debug/
/// In release it's wherever the .exe was installed.
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Find the *project* workspace root: the ancestor that contains both
/// `src/bin/` (the recordit CLI binaries) and `artifacts/` (session +
/// model storage).
///
/// This deliberately checks for `src/bin/` rather than bare `src/` to
/// skip `gui/src-tauri/` which also has `src/` and may have a Tauri
/// build-generated `artifacts/` directory.
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join("src").join("bin").exists() && cur.join("artifacts").exists() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Find the recordit CLI binary.  Works in both dev and production layouts,
/// on both Windows (recordit.exe) and Unix (recordit).
fn find_recordit_exe(_state: &AppState) -> PathBuf {
    let exe = exe_dir();
    let names = recordit_binary_names();

    // 1. Dev layout: when running from `gui/src-tauri/target/...`, prefer the
    // workspace CLI binary over any sibling executable because Tauri dev can
    // leave stale copies next to `gui.exe`.
    if let Some(workspace) = find_workspace_root(&exe) {
        for profile in &["debug", "release"] {
            for name in &names {
                let candidate = workspace.join("target").join(profile).join(name);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    // 2. Sibling in the same dir (production layout)
    for name in &names {
        let sibling = exe.join(name);
        if sibling.exists() {
            return sibling;
        }
    }

    // 3. Fallback: bare name, hope it's in PATH
    PathBuf::from(names[0])
}

/// Resolve a usable whisper.cpp helper path for Windows dev launches.
///
/// In this repo, `recordit.exe` is usually rebuilt in `target/debug/`, while
/// `whisper-cli.exe` may only exist in `target/release/`. When that happens,
/// debug live runs would submit ASR jobs but every one would fail before any
/// transcript events are emitted. We fix that by wiring the helper via the
/// runtime env override consumed by `resolve_backend_program()`.
fn maybe_find_whispercpp_helper(_recordit_exe: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let exe_dir = _recordit_exe.parent()?;
        let sibling = exe_dir.join("whisper-cli.exe");
        if sibling.is_file() {
            return Some(sibling);
        }

        let workspace = find_workspace_root(exe_dir)?;
        let release_helper = workspace
            .join("target")
            .join("release")
            .join("whisper-cli.exe");
        if release_helper.is_file() {
            return Some(release_helper);
        }
    }

    None
}

/// Return the platform-appropriate binary names to search for.
fn recordit_binary_names() -> Vec<&'static str> {
    if cfg!(target_os = "windows") {
        vec!["recordit.exe"]
    } else {
        vec!["recordit", "recordit.exe"]
    }
}

/// All directories to scan for .bin model files.
fn model_scan_dirs(_state: &AppState) -> Vec<PathBuf> {
    let mut dirs = vec![models_root()];

    let exe = exe_dir();

    // Production: models next to the exe
    dirs.push(exe.join("models"));

    // Dev: walk up to workspace root (has Cargo.lock), then artifacts/bench/models/whispercpp
    if let Some(workspace) = find_workspace_root(&exe) {
        dirs.push(
            workspace
                .join("artifacts")
                .join("bench")
                .join("models")
                .join("whispercpp"),
        );
        // Also check workspace-level models/
        dirs.push(workspace.join("models"));
    }

    dirs
}

// ── Serde models ──────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub path: String,
    pub size_mb: f64,
}

#[derive(Serialize, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub path: String,
    /// "YYYYMMDD/session_id" display label
    pub label: String,
    pub has_jsonl: bool,
    pub has_manifest: bool,
}

#[derive(Serialize, Clone)]
pub struct RecordingStarted {
    pub session_id: String,
    pub session_dir: String,
}

/// A single event parsed from session.jsonl.
#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct JsonlEvent {
    pub event_type: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub start_ms: u64,
    #[serde(default)]
    pub end_ms: u64,
    #[serde(default)]
    pub phase: String,
}

// ── Tauri commands ────────────────────────────────────────────────────────

/// Debug: return all paths being scanned + which exist, so the UI can show them.
#[tauri::command]
pub fn debug_paths(state: State<AppState>) -> Vec<String> {
    let exe = exe_dir();
    let mut lines = vec![
        format!("current_exe dir: {}", exe.display()),
        format!(
            "cwd: {}",
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or("?".into())
        ),
    ];

    let ancestor = find_workspace_root(&exe);
    lines.push(format!(
        "workspace root (artifacts+src): {}",
        ancestor
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or("NOT FOUND".into())
    ));

    for dir in model_scan_dirs(&state) {
        let exists = dir.exists();
        lines.push(format!(
            "[{}] {}",
            if exists { "EXISTS" } else { "missing" },
            dir.display()
        ));
        if exists {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    lines.push(format!("  -> {}", e.file_name().to_string_lossy()));
                }
            }
        }
    }
    lines
}

/// List .bin model files from known model directories.
#[tauri::command]
pub fn list_models(state: State<AppState>) -> Vec<ModelInfo> {
    let dirs = model_scan_dirs(&state);

    let mut models = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("bin") {
                continue;
            }
            let size_mb = entry
                .metadata()
                .map(|m| m.len() as f64 / 1_048_576.0)
                .unwrap_or(0.0);
            // Skip LFS pointer stubs (< 1 MB)
            if size_mb < 1.0 {
                continue;
            }
            models.push(ModelInfo {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                path: path.to_string_lossy().to_string(),
                size_mb,
            });
        }
    }

    models.sort_by(|a, b| a.name.cmp(&b.name));
    models.dedup_by(|a, b| a.name == b.name);
    models
}

/// List sessions.  The layout is:  sessions_root/<YYYYMMDD>/<timestamp>-live/
#[tauri::command]
pub fn list_sessions() -> Vec<SessionInfo> {
    let root = sessions_root();
    let Ok(date_dirs) = std::fs::read_dir(&root) else {
        return vec![];
    };

    let mut sessions = Vec::new();
    for date_entry in date_dirs.flatten() {
        if !date_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let date_label = date_entry.file_name().to_string_lossy().to_string();
        let Ok(session_dirs) = std::fs::read_dir(date_entry.path()) else {
            continue;
        };
        for sess_entry in session_dirs.flatten() {
            if !sess_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let path = sess_entry.path();
            let id = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let has_jsonl = path.join("session.jsonl").exists();
            let has_manifest = path.join("session.manifest.json").exists();
            sessions.push(SessionInfo {
                label: format!("{date_label} / {id}"),
                id: id.clone(),
                path: path.to_string_lossy().to_string(),
                has_jsonl,
                has_manifest,
            });
        }
    }

    // Newest first
    sessions.sort_by(|a, b| b.id.cmp(&a.id));
    sessions
}

/// Start a live recording. Returns the session id + dir.
#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    model_path: String,
    duration_sec: u64,
    state: State<AppState>,
) -> Result<RecordingStarted, String> {
    if state.recording_pid().is_some() {
        return Err("A recording is already in progress".to_string());
    }

    // Build an explicit session output root so we always know the path.
    let timestamp = utc_timestamp();
    let date = &timestamp[..8]; // YYYYMMDD
    let session_id = format!("{timestamp}-live");
    let session_dir = sessions_root().join(date).join(&session_id);
    std::fs::create_dir_all(&session_dir).map_err(|e| e.to_string())?;

    let recordit = find_recordit_exe(&state);

    // Log the full command for diagnostics.
    let cmd_display = format!(
        "{} run --mode live --duration-sec {} --profile fast --chunk-window-ms 1200 --chunk-stride-ms 250 --vad-min-speech-ms 120 --vad-min-silence-ms 220 --model \"{}\" --output-root \"{}\"",
        recordit.display(),
        duration_sec,
        model_path,
        session_dir.display()
    );

    let mut cmd = Command::new(&recordit);
    cmd.arg("run")
        .arg("--mode")
        .arg("live")
        .arg("--duration-sec")
        .arg(duration_sec.to_string())
        .arg("--profile")
        .arg("fast")
        .arg("--chunk-window-ms")
        .arg("1200")
        .arg("--chunk-stride-ms")
        .arg("250")
        .arg("--vad-min-speech-ms")
        .arg("120")
        .arg("--vad-min-silence-ms")
        .arg("220")
        .arg("--model")
        .arg(&model_path)
        .arg("--output-root")
        .arg(&session_dir)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());

    if let Some(helper) = maybe_find_whispercpp_helper(&recordit) {
        cmd.env("RECORDIT_WHISPERCPP_CLI_PATH", helper);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to launch recordit.exe: {e}\n\nCommand: {cmd_display}"))?;

    let pid = child.id();

    // Capture stderr in a background thread and write to a log file in the
    // session dir.  Also emit early errors as Tauri events so the GUI can
    // show them immediately.
    let stderr_log_path = session_dir.join("session.stderr.log");
    let stderr_app = app.clone();
    if let Some(stderr) = child.stderr.take() {
        std::thread::Builder::new()
            .name("recordit-stderr".to_string())
            .spawn(move || {
                let reader = BufReader::new(stderr);
                let mut log_file = std::fs::File::create(&stderr_log_path).ok();
                for line in reader.lines().flatten() {
                    // Write to log file.
                    if let Some(ref mut f) = log_file {
                        use std::io::Write;
                        let _ = writeln!(f, "{line}");
                    }
                    // Emit critical errors to the frontend.
                    if line.contains("error:") || line.contains("failed") || line.contains("FAILED")
                    {
                        let _ = stderr_app.emit(
                            "session-status",
                            crate::session_watcher::SessionStatusEvent {
                                phase: "error".to_string(),
                                detail: line.clone(),
                            },
                        );
                    }
                }
            })
            .ok();
    }

    // Capture stdout in a background thread for diagnostics.
    let stdout_log_path = session_dir.join("session.stdout.log");
    if let Some(stdout) = child.stdout.take() {
        std::thread::Builder::new()
            .name("recordit-stdout".to_string())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                let mut log_file = std::fs::File::create(&stdout_log_path).ok();
                for line in reader.lines().flatten() {
                    if let Some(ref mut f) = log_file {
                        use std::io::Write;
                        let _ = writeln!(f, "{line}");
                    }
                }
            })
            .ok();
    }

    // Store the child handle in state so we can use try_wait() for reliable
    // process monitoring (instead of shelling out to tasklist).
    state.set_recording(pid, session_dir.clone(), child);

    // Watch session.jsonl in background and emit transcript events.
    let app_handle = app.clone();
    let jsonl_path = session_dir.join("session.jsonl");
    let state_arc = state.inner_arc();
    let child_handle = state.recording_child().expect("child handle was just set");
    std::thread::spawn(move || {
        crate::session_watcher::watch_jsonl(app_handle, jsonl_path, state_arc, child_handle);
    });

    Ok(RecordingStarted {
        session_id,
        session_dir: session_dir.to_string_lossy().to_string(),
    })
}

/// Stop the active recording by writing a stop-request file.
#[tauri::command]
pub fn stop_recording(state: State<AppState>) -> Result<(), String> {
    let Some(session_dir) = state.active_session_dir() else {
        return Err("No active recording".to_string());
    };
    let stop_file = session_dir.join("session.stop.request");
    std::fs::write(&stop_file, "stop").map_err(|e| e.to_string())?;
    state.clear_recording();
    Ok(())
}

/// Return the currently active session id, or null.
#[tauri::command]
pub fn get_active_session(state: State<AppState>) -> Option<String> {
    state.active_session_dir().map(|p| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    })
}

/// Read all transcript events from a session's jsonl.
#[tauri::command]
pub fn get_session_transcript(session_path: String) -> Vec<JsonlEvent> {
    read_transcript_events(Path::new(&session_path))
}

// ── Internal helpers ──────────────────────────────────────────────────────

pub fn read_transcript_events(session_dir: &Path) -> Vec<JsonlEvent> {
    let jsonl = session_dir.join("session.jsonl");
    let Ok(file) = std::fs::File::open(&jsonl) else {
        return vec![];
    };
    let reader = BufReader::new(file);
    reader
        .lines()
        .flatten()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(&line).ok()?;
            let et = v.get("event_type")?.as_str()?.to_string();
            if !matches!(
                et.as_str(),
                "partial" | "stable_partial" | "final" | "reconciled_final" | "llm_final"
            ) {
                return None;
            }
            Some(JsonlEvent {
                event_type: et,
                text: v
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                channel: v
                    .get("channel")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                start_ms: v.get("start_ms").and_then(|t| t.as_u64()).unwrap_or(0),
                end_ms: v.get("end_ms").and_then(|t| t.as_u64()).unwrap_or(0),
                phase: String::new(),
            })
        })
        .collect()
}

/// Generate a compact UTC timestamp: YYYYMMDDTHHMMSSz
fn utc_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let secs_of_day = secs % 86400;
    let days = secs / 86400;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        year, month, day, hour, minute, second
    )
}
