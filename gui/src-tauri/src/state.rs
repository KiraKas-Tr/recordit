use std::path::PathBuf;
use std::process::Child;
use std::sync::{Arc, Mutex};

/// Public inner struct so session_watcher can hold an Arc to it.
pub struct Inner {
    pub recording_pid: Option<u32>,
    pub active_session_dir: Option<PathBuf>,
    /// Keep the child process handle so the watcher can call try_wait() —
    /// far more reliable than `tasklist` or OpenProcess for monitoring.
    pub recording_child: Option<Arc<Mutex<Child>>>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            recording_pid: None,
            active_session_dir: None,
            recording_child: None,
        }
    }
}

/// Shared mutable state for the Tauri app.
#[derive(Default)]
pub struct AppState {
    pub inner: Arc<Mutex<Inner>>,
}

impl AppState {
    pub fn inner_arc(&self) -> Arc<Mutex<Inner>> {
        Arc::clone(&self.inner)
    }

    pub fn set_recording(&self, pid: u32, session_dir: PathBuf, child: Child) {
        let mut g = self.inner.lock().unwrap();
        g.recording_pid = Some(pid);
        g.active_session_dir = Some(session_dir);
        g.recording_child = Some(Arc::new(Mutex::new(child)));
    }

    pub fn clear_recording(&self) {
        let mut g = self.inner.lock().unwrap();
        g.recording_pid = None;
        g.recording_child = None;
        // keep active_session_dir so UI can still read the finished session
    }

    pub fn recording_pid(&self) -> Option<u32> {
        self.inner.lock().unwrap().recording_pid
    }

    pub fn active_session_dir(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().active_session_dir.clone()
    }

    pub fn recording_child(&self) -> Option<Arc<Mutex<Child>>> {
        self.inner.lock().unwrap().recording_child.clone()
    }
}
