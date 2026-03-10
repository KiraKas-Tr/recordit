import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as dialogOpen } from "@tauri-apps/plugin-dialog";
import "./App.css";

// ── Types ──────────────────────────────────────────────────────────────────

interface ModelInfo {
  name: string;
  path: string;
  size_mb: number;
}

interface SessionInfo {
  id: string;
  path: string;
  has_jsonl: boolean;
  has_manifest: boolean;
}

interface TranscriptLine {
  id: number;
  event_type: string;
  text: string;
  channel: string;
  start_ms: number;
  end_ms: number;
}

interface PartialPreview {
  text: string;
  channel: string;
}

interface SessionStatus {
  phase: string;
  detail: string;
}

// ── Helpers ────────────────────────────────────────────────────────────────

function fmtMs(ms: number) {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}:${pad(m % 60)}:${pad(s % 60)}`;
  return `${pad(m)}:${pad(s % 60)}`;
}

function pad(n: number) {
  return n.toString().padStart(2, "0");
}

let lineCounter = 0;

// ── App ────────────────────────────────────────────────────────────────────

export default function App() {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [selectedModel, setSelectedModel] = useState<string>("");
  const [durationSec, setDurationSec] = useState<number>(300);
  const [isRecording, setIsRecording] = useState(false);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [phase, setPhase] = useState<string>("");
  const [lines, setLines] = useState<TranscriptLine[]>([]);
  const [debugLines, setDebugLines] = useState<string[]>([]);
  const [showDebug, setShowDebug] = useState(false);
  const [partialPreview, setPartialPreview] = useState<PartialPreview | null>(null);
  const [selectedSession, setSelectedSession] = useState<SessionInfo | null>(null);
  const [historyLines, setHistoryLines] = useState<TranscriptLine[]>([]);
  const [view, setView] = useState<"record" | "history">("record");
  const [error, setError] = useState<string>("");
  // Track models added via Browse so they persist across refreshes
  const [browsedModels, setBrowsedModels] = useState<ModelInfo[]>([]);

  const bottomRef = useRef<HTMLDivElement>(null);

  // Merge scanned models with browsed models, deduplicate by name
  const allModels = [...models];
  for (const bm of browsedModels) {
    if (!allModels.find((m) => m.path === bm.path)) {
      allModels.push(bm);
    }
  }

  // ── Load models + sessions on mount ──────────────────────────────────────

  const refresh = useCallback(async () => {
    try {
      const [m, s, active] = await Promise.all([
        invoke<ModelInfo[]>("list_models"),
        invoke<SessionInfo[]>("list_sessions"),
        invoke<string | null>("get_active_session"),
      ]);
      setModels(m);
      setSessions(s);
      // Auto-select first model only if nothing is selected yet
      if (m.length > 0 && !selectedModel) setSelectedModel(m[0].path);
      if (active) {
        setActiveSessionId(active);
        setIsRecording(true);
      }
    } catch (e) {
      setError(String(e));
    }
  }, [selectedModel]);

  useEffect(() => {
    refresh();
  }, []);

  // ── Tauri event listeners ─────────────────────────────────────────────────

  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    (async () => {
      const u1 = await listen<Omit<TranscriptLine, "id">>("transcript-line", (evt) => {
        const payload = evt.payload;
        if (payload.event_type === "partial") {
          setPartialPreview({ text: payload.text, channel: payload.channel || "live" });
        } else if (payload.event_type === "stable_partial") {
          // stable_partial means part of the text was committed — keep the
          // partial-preview bubble alive so "Hearing…" / "Listening…" stays
          // visible while the segment is still open.  The trailing partial
          // that follows will update the bubble with the remaining suffix.
          setLines((prev) => {
            const last = prev[prev.length - 1];
            // Deduplicate exact repeats.
            if (
              last &&
              last.event_type === payload.event_type &&
              last.text === payload.text &&
              last.channel === payload.channel &&
              last.start_ms === payload.start_ms
            ) {
              return prev;
            }
            // Accumulate consecutive stable_partials from the same channel.
            if (
              last &&
              last.event_type === "stable_partial" &&
              last.channel === payload.channel
            ) {
              const merged = [...prev];
              merged[merged.length - 1] = {
                ...last,
                text: last.text + " " + payload.text,
                end_ms: payload.end_ms,
              };
              return merged;
            }
            return [...prev, { id: lineCounter++, ...payload }];
          });
        } else {
          // final / reconciled_final / llm_final — segment is done.
          setPartialPreview(null);
          setLines((prev) => {
            const last = prev[prev.length - 1];
            // Deduplicate exact repeats.
            if (
              last &&
              last.event_type === payload.event_type &&
              last.text === payload.text &&
              last.channel === payload.channel &&
              last.start_ms === payload.start_ms
            ) {
              return prev;
            }
            return [...prev, { id: lineCounter++, ...payload }];
          });
        }
      });
      unlisteners.push(u1);

      const u2 = await listen<SessionStatus>("session-status", (evt) => {
        setPhase(evt.payload.phase);
        if (evt.payload.phase === "error") {
          setError((prev) =>
            prev
              ? `${prev}\n${evt.payload.detail}`
              : evt.payload.detail
          );
        }
      });
      unlisteners.push(u2);

      const u3 = await listen<{ session_id: string }>("recording-done", async (evt) => {
        setIsRecording(false);
        setPartialPreview(null);
        setPhase("shutdown");
        setActiveSessionId(evt.payload.session_id);
        await refresh();
      });
      unlisteners.push(u3);
    })();

    return () => unlisteners.forEach((u) => u());
  }, []);

  // Auto-scroll transcript
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [lines, partialPreview]);

  // ── Actions ───────────────────────────────────────────────────────────────

  const startRecording = async () => {
    setError("");
    setLines([]);
    setPartialPreview(null);
    setPhase("warmup");
    lineCounter = 0;
    try {
      const result = await invoke<{ session_id: string; session_dir: string }>(
        "start_recording",
        { modelPath: selectedModel, durationSec }
      );
      setIsRecording(true);
      setActiveSessionId(result.session_id);
    } catch (e) {
      setError(String(e));
      setPhase("");
    }
  };

  const stopRecording = async () => {
    setError("");
    try {
      await invoke("stop_recording");
      setIsRecording(false);
      setPhase("stopping…");
    } catch (e) {
      setError(String(e));
    }
  };

  const runDebug = async () => {
    try {
      const lines = await invoke<string[]>("debug_paths");
      setDebugLines(lines);
      setShowDebug(true);
    } catch (e) {
      setDebugLines([String(e)]);
      setShowDebug(true);
    }
  };

  const openSession = async (session: SessionInfo) => {
    setSelectedSession(session);
    setView("history");
    try {
      const events = await invoke<TranscriptLine[]>("get_session_transcript", {
        sessionPath: session.path,
      });
      const finalEvents = events.filter(
        (e) =>
          e.event_type === "stable_partial" ||
          e.event_type === "final" ||
          e.event_type === "reconciled_final" ||
          e.event_type === "llm_final"
      );
      // Merge consecutive stable_partials from the same channel and deduplicate.
      const merged: TranscriptLine[] = [];
      for (const e of finalEvents) {
        const last = merged[merged.length - 1];
        // Skip exact duplicates (same type, text, channel, timestamp).
        if (
          last &&
          last.event_type === e.event_type &&
          last.text === e.text &&
          last.channel === e.channel &&
          last.start_ms === e.start_ms
        ) {
          continue;
        }
        // Accumulate consecutive stable_partials from the same channel.
        if (
          e.event_type === "stable_partial" &&
          last &&
          last.event_type === "stable_partial" &&
          last.channel === e.channel
        ) {
          last.text = last.text + " " + e.text;
          last.end_ms = e.end_ms;
          continue;
        }
        merged.push({ ...e });
      }
      setHistoryLines(merged.map((e, i) => ({ ...e, id: i })));
    } catch (e) {
      setError(String(e));
    }
  };

  // ── Render ────────────────────────────────────────────────────────────────

  return (
    <div className="app">
      {/* Sidebar */}
      <aside className="sidebar">
        <div className="logo">🎙 Recordit</div>

        <nav className="nav">
          <button
            className={`nav-btn ${view === "record" ? "active" : ""}`}
            onClick={() => setView("record")}
          >
            ● Record
          </button>
          <button
            className={`nav-btn ${view === "history" ? "active" : ""}`}
            onClick={() => setView("history")}
          >
            📋 Sessions
          </button>
        </nav>

        {view === "history" && (
          <div className="session-list">
            {sessions.length === 0 && (
              <p className="muted small">No sessions yet</p>
            )}
            {sessions.map((s) => (
              <button
                key={s.id}
                className={`session-item ${selectedSession?.id === s.id ? "active" : ""}`}
                onClick={() => openSession(s)}
              >
                <span className="session-id">{s.id}</span>
                <span className="session-badges">
                  {s.has_jsonl && <span className="badge green">jsonl</span>}
                  {s.has_manifest && <span className="badge blue">manifest</span>}
                </span>
              </button>
            ))}
          </div>
        )}
      </aside>

      {/* Main content */}
      <main className="main">
        {view === "record" && (
          <>
            {/* Controls */}
            <section className="controls-panel">
              <div className="control-row">
                <label>Model</label>
                <select
                  value={selectedModel}
                  onChange={(e) => setSelectedModel(e.target.value)}
                  disabled={isRecording}
                  className="model-select"
                >
                  {allModels.length === 0 && (
                    <option value="">— pick a model with Browse →</option>
                  )}
                  {allModels.map((m) => (
                    <option key={m.path} value={m.path}>
                      {m.name} ({m.size_mb.toFixed(0)} MB)
                    </option>
                  ))}
                </select>
                <button
                  className="btn btn-browse"
                  disabled={isRecording}
                  onClick={async () => {
                    const file = await dialogOpen({
                      title: "Select Whisper model (.bin)",
                      filters: [{ name: "Model", extensions: ["bin"] }],
                      multiple: false,
                    });
                    if (typeof file === "string") {
                      const name = file.split(/[\\/]/).pop() || file;
                      // Add to browsed models if not already in scanned list
                      if (!models.find((m) => m.path === file)) {
                        setBrowsedModels((prev) => {
                          if (prev.find((m) => m.path === file)) return prev;
                          return [...prev, { name, path: file, size_mb: 0 }];
                        });
                      }
                      setSelectedModel(file);
                    }
                  }}
                >
                  Browse…
                </button>
              </div>

              <div className="control-row">
                <label>Duration (sec)</label>
                <input
                  type="number"
                  min={10}
                  max={3600}
                  value={durationSec}
                  onChange={(e) => setDurationSec(Number(e.target.value))}
                  disabled={isRecording}
                  className="duration-input"
                />
              </div>

              <div className="control-row actions">
                {!isRecording ? (
                  <button
                    className="btn btn-record"
                    onClick={startRecording}
                    disabled={!selectedModel}
                  >
                    ▶ Start Recording
                  </button>
                ) : (
                  <button className="btn btn-stop" onClick={stopRecording}>
                    ■ Stop
                  </button>
                )}

                {isRecording && (
                  <span className="recording-indicator">
                    <span className="dot-pulse" />
                    {" Recording"}
                    {activeSessionId && (
                      <span className="session-label">{activeSessionId}</span>
                    )}
                  </span>
                )}

                {!isRecording && phase && (
                  <span className="phase-label muted">{phase}</span>
                )}
              </div>

              {error && <div className="error-box">{error}</div>}

              <div className="control-row">
                <button className="btn btn-debug" onClick={runDebug}>
                  🔍 Debug paths
                </button>
              </div>

              {showDebug && (
                <div className="debug-box">
                  <div className="debug-header">
                    <strong>Path scan results</strong>
                    <button className="btn-close" onClick={() => setShowDebug(false)}>✕</button>
                  </div>
                  {debugLines.map((l, i) => (
                    <div key={i} className={`debug-line ${l.startsWith("[EXISTS]") ? "exists" : l.startsWith("[missing]") ? "missing" : l.startsWith("  ->") ? "file" : ""}`}>{l}</div>
                  ))}
                </div>
              )}
            </section>

            {/* Live phase banner */}
            {isRecording && phase && (
              <div className={`phase-banner phase-${phase}`}>
                {phase === "warmup" && "⏳ Warming up…"}
                {phase === "active" && "🎙 Live — transcribing"}
                {phase === "draining" && "⌛ Draining…"}
                {phase === "shutdown" && "✅ Done"}
                {phase === "error" && "❌ Error"}
                {!["warmup","active","draining","shutdown","error"].includes(phase) && phase}
              </div>
            )}

            {/* Transcript panel */}
            <section className="transcript-panel">
              {lines.length === 0 && !partialPreview && (
                <p className="empty-hint muted">
                  {isRecording
                    ? "Waiting for first transcript…"
                    : "Start a recording to see realtime transcript here."}
                </p>
              )}

              {lines.map((line) => (
                <div key={line.id} className={`transcript-line type-${line.event_type}`}>
                  <span className="ts">{fmtMs(line.start_ms)}</span>
                  <span className="ch">{line.channel}</span>
                  <span className="txt">{line.text}</span>
                </div>
              ))}

              {partialPreview && (
                <div className="partial-preview-bubble">
                  <div className="partial-preview-label">
                    {partialPreview.channel === "mic" || partialPreview.channel === "microphone"
                      ? "Listening…"
                      : "Hearing…"}
                  </div>
                  <div className="partial-preview-text">{partialPreview.text}</div>
                </div>
              )}

              <div ref={bottomRef} />
            </section>
          </>
        )}

        {view === "history" && (
          <>
            {!selectedSession && (
              <div className="empty-state">
                <p className="muted">Select a session from the sidebar.</p>
              </div>
            )}

            {selectedSession && (
              <section className="history-panel">
                <h2 className="history-title">{selectedSession.id}</h2>
                <p className="muted small history-path">{selectedSession.path}</p>

                <div className="transcript-panel">
                  {historyLines.length === 0 && (
                    <p className="empty-hint muted">No transcript events found.</p>
                  )}
                  {historyLines.map((line) => (
                    <div
                      key={line.id}
                      className={`transcript-line type-${line.event_type}`}
                    >
                      <span className="ts">{fmtMs(line.start_ms)}</span>
                      <span className="ch">{line.channel}</span>
                      <span className="txt">{line.text}</span>
                    </div>
                  ))}
                </div>
              </section>
            )}
          </>
        )}
      </main>
    </div>
  );
}
