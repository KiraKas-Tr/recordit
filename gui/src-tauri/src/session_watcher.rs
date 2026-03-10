use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
pub struct TranscriptLineEvent {
    pub event_type: String,
    pub text: String,
    pub channel: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Serialize, Clone)]
pub struct SessionStatusEvent {
    pub phase: String,
    pub detail: String,
}

#[derive(Serialize, Clone)]
pub struct RecordingDoneEvent {
    pub session_id: String,
}

/// Background thread: poll session.jsonl and emit Tauri events to the frontend.
///
/// Key design decisions:
///   1. We use byte-accurate file seeking (not `lines()`) to avoid off-by-one
///      errors from Windows CRLF line endings.
///   2. We check process liveness via `try_wait()` on the actual Child handle —
///      no shelling out to `tasklist`, no racy OpenProcess.
///   3. We read *after* checking file size to avoid blocking on partial writes.
pub fn watch_jsonl(
    app: AppHandle,
    jsonl_path: PathBuf,
    _state: Arc<Mutex<crate::state::Inner>>,
    child_handle: Arc<Mutex<Child>>,
) {
    let mut byte_offset: u64 = 0;

    loop {
        // Read any new complete lines from the JSONL file.
        read_new_lines(&app, &jsonl_path, &mut byte_offset);

        // Check AFTER reading so we always flush the last batch of lines.
        let still_running = match child_handle.lock() {
            Ok(mut child) => child.try_wait().ok().flatten().is_none(),
            Err(_) => false, // poisoned mutex — treat as dead
        };

        if !still_running {
            // Do one final read pass (file may have been flushed after process exit).
            read_new_lines(&app, &jsonl_path, &mut byte_offset);

            let session_id = jsonl_path
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let _ = app.emit("recording-done", RecordingDoneEvent { session_id });
            break;
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Read all complete lines from `jsonl_path` starting at `byte_offset`.
/// Advances `byte_offset` past all consumed bytes.
///
/// Handles both `\n` (Unix) and `\r\n` (Windows) line endings correctly
/// by counting actual bytes read rather than relying on `str::len()`.
///
/// If the file shrinks (e.g. because the runtime rewrites it at end-of-run
/// with `File::create()`), we skip the read entirely and reset `byte_offset`
/// to the new file length. The rewritten file contains the same events we
/// already forwarded during live tailing, so re-reading would only produce
/// duplicate transcript lines in the GUI.
fn read_new_lines(app: &AppHandle, jsonl_path: &PathBuf, byte_offset: &mut u64) {
    let Ok(meta) = std::fs::metadata(jsonl_path) else {
        return;
    };
    let file_len = meta.len();
    if file_len < *byte_offset {
        // File was truncated/rewritten — skip to avoid duplicates.
        *byte_offset = file_len;
        return;
    }
    if file_len == *byte_offset {
        return;
    }

    let Ok(file) = std::fs::File::open(jsonl_path) else {
        return;
    };
    let mut reader = std::io::BufReader::new(file);
    if reader.seek(SeekFrom::Start(*byte_offset)).is_err() {
        return;
    }

    let bytes_to_read = (file_len - *byte_offset) as usize;
    let mut buf = vec![0u8; bytes_to_read];
    if reader.read_exact(&mut buf).is_err() {
        return;
    }

    // Find the last newline — everything after it is an incomplete line
    // that we'll pick up on the next poll.
    let last_newline_pos = buf.iter().rposition(|&b| b == b'\n');
    let complete_bytes = match last_newline_pos {
        Some(pos) => pos + 1, // include the \n itself
        None => return,       // no complete line yet
    };

    // Process only the complete portion.
    let text = String::from_utf8_lossy(&buf[..complete_bytes]);
    for raw_line in text.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if !line.is_empty() {
            process_jsonl_line(app, line);
        }
    }

    *byte_offset += complete_bytes as u64;
}

fn process_jsonl_line(app: &AppHandle, line: &str) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        let et = v.get("event_type").and_then(|e| e.as_str()).unwrap_or("");

        match et {
            "partial" | "stable_partial" | "final" | "reconciled_final" | "llm_final" => {
                let evt = TranscriptLineEvent {
                    event_type: et.to_string(),
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
                };
                let _ = app.emit("transcript-line", evt);
            }
            "lifecycle_phase" => {
                let phase = v
                    .get("phase")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let detail = v
                    .get("detail")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let _ = app.emit("session-status", SessionStatusEvent { phase, detail });
            }
            _ => {}
        }
    }
}
