use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};
use hound::{SampleFormat as HoundFormat, WavSpec, WavWriter};
use midir::{Ignore, MidiInput};
use reqwest::Client;
use rouille::Server;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use futures::StreamExt;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use rand::seq::SliceRandom;
use tempfile::NamedTempFile;

fn base_dir() -> PathBuf {
    dirs::home_dir().expect("No home directory found").join(".stomp-claw")
}

static LOG_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("stomp-claw.log").to_string_lossy().to_string());
static CONVERSATION_LOG_DIR: LazyLock<String> = LazyLock::new(|| base_dir().join("conversations").to_string_lossy().to_string());
static LIVE_DIR: LazyLock<String> = LazyLock::new(|| base_dir().join("live").to_string_lossy().to_string());
static SESSION_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("session.txt").to_string_lossy().to_string());
static CONFIG_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("config.toml").to_string_lossy().to_string());
static SESSIONS_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("sessions.json").to_string_lossy().to_string());

const PEDAL_CC: u8 = 85;
const NEMO_URL: &str = "http://localhost:5051";
const TARGET_SAMPLE_RATE: u32 = 16000;
const OPENCLAW_URL: &str = "http://127.0.0.1:18789/v1/chat/completions";
const OPENCLAW_TOKEN: &str = "06b21a7fafad855670f81018f3a455edccaf5dedc470fa0b";
const AUDIO_SINK: &str = "alsa_output.pci-0000_0d_00.4.analog-stereo";
const VIEWER_PORT: &str = "localhost:8765";

const HELP_HTML: &str = r##"<div class="help-page">
<h2>Voice Commands</h2>

<div class="help-group">
<h3>Session Management</h3>
<table>
<tr><td class="cmd">new session</td><td>Start a fresh conversation session</td></tr>
<tr><td class="cmd">list sessions</td><td>List all sessions by number</td></tr>
<tr><td class="cmd">switch session <em>&lt;name or #&gt;</em></td><td>Switch to a session by name or number</td></tr>
<tr><td class="cmd"><em>&lt;codename&gt;</em></td><td>Say a session codename directly to switch to it</td></tr>
<tr><td class="cmd">rename session <em>&lt;name&gt;</em></td><td>Rename the current session</td></tr>
<tr><td class="cmd">delete session</td><td>Delete the current session (asks for confirmation)</td></tr>
</table>
<p class="help-aliases">Aliases: <span>new conversation</span>, <span>reset session</span>, <span>clear context</span>, <span>start over</span>, <span>fresh start</span>, <span>show sessions</span>, <span>go to session &lt;name&gt;</span>, <span>name session &lt;name&gt;</span>, <span>remove session</span></p>
</div>

<div class="help-group">
<h3>Voice Control</h3>
<table>
<tr><td class="cmd">voice on</td><td>Enable spoken responses (short, 1-2 sentences)</td></tr>
<tr><td class="cmd">voice off</td><td>Disable spoken responses (full text replies)</td></tr>
</table>
<p class="help-aliases">Aliases: <span>speech on</span>, <span>speech off</span></p>
</div>

<div class="help-group">
<h3>Navigation</h3>
<table>
<tr><td class="cmd">help</td><td>Show this help page</td></tr>
<tr><td class="cmd">show live</td><td>Switch to the live view</td></tr>
<tr><td class="cmd">show history</td><td>Switch to the history view</td></tr>
</table>
<p class="help-aliases">Aliases: <span>commands</span>, <span>live view</span>, <span>view live</span>, <span>live</span>, <span>history view</span>, <span>view history</span>, <span>history</span></p>
</div>

<div class="help-group">
<h3>Recording</h3>
<table>
<tr><td class="cmd">Hold pedal</td><td>Start recording your voice</td></tr>
<tr><td class="cmd">Release pedal</td><td>Stop recording and send to AI</td></tr>
<tr><td class="cmd">ignore this</td><td>Cancel the current recording (say while holding pedal)</td></tr>
</table>
<p class="help-aliases">Cancel aliases: <span>never mind</span>, <span>forget it</span>, <span>scratch that</span></p>
</div>

<div class="help-group">
<h3>Confirmation</h3>
<table>
<tr><td class="cmd">yes</td><td>Confirm a pending action (e.g. delete session)</td></tr>
<tr><td class="cmd">no</td><td>Cancel a pending action</td></tr>
</table>
<p class="help-aliases">Aliases: <span>yeah</span>, <span>yep</span>, <span>confirm</span>, <span>do it</span>, <span>nope</span>, <span>cancel</span>, <span>never mind</span></p>
</div>
</div>"##;

const VIEWER_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta http-equiv="Cache-Control" content="no-cache, no-store, must-revalidate">
    <meta http-equiv="Pragma" content="no-cache">
    <meta http-equiv="Expires" content="0">
    <meta charset="utf-8">
    <title>Stomp Claw Live</title>
    <style>
        * { box-sizing: border-box; }
        body {
            background: #0d1117;
            color: #c9d1d9;
            font-family: 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
            font-size: 16px;
            line-height: 1.7;
            margin: 0;
            padding: 0;
        }
        #top-bar {
            position: sticky;
            top: 0;
            z-index: 10;
        }
        .help-btn {
            padding: 4px 10px;
            border-radius: 14px;
            border: 1px solid #30363d;
            background: #161b22;
            color: #8b949e;
            cursor: pointer;
            font-size: 12px;
            font-family: inherit;
            margin-left: auto;
        }
        .help-btn:hover { border-color: #58a6ff; color: #c9d1d9; }
        #content {
            max-width: 800px;
            margin: 0 auto;
            padding: 32px 20px;
        }
        .heading { color: #58a6ff; font-weight: bold; }
        .sub-heading { color: #d2a8ff; font-weight: bold; }
        .you-said { color: #3fb950; font-weight: bold; }
        .user-text { color: #7ee787; }
        .alan-replied { color: #4488ff; font-weight: bold; }
        .alan-text { color: #aaddff; }
        .recording { color: #f0883e; font-weight: bold; }
        .thinking { color: #d29922; font-weight: bold; }
        .separator { color: #30363d; }
        .live-separator {
            color: #30363d;
            text-align: center;
            margin: 24px 0 16px 0;
            font-size: 13px;
            letter-spacing: 1px;
        }
        .live-separator hr {
            border: none;
            border-top: 1px solid #30363d;
            margin: 0;
        }
        .timestamp { color: #6e7681; }
        #status {
            position: fixed;
            bottom: 10px;
            right: 10px;
            font-size: 12px;
            color: #484f58;
        }
        .connected { color: #3fb950; }
        .disconnected { color: #f85149; }
        #session-bar {
            background: #0d1117;
            border-bottom: 1px solid #30363d;
            padding: 8px 20px;
            display: flex;
            gap: 6px;
            flex-wrap: wrap;
            align-items: center;
        }
        .session-btn {
            padding: 4px 14px;
            border-radius: 14px;
            border: 1px solid #30363d;
            background: #161b22;
            color: #8b949e;
            cursor: pointer;
            font-size: 12px;
            font-family: inherit;
            transition: all 0.15s;
            white-space: nowrap;
        }
        .session-btn:hover { border-color: #58a6ff; color: #c9d1d9; }
        .session-btn.active {
            background: #1f6feb;
            border-color: #1f6feb;
            color: #fff;
        }
        .session-btn.new-btn {
            border-style: dashed;
            color: #58a6ff;
        }
        .session-btn.new-btn:hover {
            background: #1f6feb22;
        }
        .session-btn.busy {
            border-color: #d29922;
            animation: busy-pulse 1.5s ease-in-out infinite;
        }
        .session-btn.busy.active {
            border-color: #d29922;
            background: #1f6feb;
        }
        .session-btn.ready {
            border-color: #3fb950;
        }
        @keyframes busy-pulse {
            0%, 100% { border-color: #d29922; }
            50% { border-color: #d2992266; }
        }
        @keyframes spin {
            from { transform: rotate(0deg); }
            to { transform: rotate(360deg); }
        }
        .spinner {
            display: inline-block;
            animation: spin 1s linear infinite;
            margin-left: 6px;
        }
        .help-page h2 {
            color: #58a6ff;
            font-size: 22px;
            margin: 0 0 24px 0;
        }
        .help-group {
            margin-bottom: 28px;
        }
        .help-group h3 {
            color: #d2a8ff;
            font-size: 15px;
            margin: 0 0 10px 0;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .help-group table {
            width: 100%;
            border-collapse: collapse;
        }
        .help-group td {
            padding: 6px 0;
            vertical-align: top;
            border-bottom: 1px solid #21262d;
        }
        .help-group td.cmd {
            color: #7ee787;
            white-space: nowrap;
            padding-right: 24px;
            width: 1%;
        }
        .help-group td.cmd em {
            color: #8b949e;
            font-style: italic;
        }
        .help-aliases {
            color: #484f58;
            font-size: 12px;
            margin: 6px 0 0 0;
        }
        .help-aliases span {
            color: #6e7681;
        }
        .help-aliases span::after {
            content: " \00b7 ";
            color: #484f58;
        }
        .help-aliases span:last-child::after {
            content: "";
        }
    </style>
</head>
<body>
    <div id="top-bar">
        <div id="session-bar"></div>
    </div>
    <div id="content">Waiting for recording...</div>
    <div id="status"><span class="disconnected">●</span> <span id="status-text">Disconnected</span></div>
    <script>
        const contentEl = document.getElementById('content');
        const statusEl = document.getElementById('status-text');
        const dot = document.querySelector('#status span');

        let liveContent = 'Waiting for recording...';
        let sessionWasBusy = {};
        let sessionReady = {};
        let helpContent = '';
        let showingHelp = false;
        let eventSource = null;
        let firstConnect = true;

        // Incremental turn rendering state
        let cachedTurns = [];
        let lastTurnId = 0;

        function escapeHtml(s) {
            return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
        }

        function renderMarkdown(text) {
            const lines = text.split('\\n');
            let html = '';
            let section = 'none';

            for (let line of lines) {
                line = escapeHtml(line);

                if (line.match(/^##\s+.*Recording/)) {
                    html += '<span class="recording">' + line + '</span><br>';
                    section = 'none';
                } else if (line.match(/^##\s+.*You said/)) {
                    html += '<span class="you-said">' + line + '</span><br>';
                    section = 'user';
                } else if (line.match(/^###?\s+.*thinking/i)) {
                    html += '<span class="thinking">' + line + '</span><br>';
                    section = 'none';
                } else if (line.match(/^###?\s+.*Alan replied/)) {
                    html += '<span class="alan-replied">' + line + '</span><br>';
                    section = 'alan';
                } else if (line.match(/^#{1,6}\s+/)) {
                    html += '<span class="heading">' + line + '</span><br>';
                    section = 'none';
                } else if (line.match(/^---+$/)) {
                    html += '<span class="separator">' + line + '</span><br>';
                    section = 'none';
                } else if (section === 'user') {
                    html += '<span class="user-text">' + line + '</span><br>';
                } else if (section === 'alan') {
                    html += '<span class="alan-text">' + line + '</span><br>';
                } else {
                    html += line + '<br>';
                }
            }
            return html;
        }

        function renderTurnHtml(turn) {
            var NL = String.fromCharCode(10);
            var html = '';
            html += '<span class="you-said">## ' + escapeHtml(turn.timestamp) + ' - You said:</span><br>';
            var userLines = turn.user.split(NL);
            for (var i = 0; i < userLines.length; i++) {
                html += '<span class="user-text">' + escapeHtml(userLines[i]) + '</span><br>';
            }
            html += '<br>';
            html += '<span class="alan-replied">### Alan replied:</span><br>';
            var asstLines = turn.assistant.split(NL);
            for (var j = 0; j < asstLines.length; j++) {
                var line = escapeHtml(asstLines[j]);
                if (line.trim() === '') {
                    html += '<br>';
                } else {
                    html += '<span class="alan-text">' + line + '</span><br>';
                }
            }
            html += '<span class="separator">---</span><br>';
            return html;
        }

        function hasLiveActivity(content) {
            if (!content) return false;
            let c = content.trim();
            if (c === '' || c === 'Waiting for recording...') return false;
            let lines = c.split('\\n').filter(l => !l.startsWith('# ') && !l.startsWith('**Session:') && l.trim() !== '' && l.trim() !== 'Waiting for recording...' && l.trim() !== 'Hold the pedal and speak...');
            return lines.length > 0;
        }

        function renderHistoryView() {
            if (showingHelp) return;

            // Always ensure containers exist
            if (!document.getElementById('turns-container')) {
                contentEl.innerHTML = '<div id="turns-container"></div><div id="live-activity"></div>';
            }
            let container = document.getElementById('turns-container');
            let liveEl = document.getElementById('live-activity');

            // Append any turns not yet in the DOM
            for (const turn of cachedTurns) {
                try {
                    if (!container.querySelector('[data-turn-id="' + turn.id + '"]')) {
                        const div = document.createElement('div');
                        div.setAttribute('data-turn-id', turn.id);
                        div.innerHTML = renderTurnHtml(turn);
                        container.appendChild(div);
                    }
                } catch(e) {
                    console.error('Error rendering turn ' + turn.id + ':', e);
                }
            }

            // Empty history placeholder
            let placeholder = container.querySelector('.empty-placeholder');
            if (cachedTurns.length === 0 && container.querySelectorAll('[data-turn-id]').length === 0) {
                if (!placeholder) {
                    container.innerHTML = '<span class="empty-placeholder" style="color:#6e7681">No history for this session yet.</span><br>';
                }
            } else if (placeholder) {
                placeholder.remove();
            }

            // Always re-render live activity
            if (hasLiveActivity(liveContent)) {
                liveEl.innerHTML = '<div class="live-separator"><hr></div>' + renderMarkdown(liveContent);
            } else {
                liveEl.innerHTML = '';
            }
        }

        function showHelp() {
            if (showingHelp) {
                // Toggle off — back to history
                showingHelp = false;
                contentEl.innerHTML = '';
                renderHistoryView();
                return;
            }
            showingHelp = true;
            if (helpContent) {
                contentEl.innerHTML = helpContent;
                return;
            }
            fetch('/help')
                .then(r => r.text())
                .then(html => {
                    helpContent = html;
                    if (showingHelp) {
                        contentEl.innerHTML = helpContent;
                    }
                });
        }

        function fetchNewTurns() {
            fetch('/turns?after=' + lastTurnId)
                .then(r => r.json())
                .then(turns => {
                    if (!turns || turns.length === 0) {
                        statusEl.textContent = 'Connected (turns: ' + cachedTurns.length + ', polling after: ' + lastTurnId + ')';
                        renderHistoryView();
                        return;
                    }
                    turns.forEach(turn => {
                        cachedTurns.push(turn);
                        lastTurnId = turn.id;
                    });
                    statusEl.textContent = 'Connected (turns: ' + cachedTurns.length + ', last: ' + lastTurnId + ')';
                    renderHistoryView();
                    setTimeout(() => window.scrollTo(0, document.body.scrollHeight), 50);
                })
                .catch(err => { statusEl.textContent = 'ERROR: ' + err.message; });
        }

        function resetTurns() {
            cachedTurns = [];
            lastTurnId = 0;
            let container = document.getElementById('turns-container');
            if (container) container.innerHTML = '';
        }

        function connect() {
            eventSource = new EventSource('/events');

            eventSource.onmessage = (e) => {
                let oldLive = liveContent;
                liveContent = e.data;
                let hadActivity = hasLiveActivity(oldLive);
                let hasActivity = hasLiveActivity(liveContent);
                if (hasActivity && showingHelp) {
                    // Live activity started — dismiss help overlay
                    showingHelp = false;
                    contentEl.innerHTML = '';
                }
                if (liveContent !== oldLive && (hadActivity || hasActivity)) {
                    // Activity just ended — fetch new turns (response was saved as a turn)
                    if (hadActivity && !hasActivity) {
                        fetchNewTurns();
                    }
                    renderHistoryView();
                    if (hasActivity) {
                        window.scrollTo(0, document.body.scrollHeight);
                    }
                }
            };

            eventSource.onopen = () => {
                if (!firstConnect) {
                    // Server restarted — reload page to get fresh JS
                    window.location.reload();
                    return;
                }
                firstConnect = false;
                statusEl.textContent = 'Connected';
                dot.className = 'connected';
                fetchSessions();
                resetTurns();
                fetchNewTurns();
            };

            eventSource.onerror = () => {
                statusEl.textContent = 'Reconnecting...';
                dot.className = 'disconnected';
                eventSource.close();
                setTimeout(connect, 1000);
            };
        }

        setInterval(fetchNewTurns, 3000);

        // Session bar
        const sessionBar = document.getElementById('session-bar');
        let lastSessionJson = '';

        function renderSessions(sessions) {
            sessionBar.innerHTML = '';
            for (const s of sessions) {
                const btn = document.createElement('button');
                let wasBusy = sessionWasBusy[s.id] || false;
                let justFinished = wasBusy && !s.busy;
                if (justFinished) {
                    sessionReady[s.id] = Date.now();
                }
                sessionWasBusy[s.id] = s.busy;
                let isReady = sessionReady[s.id] && (Date.now() - sessionReady[s.id] < 10000);
                let cls = 'session-btn' + (s.active ? ' active' : '') + (s.busy ? ' busy' : '') + (isReady && !s.busy ? ' ready' : '');
                btn.className = cls;
                if (s.busy) {
                    btn.innerHTML = s.name + ' <span class=\"spinner\">⟳</span>';
                } else if (isReady) {
                    btn.innerHTML = s.name + ' ✅';
                } else {
                    btn.textContent = s.name;
                }
                btn.onclick = () => {
                    if (!s.active) {
                        fetch('/session/switch?id=' + encodeURIComponent(s.id), {method:'POST'})
                            .then(() => {
                                fetchSessions();
                                resetTurns();
                                fetchNewTurns();
                            });
                    }
                };
                sessionBar.appendChild(btn);
            }
            const newBtn = document.createElement('button');
            newBtn.className = 'session-btn new-btn';
            newBtn.textContent = '+ New';
            newBtn.onclick = () => {
                fetch('/session/new', {method:'POST'})
                    .then(() => { fetchSessions(); resetTurns(); fetchNewTurns(); });
            };
            sessionBar.appendChild(newBtn);

            // Help button at the end
            const helpBtn = document.createElement('button');
            helpBtn.className = 'help-btn';
            helpBtn.textContent = '?';
            helpBtn.onclick = showHelp;
            sessionBar.appendChild(helpBtn);
        }

        function fetchSessions() {
            fetch('/sessions')
                .then(r => r.json())
                .then(sessions => {
                    const json = JSON.stringify(sessions);
                    if (json !== lastSessionJson) {
                        lastSessionJson = json;
                        renderSessions(sessions);
                    }
                })
                .catch(() => {});
        }

        fetchSessions();
        setInterval(fetchSessions, 2000);

        connect();
        fetchNewTurns();
    </script>
</body>
</html>"#;

struct ViewerFileReader {
    last_content: String,
    first_read: bool,
}

impl ViewerFileReader {
    fn new() -> Self {
        let initial = fs::read_to_string(&live_log_path()).unwrap_or_else(|_| "Waiting for recording...".to_string());
        Self { last_content: initial, first_read: true }
    }
}

impl std::io::Read for ViewerFileReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Throttle reads — 50ms keeps transcription responsive while saving CPU
        std::thread::sleep(std::time::Duration::from_millis(50));

        let content = fs::read_to_string(&live_log_path())
            .unwrap_or_else(|_| "Waiting for recording...".to_string());

        let msg = if self.first_read || content != self.last_content {
            self.first_read = false;
            self.last_content = content.clone();
            format!("data: {}\n\n", escape_sse(&content))
        } else {
            ": heartbeat\n\n".to_string()
        };

        // Pad to >1024 bytes so tiny_http's BufWriter flushes immediately.
        // Without this, small messages sit in the buffer and the client
        // receives nothing until ~1KB accumulates (many seconds with the
        // 200ms read throttle).
        let padded = if msg.len() < 1200 {
            format!(":{}\n{}", " ".repeat(1200 - msg.len()), msg)
        } else {
            msg
        };

        let bytes = padded.as_bytes();
        let to_copy = std::cmp::min(buf.len(), bytes.len());
        buf[..to_copy].copy_from_slice(&bytes[..to_copy]);
        Ok(to_copy)
    }
}

fn escape_sse(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
}

fn start_viewer(busy_sessions: Arc<Mutex<HashSet<String>>>) {
    log("🌐 Starting viewer on http://localhost:8765");

    let tx = std::sync::mpsc::channel::<PathBuf>().0;

    std::thread::spawn(move || {
        let (watcher_tx, watcher_rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();

        let mut watcher = RecommendedWatcher::new(
            move |res| { let _ = watcher_tx.send(res); },
            NotifyConfig::default().with_poll_interval(std::time::Duration::from_secs(1)),
        ).unwrap();

        let live_dir = PathBuf::from(LIVE_DIR.as_str());
        let _ = watcher.watch(&live_dir, RecursiveMode::NonRecursive);

        loop {
            if let Ok(Ok(_event)) = watcher_rx.recv_timeout(std::time::Duration::from_millis(500)) {
                let _ = tx.send(live_dir.clone());
            }
        }
    });

    let server = Server::new(VIEWER_PORT, move |request| {
        rouille::router!(request,
            (GET) ["/"] => {
                let mut resp = rouille::Response::html(VIEWER_HTML);
                resp.headers.push(("Cache-Control".into(), "no-store, no-cache, must-revalidate".into()));
                resp
            },
            (GET) ["/events"] => {
                let reader = ViewerFileReader::new();

                rouille::Response {
                    status_code: 200,
                    headers: vec![
                        ("Content-Type".into(), "text/event-stream".into()),
                        ("Cache-Control".into(), "no-cache".into()),
                        ("Connection".into(), "keep-alive".into()),
                    ],
                    data: rouille::ResponseBody::from_reader(Box::new(reader)),
                    upgrade: None,
                }
            },
            (GET) ["/history"] => {
                let session = fs::read_to_string(SESSION_FILE.as_str())
                    .unwrap_or_else(|_| "unknown".to_string())
                    .trim().to_string();
                let md = turns_to_markdown(&session);
                let content = if md.is_empty() {
                    "No history for this session yet.".to_string()
                } else {
                    md
                };
                rouille::Response::text(content)
            },
            (GET) ["/turns"] => {
                let session = request.get_param("session").unwrap_or_else(|| {
                    fs::read_to_string(SESSION_FILE.as_str())
                        .unwrap_or_else(|_| "unknown".to_string())
                        .trim().to_string()
                });
                let after: u32 = request.get_param("after")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let turns = read_turns_after(&session, after);
                rouille::Response::json(&turns)
            },
            (GET) ["/turns/count"] => {
                let session = request.get_param("session").unwrap_or_else(|| {
                    fs::read_to_string(SESSION_FILE.as_str())
                        .unwrap_or_else(|_| "unknown".to_string())
                        .trim().to_string()
                });
                let count = turn_count_for(&session);
                rouille::Response::json(&serde_json::json!({"count": count}))
            },
            (GET) ["/sessions"] => {
                let sessions = load_sessions();
                let active_id = get_active_session_id();
                let busy = busy_sessions.lock().unwrap();
                let items: Vec<serde_json::Value> = sessions.iter().map(|s| {
                    serde_json::json!({
                        "id": s.id,
                        "name": s.name,
                        "active": s.id == active_id,
                        "busy": busy.contains(&s.id),
                    })
                }).collect();
                rouille::Response::json(&items)
            },
            (POST) ["/session/switch"] => {
                if let Some(id) = request.get_param("id") {
                    let mut sessions = load_sessions();
                    if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
                        session.last_used = chrono::Local::now().to_rfc3339();
                        let name = session.name.clone();
                        let sid = session.id.clone();
                        save_sessions(&sessions);
                        let _ = std::fs::write(SESSION_FILE.as_str(), &sid);
                        // Restore live view from this session's history
                        restore_live_from_history();
                        play_session_tone(get_active_session_index());
                        rouille::Response::json(&serde_json::json!({"ok": true, "name": name}))
                    } else {
                        rouille::Response::json(&serde_json::json!({"ok": false, "error": "Session not found"}))
                            .with_status_code(404)
                    }
                } else {
                    rouille::Response::json(&serde_json::json!({"ok": false, "error": "Missing id param"}))
                        .with_status_code(400)
                }
            },
            (POST) ["/session/new"] => {
                let name = handle_new_session();
                let id = get_active_session_id();
                play_session_tone(get_active_session_index());
                let live_content = format!("{}New session: **{}**\n\n---\n", session_header(), name);
                let _ = std::fs::write(&live_log_path(), live_content);
                rouille::Response::json(&serde_json::json!({"ok": true, "name": name, "id": id}))
            },
            (POST) ["/session/rename"] => {
                if let Some(name) = request.get_param("name") {
                    match handle_rename_session(&name) {
                        Ok(msg) => rouille::Response::json(&serde_json::json!({"ok": true, "message": msg})),
                        Err(e) => rouille::Response::json(&serde_json::json!({"ok": false, "error": e}))
                            .with_status_code(400),
                    }
                } else {
                    rouille::Response::json(&serde_json::json!({"ok": false, "error": "Missing name param"}))
                        .with_status_code(400)
                }
            },
            (POST) ["/session/delete"] => {
                let deleted = handle_delete_session_confirmed();
                rouille::Response::json(&serde_json::json!({"ok": true, "deleted": deleted}))
            },
            (GET) ["/help"] => {
                rouille::Response::html(HELP_HTML)
            },
            _ => {
                rouille::Response {
                    status_code: 404,
                    headers: vec![("Content-Type".into(), "text/plain".into())],
                    data: rouille::ResponseBody::from_string("Not Found"),
                    upgrade: None,
                }
            },
        )
    }).expect("Failed to create viewer server");

    server.run();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Turn {
    id: u32,
    timestamp: String,
    user: String,
    assistant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    voice_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { voice_enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    id: String,
    name: String,
    created_at: String,
    last_used: String,
}

enum SessionCommand {
    NewSession,
    SwitchSession(String),
    ListSessions,
    RenameSession(String),
    DeleteSession,
}

fn load_sessions() -> Vec<Session> {
    if let Ok(content) = std::fs::read_to_string(SESSIONS_FILE.as_str()) {
        if let Ok(sessions) = serde_json::from_str(&content) {
            return sessions;
        }
    }
    Vec::new()
}

fn save_sessions(sessions: &[Session]) {
    if let Ok(json) = serde_json::to_string_pretty(sessions) {
        let _ = std::fs::write(SESSIONS_FILE.as_str(), json);
    }
}

fn get_active_session_id() -> String {
    std::fs::read_to_string(SESSION_FILE.as_str())
        .unwrap_or_else(|_| "unknown".to_string())
        .trim().to_string()
}

fn get_active_session_name() -> String {
    let id = get_active_session_id();
    let sessions = load_sessions();
    sessions.iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn session_header() -> String {
    format!("**Session: {}**\n\n", get_active_session_name())
}

const SESSION_ADJECTIVES: &[&str] = &[
    "amber", "arctic", "ashen", "azure", "basalt", "blazing", "boreal", "brazen",
    "brisk", "bronze", "cedar", "cobalt", "copper", "coral", "crimson", "crypt",
    "dusk", "ember", "feral", "ferric", "flint", "fossil", "frozen", "gilded",
    "glacial", "granite", "hollow", "hushed", "iron", "ivory", "jagged", "lunar",
    "molten", "moss", "mystic", "neon", "nimble", "obsidian", "onyx", "opaque",
    "pale", "phantom", "plume", "quartz", "riven", "runic", "rustic", "sable",
    "scarlet", "silver", "slate", "smoked", "solar", "stark", "tawny", "umbral",
    "velvet", "vivid", "woven", "zinc",
];

const SESSION_NOUNS: &[&str] = &[
    "anchor", "anvil", "badger", "bastion", "beacon", "bison", "cairn", "chalice",
    "cipher", "compass", "condor", "coyote", "dagger", "drake", "falcon", "forge",
    "frigate", "garnet", "griffin", "harbor", "herald", "hornet", "jackal", "javelin",
    "lantern", "locus", "mammoth", "mantis", "marlin", "monolith", "nexus", "obelisk",
    "osprey", "outpost", "panther", "pebble", "pilgrim", "plinth", "prism", "pylon",
    "quarry", "raven", "ridgeback", "scepter", "schooner", "sentinel", "serpent",
    "sigil", "sparrow", "spindle", "summit", "talon", "tempest", "thistle", "trident",
    "tundra", "vanguard", "vortex", "warden", "zenith",
];

fn generate_session_name() -> String {
    let existing: Vec<String> = load_sessions().iter().map(|s| s.name.clone()).collect();
    let mut rng = rand::thread_rng();

    for _ in 0..100 {
        let adj = SESSION_ADJECTIVES.choose(&mut rng).unwrap();
        let noun = SESSION_NOUNS.choose(&mut rng).unwrap();
        let name = format!("{} {}", adj, noun);
        if !existing.contains(&name) {
            return name;
        }
    }

    // Fallback: append number to guarantee uniqueness
    let adj = SESSION_ADJECTIVES.choose(&mut rng).unwrap();
    let noun = SESSION_NOUNS.choose(&mut rng).unwrap();
    format!("{} {} {}", adj, noun, existing.len() + 1)
}

fn migrate_sessions() {
    if std::path::Path::new(SESSIONS_FILE.as_str()).exists() {
        return;
    }

    let now = chrono::Local::now().to_rfc3339();

    if let Ok(id) = std::fs::read_to_string(SESSION_FILE.as_str()) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            let session = Session {
                id,
                name: generate_session_name(),
                created_at: now.clone(),
                last_used: now,
            };
            save_sessions(&[session]);
            log("📋 Migrated existing session to sessions.json");
            return;
        }
    }

    let session = Session {
        id: format!("stomp-{}", uuid::Uuid::new_v4()),
        name: generate_session_name(),
        created_at: now.clone(),
        last_used: now,
    };
    let _ = std::fs::write(SESSION_FILE.as_str(), &session.id);
    save_sessions(&[session]);
    log("📋 Created initial session");
}

fn handle_new_session() -> String {
    let now = chrono::Local::now().to_rfc3339();
    let mut sessions = load_sessions();
    let session = Session {
        id: format!("stomp-{}", uuid::Uuid::new_v4()),
        name: generate_session_name(),
        created_at: now.clone(),
        last_used: now,
    };
    let id = session.id.clone();
    let name = session.name.clone();
    sessions.push(session);
    save_sessions(&sessions);
    let _ = std::fs::write(SESSION_FILE.as_str(), &id);
    log(&format!("🔄 New session created: {} ({})", name, id));
    name
}

fn handle_switch_session(query: &str) -> Result<String, String> {
    let mut sessions = load_sessions();
    if sessions.is_empty() {
        return Err("No sessions available.".to_string());
    }

    // Convert spoken number words to digits
    let query = match query.to_lowercase().trim() {
        "one" | "won" => "1",
        "two" | "to" | "too" => "2",
        "three" | "tree" => "3",
        "four" | "for" | "fore" => "4",
        "five" => "5",
        "six" | "sicks" => "6",
        "seven" => "7",
        "eight" | "ate" => "8",
        "nine" => "9",
        "ten" => "10",
        other => other,
    }.to_string();
    let query = query.as_str();

    // Try as number first (1-based index)
    if let Ok(num) = query.parse::<usize>() {
        if num >= 1 && num <= sessions.len() {
            sessions[num - 1].last_used = chrono::Local::now().to_rfc3339();
            let id = sessions[num - 1].id.clone();
            let name = sessions[num - 1].name.clone();
            save_sessions(&sessions);
            let _ = std::fs::write(SESSION_FILE.as_str(), &id);
            return Ok(name);
        } else {
            return Err(format!("Session {} out of range. You have {} sessions.", num, sessions.len()));
        }
    }

    // Substring match (case-insensitive)
    let query_lower = query.to_lowercase();
    let matched: Vec<usize> = sessions.iter().enumerate()
        .filter(|(_, s)| s.name.to_lowercase().contains(&query_lower))
        .map(|(i, _)| i)
        .collect();

    match matched.len() {
        0 => Err(format!("No session found matching '{}'.", query)),
        1 => {
            let idx = matched[0];
            sessions[idx].last_used = chrono::Local::now().to_rfc3339();
            let id = sessions[idx].id.clone();
            let name = sessions[idx].name.clone();
            save_sessions(&sessions);
            let _ = std::fs::write(SESSION_FILE.as_str(), &id);
            Ok(name)
        }
        _ => {
            let names: Vec<String> = matched.iter().map(|&i| sessions[i].name.clone()).collect();
            Err(format!("Multiple sessions match: {}. Be more specific.", names.join(", ")))
        }
    }
}

fn handle_list_sessions() -> String {
    let sessions = load_sessions();
    let active_id = get_active_session_id();

    if sessions.is_empty() {
        return "No sessions.".to_string();
    }

    let mut parts = Vec::new();
    for (i, s) in sessions.iter().enumerate() {
        let marker = if s.id == active_id { " (active)" } else { "" };
        parts.push(format!("{}. {}{}", i + 1, s.name, marker));
    }

    format!("You have {} sessions: {}", sessions.len(), parts.join(". "))
}

fn handle_rename_session(new_name: &str) -> Result<String, String> {
    let active_id = get_active_session_id();
    let mut sessions = load_sessions();

    if let Some(session) = sessions.iter_mut().find(|s| s.id == active_id) {
        let old_name = session.name.clone();
        session.name = new_name.to_string();
        save_sessions(&sessions);
        Ok(format!("Renamed '{}' to '{}'", old_name, new_name))
    } else {
        Err("No active session found.".to_string())
    }
}

fn handle_delete_session_confirmed() -> String {
    let active_id = get_active_session_id();
    let mut sessions = load_sessions();

    let deleted_name = sessions.iter()
        .find(|s| s.id == active_id)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    sessions.retain(|s| s.id != active_id);

    if sessions.is_empty() {
        let now = chrono::Local::now().to_rfc3339();
        let new_session = Session {
            id: format!("stomp-{}", uuid::Uuid::new_v4()),
            name: generate_session_name(),
            created_at: now.clone(),
            last_used: now,
        };
        let _ = std::fs::write(SESSION_FILE.as_str(), &new_session.id);
        sessions.push(new_session);
    } else {
        let most_recent = sessions.iter()
            .max_by_key(|s| s.last_used.clone())
            .unwrap();
        let _ = std::fs::write(SESSION_FILE.as_str(), &most_recent.id);
    }

    save_sessions(&sessions);
    // Clean up live file and turn directory for deleted session
    let _ = std::fs::remove_file(&live_log_path_for(&active_id));
    let turn_dir = format!("{}/{}", *CONVERSATION_LOG_DIR, active_id);
    let _ = std::fs::remove_dir_all(&turn_dir);
    // Also remove legacy .md file if it exists
    let md_path = format!("{}/{}.md", *CONVERSATION_LOG_DIR, active_id);
    let _ = std::fs::remove_file(&md_path);
    log(&format!("🗑️ Deleted session: {}", deleted_name));
    deleted_name
}

fn touch_active_session() {
    let active_id = get_active_session_id();
    let mut sessions = load_sessions();
    if let Some(session) = sessions.iter_mut().find(|s| s.id == active_id) {
        session.last_used = chrono::Local::now().to_rfc3339();
        save_sessions(&sessions);
    }
}

fn load_config() -> Config {
    if let Ok(content) = std::fs::read_to_string(CONFIG_FILE.as_str()) {
        if let Ok(c) = toml::from_str(&content) {
            return c;
        }
    }
    Config::default()
}

fn save_config(config: &Config) {
    if let Ok(content) = toml::to_string(config) {
        let _ = std::fs::write(CONFIG_FILE.as_str(), content);
    }
}

fn log(msg: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG_FILE.as_str()) {
        let _ = writeln!(f, "[{}] {}", timestamp, msg);
    }
}

// Simple rotating dots
fn get_thinking_dots() -> &'static str {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let i = COUNTER.fetch_add(1, Ordering::Relaxed) % 4;
    match i {
        0 => ".",
        1 => "..",
        2 => "...",
        3 => "....",
        _ => "....",
    }
}

// Send current audio to nemo for partial transcription
fn send_partial_transcription(samples: &[f32], client: &Client) -> Option<String> {
    log("🔄 Attempting partial transcription...");
    let temp_dir = std::env::temp_dir();
    let wav_path = temp_dir.join("stomp_claw_partial.wav");
    {
        let spec = WavSpec { channels: 1, sample_rate: TARGET_SAMPLE_RATE, bits_per_sample: 16, sample_format: HoundFormat::Int };
        let mut w = WavWriter::create(&wav_path, spec).ok()?;
        for s in samples { w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).ok()?; }
        w.finalize().ok()?;
    }
    
    let mut file = std::fs::File::open(&wav_path).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    drop(file);
    let _ = std::fs::remove_file(&wav_path);
    
    let part = reqwest::multipart::Part::bytes(buf).file_name("audio.wav").mime_str("audio/wav").ok()?;
    let form = reqwest::multipart::Form::new().part("file", part);
    
    let rt = tokio::runtime::Runtime::new();
    if let Ok(rt) = rt {
        return rt.block_on(async {
            let resp = client.post(&format!("{}/transcribe/", NEMO_URL)).multipart(form).send().await.ok()?;
            if resp.status().is_success() {
                let text = resp.text().await.ok()?;
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            None
        });
    }
    None
}

fn update_live_with_partial(text: &str) {
    update_live_with_partial_for(text, &live_log_path());
}

fn update_live_with_partial_for(text: &str, path: &str) {
    let content = format!(
        "{}## 🎙️ Recording... (transcribing)

{}

---
",
        session_header(), text
    );
    let _ = std::fs::write(path, content);
}

fn update_live_recording(seconds: f64) {
    update_live_recording_for(seconds, &live_log_path());
}

fn update_live_recording_for(seconds: f64, path: &str) {
    let dots = ".".repeat((seconds as usize / 2) % 4);
    let content = format!(
        "{}## 🎙️ Recording{} ({}s)

Release pedal to transcribe... (say \"ignore this\" to cancel)
---
",
        session_header(), dots, seconds
    );
    let _ = std::fs::write(path, content);
}

fn update_live_cancelled() {
    update_live_cancelled_for(&live_log_path());
}

fn update_live_cancelled_for(path: &str) {
    let content = format!("{}## ❌ Transcription cancelled by user

---
", session_header());
    let _ = std::fs::write(path, content);
}

fn update_live_thinking(user: &str) {
    update_live_thinking_for(user, &live_log_path());
}

fn update_live_thinking_for(user: &str, path: &str) {
    let dots = get_thinking_dots();
    let content = format!(
        "{}## You said:
{}

### Alan is thinking{}
---
",
        session_header(), user, dots
    );
    let _ = std::fs::write(path, content);
}

fn restore_live_from_history() {
    // With per-turn rendering, history view shows turns directly.
    // Live file just needs a clean slate — no need to replay the last turn.
    let live_content = format!("{}Waiting for recording...\n", session_header());
    let _ = std::fs::write(&live_log_path(), live_content);
}

fn update_live(user: &str, assistant: &str) {
    update_live_for(user, assistant, &live_log_path());
}

fn update_live_for(user: &str, assistant: &str, path: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let content = format!(
        "## {} - You said:
{}

### Alan replied:
{}
---
",
        timestamp, user, assistant
    );
    let _ = std::fs::write(path, content);
}

fn live_log_path() -> String {
    let session = get_active_session_id();
    format!("{}/{}.md", *LIVE_DIR, session)
}

fn live_log_path_for(session_id: &str) -> String {
    format!("{}/{}.md", *LIVE_DIR, session_id)
}

/// Get the directory for per-turn files for a session
fn turn_dir_for(session_id: &str) -> String {
    let dir = format!("{}/{}", *CONVERSATION_LOG_DIR, session_id);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Count turn files in a session directory
fn turn_count_for(session_id: &str) -> u32 {
    let dir = turn_dir_for(session_id);
    std::fs::read_dir(&dir)
        .map(|entries| entries.filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("turn-") && e.file_name().to_string_lossy().ends_with(".json"))
            .count() as u32)
        .unwrap_or(0)
}

/// Read all turns for a session with id > after
fn read_turns_after(session_id: &str, after: u32) -> Vec<Turn> {
    let dir = turn_dir_for(session_id);
    let mut turns = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries.filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("turn-") && name.ends_with(".json")
            })
            .collect();
        files.sort_by_key(|e| e.file_name().to_string_lossy().to_string());
        for entry in files {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(turn) = serde_json::from_str::<Turn>(&content) {
                    if turn.id > after {
                        turns.push(turn);
                    }
                }
            }
        }
    }
    turns
}

/// Read all turns for a session and produce the old markdown format
fn turns_to_markdown(session_id: &str) -> String {
    let turns = read_turns_after(session_id, 0);
    if turns.is_empty() {
        return String::new();
    }
    let mut md = String::new();
    for turn in &turns {
        md.push_str(&format!("## {} - You said:\n{}\n\n### Alan replied:\n{}\n---\n", turn.timestamp, turn.user, turn.assistant));
    }
    md
}

/// Write a new turn file and return its id
fn write_turn(session_id: &str, user: &str, assistant: &str) -> u32 {
    let next_id = turn_count_for(session_id) + 1;
    let turn = Turn {
        id: next_id,
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        user: user.to_string(),
        assistant: assistant.to_string(),
    };
    let dir = turn_dir_for(session_id);
    let path = format!("{}/turn-{:04}.json", dir, next_id);
    if let Ok(json) = serde_json::to_string_pretty(&turn) {
        let _ = std::fs::write(&path, json);
    }
    next_id
}

/// Migrate a monolithic .md conversation file to per-turn files
fn migrate_conversation(session_id: &str) {
    let md_path = format!("{}/{}.md", *CONVERSATION_LOG_DIR, session_id);
    if !std::path::Path::new(&md_path).exists() {
        return;
    }
    let content = match std::fs::read_to_string(&md_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    if content.trim().is_empty() {
        let _ = std::fs::remove_file(&md_path);
        return;
    }

    let dir = turn_dir_for(session_id);
    // Don't re-migrate if turns already exist
    if turn_count_for(session_id) > 0 {
        let _ = std::fs::remove_file(&md_path);
        return;
    }

    let entries: Vec<&str> = content.split("\n---\n").collect();
    let mut turn_id: u32 = 0;
    for entry in &entries {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        // Parse: "## TIMESTAMP - You said:\nUSER\n\n### Alan replied:\nASSISTANT"
        let mut timestamp = String::new();
        let mut user_text = String::new();
        let mut assistant_text = String::new();

        let mut section = "none";
        for line in entry.lines() {
            if line.starts_with("## ") && line.contains("You said") {
                // Extract timestamp: "## 2026-03-06 11:59:41 - You said:"
                if let Some(ts) = line.strip_prefix("## ") {
                    if let Some(pos) = ts.find(" - You said") {
                        timestamp = ts[..pos].to_string();
                    }
                }
                section = "user";
            } else if line.starts_with("### Alan replied") || line.starts_with("### Alan:") {
                section = "assistant";
            } else {
                match section {
                    "user" => {
                        if !user_text.is_empty() { user_text.push('\n'); }
                        user_text.push_str(line);
                    }
                    "assistant" => {
                        if !assistant_text.is_empty() { assistant_text.push('\n'); }
                        assistant_text.push_str(line);
                    }
                    _ => {}
                }
            }
        }

        let user_text = user_text.trim().to_string();
        let assistant_text = assistant_text.trim().to_string();
        if user_text.is_empty() && assistant_text.is_empty() {
            continue;
        }

        turn_id += 1;
        let turn = Turn {
            id: turn_id,
            timestamp: if timestamp.is_empty() { "unknown".to_string() } else { timestamp },
            user: user_text,
            assistant: assistant_text,
        };
        let path = format!("{}/turn-{:04}.json", dir, turn_id);
        if let Ok(json) = serde_json::to_string_pretty(&turn) {
            let _ = std::fs::write(&path, json);
        }
    }

    if turn_id > 0 {
        log(&format!("📋 Migrated {} turns for session {}", turn_id, session_id));
    }
    let _ = std::fs::remove_file(&md_path);
}

fn log_conversation(user: &str, assistant: &str) {
    let session_id = get_active_session_id();
    log_conversation_for(user, assistant, &session_id);
}

fn log_conversation_for(user: &str, assistant: &str, session_id: &str) {
    write_turn(session_id, user, assistant);
}

fn truncate_to_sentences(text: &str, max_sentences: usize) -> String {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() { return text; }
    
    let mut sentence_count = 0;
    for (i, c) in text.char_indices() {
        if c == '.' || c == '?' || c == '!' {
            sentence_count += 1;
            if sentence_count >= max_sentences {
                return text[..=i].trim().to_string();
            }
        }
    }
    text.to_string()
}

/// Strip punctuation and common filler words, return remaining words
fn command_words(transcript: &str) -> Vec<String> {
    let stripped: String = transcript.chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    stripped.to_lowercase()
        .split_whitespace()
        .filter(|w| !matches!(*w, "uh" | "um" | "please" | "the" | "a" | "my"))
        .map(|w| w.to_string())
        .collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

fn check_session_command(transcript: &str) -> Option<SessionCommand> {
    let words = command_words(transcript);
    let slice: Vec<&str> = words.iter().map(|w| w.as_str()).collect();

    match slice.as_slice() {
        ["new", "session"] | ["new", "conversation"]
        | ["reset", "session"] | ["reset", "context"]
        | ["clear", "session"] | ["clear", "context"]
        | ["start", "over"] | ["fresh", "start"] => Some(SessionCommand::NewSession),
        ["list", "sessions"] | ["show", "sessions"] => Some(SessionCommand::ListSessions),
        ["delete", "session"] | ["remove", "session"] => Some(SessionCommand::DeleteSession),
        ["switch", "session", rest @ ..] if !rest.is_empty() => {
            Some(SessionCommand::SwitchSession(rest.join(" ")))
        }
        ["go", "to", "session", rest @ ..] if !rest.is_empty() => {
            Some(SessionCommand::SwitchSession(rest.join(" ")))
        }
        ["rename", "session", rest @ ..] if !rest.is_empty() => {
            Some(SessionCommand::RenameSession(rest.join(" ")))
        }
        ["name", "session", rest @ ..] if !rest.is_empty() => {
            Some(SessionCommand::RenameSession(rest.join(" ")))
        }
        _ => {
            // Direct session name match with fuzzy matching — STT often mangles words
            let transcript_clean: String = transcript.trim().to_lowercase()
                .chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
            let transcript_clean = transcript_clean.trim();
            let word_count = transcript_clean.split_whitespace().count();
            if word_count < 2 || word_count > 4 {
                return None;
            }
            let sessions = load_sessions();
            let mut best_match: Option<(String, f64)> = None;
            for s in &sessions {
                let name_clean: String = s.name.to_lowercase()
                    .chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
                let name_clean = name_clean.trim();
                let dist = levenshtein(transcript_clean, name_clean);
                let max_len = transcript_clean.len().max(name_clean.len());
                if max_len == 0 { continue; }
                let normalized = dist as f64 / max_len as f64;
                if normalized <= 0.35 {
                    if best_match.as_ref().map_or(true, |(_, d)| normalized < *d) {
                        best_match = Some((s.name.clone(), normalized));
                    }
                }
            }
            best_match.map(|(name, _)| SessionCommand::SwitchSession(name))
        }
    }
}

/// Check if the transcript is a confirmation (must be the entire utterance)
fn is_confirmation(transcript: &str) -> Option<bool> {
    let words = command_words(transcript);
    match words.iter().map(|w| w.as_str()).collect::<Vec<_>>().as_slice() {
        ["yes"] | ["yeah"] | ["yep"] | ["confirm"] | ["do", "it"] | ["yes", "sir"] => Some(true),
        ["no"] | ["nope"] | ["cancel"] | ["never", "mind"] => Some(false),
        _ => None,
    }
}

/// Check if the transcript is a voice toggle command (must be the entire utterance)
fn check_voice_command(transcript: &str) -> Option<bool> {
    let words = command_words(transcript);
    match words.iter().map(|w| w.as_str()).collect::<Vec<_>>().as_slice() {
        ["voice", "on"] | ["speech", "on"] => Some(true),
        ["voice", "off"] | ["speech", "off"] => Some(false),
        _ => None,
    }
}

/// Check if the transcript is a help command
fn check_help_command(transcript: &str) -> bool {
    let words = command_words(transcript);
    matches!(words.iter().map(|w| w.as_str()).collect::<Vec<_>>().as_slice(),
        ["help"] | ["commands"] | ["show", "help"] | ["show", "commands"])
}

#[derive(Deserialize, Debug)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize, Debug)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Deserialize, Debug)]
struct StreamDelta {
    content: Option<String>,
}

fn update_live_streaming(user: &str, partial_response: &str) {
    update_live_streaming_for(user, partial_response, &live_log_path());
}

fn update_live_streaming_for(user: &str, partial_response: &str, path: &str) {
    let content = format!(
        "## You said:
{}

### Alan:
{}

---
",
        user, partial_response
    );
    let _ = std::fs::write(path, content);
}

fn get_beep_path(name: &str) -> String {
    let filename = format!("{}.wav", name);
    // Try current working directory first (start.sh sets this)
    if std::path::Path::new(&filename).exists() {
        return filename;
    }
    // Try binary's directory
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let path = dir.join(&filename);
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
    }
    // Fallback to /tmp
    format!("/tmp/{}", filename)
}

fn play_sound(name: &str) {
    Command::new("paplay")
        .arg("--device").arg(AUDIO_SINK)
        .arg(get_beep_path(name))
        .spawn().ok();
}

fn beep_down() {
    play_sound("beep-down");
}

fn beep_up() {
    play_sound("beep-up");
}

fn beep_abort() {
    play_sound("beep-abort");
}

fn beep_busy() {
    play_sound("beep-busy");
}

fn notify() {
    play_sound("notify");
}

fn speak(text: &str) {
    Command::new("/home/jb/bin/speak").arg(text).spawn().ok();
}

/// Returns the 0-based index of the active session in the sessions list.
fn get_active_session_index() -> usize {
    let active_id = get_active_session_id();
    let sessions = load_sessions();
    sessions.iter().position(|s| s.id == active_id).unwrap_or(0)
}

/// C major scale frequencies: C4 D4 E4 F4 G4 A4 B4 C5 ...
fn session_tone_freq(session_index: usize) -> f32 {
    // C major scale semitone offsets from C4: C=0, D=2, E=4, F=5, G=7, A=9, B=11
    const SCALE: &[u32] = &[0, 2, 4, 5, 7, 9, 11];
    let octave = session_index / SCALE.len();
    let note = session_index % SCALE.len();
    let semitones = (octave * 12 + SCALE[note] as usize) as f32;
    523.25 * 2.0_f32.powf(semitones / 12.0)
}

/// Play a short startup jingle via paplay.
/// A quick ascending arpeggio that ends on the active session's note.
fn play_startup_tune(session_index: usize) {
    std::thread::spawn(move || {
        let sample_rate = 48000u32;
        let note_ms = 80u32;
        let gap_ms = 20u32;
        // C5, E5, G5, then the session note
        let mut freqs = vec![523.25_f32, 659.25, 783.99];
        let session_freq = session_tone_freq(session_index);
        freqs.push(session_freq);

        let wav_path = "/tmp/stomp-claw-startup.wav";
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: HoundFormat::Int,
        };

        if let Ok(mut writer) = WavWriter::create(wav_path, spec) {
            let fade_samples = (sample_rate as usize * 5) / 1000;
            for freq in &freqs {
                let note_samples = (sample_rate as usize * note_ms as usize) / 1000;
                for i in 0..note_samples {
                    let t = i as f32 / sample_rate as f32;
                    let mut s = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.25;
                    if i < fade_samples { s *= i as f32 / fade_samples as f32; }
                    if i >= note_samples - fade_samples { s *= (note_samples - 1 - i) as f32 / fade_samples as f32; }
                    let _ = writer.write_sample((s * i16::MAX as f32) as i16);
                }
                // Gap between notes
                let gap_samples = (sample_rate as usize * gap_ms as usize) / 1000;
                for _ in 0..gap_samples {
                    let _ = writer.write_sample(0i16);
                }
            }
            let _ = writer.finalize();
            Command::new("paplay")
                .arg("--device").arg(AUDIO_SINK)
                .arg(wav_path)
                .spawn().ok();
        }
    });
}

/// Play a short sine-wave tone at a pitch corresponding to the session index.
/// Generates a WAV file in /tmp and plays it via paplay (same as all other sounds).
/// Runs in a spawned thread so it never blocks the caller.
fn play_session_tone(session_index: usize) {
    std::thread::spawn(move || {
        let freq = session_tone_freq(session_index);
        let sample_rate = 48000u32;
        let duration_ms = 120;
        let total_samples = (sample_rate as usize * duration_ms) / 1000;
        let fade_samples = (sample_rate as usize * 10) / 1000; // 10ms fade in/out

        let wav_path = format!("/tmp/stomp-claw-tone-{}.wav", session_index);
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: HoundFormat::Int,
        };

        if let Ok(mut writer) = WavWriter::create(&wav_path, spec) {
            for i in 0..total_samples {
                let t = i as f32 / sample_rate as f32;
                let mut s = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.3;
                // Fade in
                if i < fade_samples {
                    s *= i as f32 / fade_samples as f32;
                }
                // Fade out
                if i >= total_samples - fade_samples {
                    s *= (total_samples - 1 - i) as f32 / fade_samples as f32;
                }
                let sample_i16 = (s * i16::MAX as f32) as i16;
                let _ = writer.write_sample(sample_i16);
            }
            let _ = writer.finalize();

            Command::new("paplay")
                .arg("--device").arg(AUDIO_SINK)
                .arg(&wav_path)
                .spawn().ok();
        }
    });
}

fn get_or_create_session() -> String {
    if let Ok(s) = std::fs::read_to_string(SESSION_FILE.as_str()) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    let session = format!("stomp-{}", uuid::Uuid::new_v4());
    let _ = std::fs::write(SESSION_FILE.as_str(), &session);
    session
}

fn main() {
    let _ = std::fs::create_dir_all(base_dir());
    let _ = std::fs::create_dir_all(base_dir().join("conversations"));
    let _ = std::fs::create_dir_all(LIVE_DIR.as_str());
    let _ = File::create(LOG_FILE.as_str());
    // Clean up old single live.md if it exists
    let _ = std::fs::remove_file(base_dir().join("live.md"));
    log("🎹 Stomp Claw starting...");

    let config = load_config();
    log(&format!("Voice enabled: {}", config.voice_enabled));

    migrate_sessions();
    let session = get_or_create_session();
    log(&format!("Using session: {}", session));

    // Migrate all existing monolithic .md conversation files to per-turn format
    let sessions = load_sessions();
    for s in &sessions {
        migrate_conversation(&s.id);
    }
    // Also migrate orphaned .md files not in sessions.json
    if let Ok(entries) = std::fs::read_dir(CONVERSATION_LOG_DIR.as_str()) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") {
                let session_id = name.trim_end_matches(".md");
                migrate_conversation(session_id);
            }
        }
    }

    restore_live_from_history();
    play_startup_tune(get_active_session_index());

    if let Err(e) = run(config) {
        log(&format!("Fatal error: {}", e));
    }
}

fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let recording = Arc::new(AtomicBool::new(false));
    let pedal_down = Arc::new(AtomicBool::new(false));
    let recording_start = Arc::new(Mutex::new(Option::<std::time::Instant>::None));
    let audio_data: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));

    // Config shared between threads
    let config = Arc::new(Mutex::new(config));
    // Flag to stop thinking animation thread when response arrives (or new recording starts)
    let thinking = Arc::new(AtomicBool::new(false));
    // Flag for session reset confirmation flow
    let awaiting_session_reset = Arc::new(AtomicBool::new(false));
    let abort_recording = Arc::new(AtomicBool::new(false));
    let busy_sessions: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    // Start the web viewer in a background thread
    let busy_for_viewer = busy_sessions.clone();
    std::thread::spawn(move || start_viewer(busy_for_viewer));

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No input device")?;
    log(&format!("Using input device: {}", device.name()?));
    
    let default_config = device.default_input_config()?;
    let sample_format = default_config.sample_format();
    
    let config_stream = StreamConfig {
        channels: 1,
        sample_rate: SampleRate(TARGET_SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };
    log(&format!("Recording at {}Hz (mono)", TARGET_SAMPLE_RATE));

    let rec = recording.clone();
    let audio = audio_data.clone();

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config_stream,
            move |data: &[f32], _: &_| {
                if rec.load(Ordering::Relaxed) {
                    audio.lock().unwrap().extend_from_slice(data);
                }
            },
            |err| log(&format!("Audio error: {}", err)),
            None,
        )?,
        _ => return Err("Unsupported sample format".into()),
    };

    stream.play()?;

    let recording2 = recording.clone();
    let pedal_down2 = pedal_down.clone();
    let audio2 = audio_data.clone();
    let config2 = config.clone();
    let recording_start2 = recording_start.clone();
    let thinking2 = thinking.clone();
    let awaiting_session_reset2 = awaiting_session_reset.clone();

    let busy_sessions2 = busy_sessions.clone();
    std::thread::spawn(move || {
        let abort_recording2 = abort_recording.clone();
        if let Err(e) = midi_listener(recording2, pedal_down2, audio2, config2, recording_start2, thinking2, awaiting_session_reset2, abort_recording2, busy_sessions2) {
            log(&format!("MIDI error: {}", e));
        }
    });

    loop { std::thread::sleep(std::time::Duration::from_secs(1)); }
}

fn midi_listener(
    recording: Arc<AtomicBool>,
    pedal_down: Arc<AtomicBool>,
    audio_data: Arc<Mutex<Vec<f32>>>,
    config: Arc<Mutex<Config>>,
    recording_start: Arc<Mutex<Option<std::time::Instant>>>,
    thinking: Arc<AtomicBool>,
    awaiting_session_reset: Arc<AtomicBool>,
    abort_recording: Arc<AtomicBool>,
    busy_sessions: Arc<Mutex<HashSet<String>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut midi_in = MidiInput::new("stomp_claw")?;
    midi_in.ignore(Ignore::None);

    let port = midi_in.ports().into_iter().find(|p| {
        midi_in.port_name(p).map(|n| n.contains("FS-1-WL")).unwrap_or(false)
    }).ok_or("FS-1-WL not found")?;

    log(&format!("Connected to: {}", midi_in.port_name(&port)?));

    let _conn = midi_in.connect(
        &port,
        "stomp_claw_read",
        move |_, msg, _| {
            if msg.len() >= 3 && (msg[0] & 0xF0) == 0xB0 && msg[1] == PEDAL_CC {
                if msg[2] == 127 && !pedal_down.load(Ordering::Relaxed) {
                    beep_down();
                    log("👟 PEDAL DOWN");
                    // Kill any existing thinking animation thread
                    thinking.store(false, Ordering::Relaxed);
                    pedal_down.store(true, Ordering::Relaxed);
                    recording.store(true, Ordering::Relaxed);
                    audio_data.lock().unwrap().clear();
                    // Start recording with partial transcription
                    log("🎙️ Starting recording thread");
                    let pd = pedal_down.clone();
                    let rs = recording_start.clone();
                    let audio = audio_data.clone();
                    let abort = abort_recording.clone();
                    let recording_live_path = live_log_path();
                    *rs.lock().unwrap() = Some(std::time::Instant::now());
                    std::thread::spawn(move || {
                        let client = Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap();
                        while pd.load(Ordering::Relaxed) {
                            if let Some(start) = *rs.lock().unwrap() {
                                let elapsed = start.elapsed().as_secs_f64();
                                log(&format!("⏱️ Recording: {}s", elapsed));
                                if elapsed > 0.5 {
                                    let samples = audio.lock().unwrap();
                                    if let Some(text) = send_partial_transcription(&samples, &client) {
                                        log(&format!("📝 Partial: {}", text));
                                        update_live_with_partial_for(&text, &recording_live_path);
                                        // Check for cancel keywords (full phrases)
                                        let lower = text.to_lowercase();
                                        if lower.contains("ignore this") || lower.contains("never mind") || lower.contains("forget it") || lower.contains("scratch that") {
                                            log("🛑 CANCEL keyword detected");
                                            beep_abort();
                                            update_live_cancelled_for(&recording_live_path);
                                            abort.store(true, Ordering::Relaxed);
                                            break;
                                        }
                                    }
                                } else {
                                    update_live_recording_for(elapsed, &recording_live_path);
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_millis(300));
                        }
                    });
                } else if msg[2] == 0 && pedal_down.load(Ordering::Relaxed) {
                    log("👟 PEDAL UP");
                    beep_up();
                    
                    // Check if recording was aborted
                    let was_aborted = abort_recording.load(Ordering::Relaxed);
                    abort_recording.store(false, Ordering::Relaxed);  // Reset for next time
                    
                    recording.store(false, Ordering::Relaxed);
                    pedal_down.store(false, Ordering::Relaxed);
                    
                    if was_aborted {
                        log("🛑 Recording aborted (already handled)");
                    } else {
                        let samples = audio_data.lock().unwrap().clone();
                        let config = config.clone();
                        let thinking = thinking.clone();
                        let awaiting_session_reset = awaiting_session_reset.clone();
                        let busy = busy_sessions.clone();

                        let session_id = get_active_session_id();

                        std::thread::spawn(move || {
                            let result = process(samples, config, thinking.clone(), awaiting_session_reset, busy.clone(), session_id.clone());

                            // Always clear busy flag, even on error
                            busy.lock().unwrap().remove(&session_id);

                            if let Err(e) = result {
                                thinking.store(false, Ordering::Relaxed);
                                log(&format!("Processing error: {}", e));
                                update_live("Error", &format!("Something went wrong: {}", e));
                            }
                        });
                    }
                }
            }
        },
        (),
    )?;

    log("Ready. Hold foot pedal to record.");
    loop { std::thread::sleep(std::time::Duration::from_secs(1)); }
}

fn process(samples: Vec<f32>, config: Arc<Mutex<Config>>, thinking: Arc<AtomicBool>, awaiting_session_reset: Arc<AtomicBool>, busy_sessions: Arc<Mutex<HashSet<String>>>, own_session_id: String) -> Result<(), Box<dyn std::error::Error>> {
    if samples.is_empty() { log("Empty recording"); return Ok(()); }
    // Capture the live file path for THIS session so updates go to the right file
    // even if the user switches sessions while we're processing
    let live_path = live_log_path_for(&own_session_id);
    log(&format!("Processing {} samples @ {}Hz", samples.len(), TARGET_SAMPLE_RATE));

    let tmp = NamedTempFile::new()?;
    {
        let spec = WavSpec {
            channels: 1,
            sample_rate: TARGET_SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: HoundFormat::Int,
        };
        let mut w = WavWriter::new(&tmp, spec)?;
        for s in &samples {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)?;
        }
        w.finalize()?;
    }

    let mut buf = Vec::new();
    std::fs::File::open(tmp.path())?.read_to_end(&mut buf)?;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;

    let part = reqwest::multipart::Part::bytes(buf)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        log("📡 Sending to nemo...");
        let resp = client.post(&format!("{}/transcribe/", NEMO_URL))
            .multipart(form).send().await?;

        let status = resp.status();
        let text = resp.text().await?;
        log(&format!("Nemo: status={}, text={:?}", status, text));

        if !text.trim().is_empty() {
            let transcript = text.trim().to_string();
            log(&format!("📝 Transcript: {}", transcript));

            // Handle delete session confirmation if awaiting
            if awaiting_session_reset.load(Ordering::Relaxed) {
                awaiting_session_reset.store(false, Ordering::Relaxed);
                match is_confirmation(&transcript) {
                    Some(true) => {
                        let deleted = handle_delete_session_confirmed();
                        let msg = format!("Session '{}' deleted.", deleted);
                        log(&format!("✅ {}", msg));
                        update_live("Delete session", &msg);
                        return Ok(());
                    }
                    _ => {
                        log("❌ Delete session cancelled");
                        update_live("Delete session", "Cancelled.");
                        return Ok(());
                    }
                }
            }

            // Check for session picker commands
            if let Some(cmd) = check_session_command(&transcript) {
                match cmd {
                    SessionCommand::NewSession => {
                        let name = handle_new_session();
                        let msg = format!("New session started: {}", name);
                        log(&format!("✅ {}", msg));
                        update_live("New session", &msg);
                        play_session_tone(get_active_session_index());
                    }
                    SessionCommand::SwitchSession(query) => {
                        match handle_switch_session(&query) {
                            Ok(name) => {
                                log(&format!("✅ Switched to {}", name));
                                restore_live_from_history();
                                play_session_tone(get_active_session_index());
                            }
                            Err(e) => {
                                log(&format!("❌ {}", e));
                                update_live("Switch session", &e);
                            }
                        }
                    }
                    SessionCommand::ListSessions => {
                        let msg = handle_list_sessions();
                        log(&format!("📋 {}", msg));
                        update_live("Sessions", &msg);
                    }
                    SessionCommand::RenameSession(new_name) => {
                        match handle_rename_session(&new_name) {
                            Ok(msg) => {
                                log(&format!("✅ {}", msg));
                                update_live("Rename session", &msg);
                            }
                            Err(e) => {
                                log(&format!("❌ {}", e));
                                update_live("Rename session", &e);
                            }
                        }
                    }
                    SessionCommand::DeleteSession => {
                        log("🗑️ Delete session requested, awaiting confirmation");
                        awaiting_session_reset.store(true, Ordering::Relaxed);
                        update_live(&transcript, "Delete this session? Say **yes** or **no**.");
                    }
                }
                return Ok(());
            }

            // Spawn thread to animate thinking (stops when thinking flag is cleared)
            thinking.store(true, Ordering::Relaxed);
            let user_for_thread = transcript.clone();
            let thinking_flag = thinking.clone();
            let live_path_for_thinking = live_path.clone();
            std::thread::spawn(move || {
                while thinking_flag.load(Ordering::Relaxed) {
                    update_live_thinking_for(&user_for_thread, &live_path_for_thinking);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });

            // Check for voice toggle command
            let voice_was_enabled = {
                let cfg = config.lock().unwrap();
                cfg.voice_enabled
            };
            
            if let Some(new_voice_state) = check_voice_command(&transcript) {
                let mut cfg = config.lock().unwrap();
                let _changed = cfg.voice_enabled != new_voice_state;
                cfg.voice_enabled = new_voice_state;
                save_config(&cfg);
                
                let msg = if new_voice_state {
                    "Voice enabled"
                } else {
                    "Voice disabled"
                };
                log(&format!("🔊 {}", msg));
                thinking.store(false, Ordering::Relaxed);
                if voice_was_enabled != new_voice_state {
                    speak(msg);
                }
                return Ok::<_, Box<dyn std::error::Error>>(());
            }

            // Check for help command
            if check_help_command(&transcript) {
                thinking.store(false, Ordering::Relaxed);
                log("👁️ Showing help page");
                return Ok::<_, Box<dyn std::error::Error>>(());
            }

            // Check if active session is busy before sending a normal message
            {
                let busy = busy_sessions.lock().unwrap();
                if busy.contains(&own_session_id) {
                    log("⏳ Active session still processing, rejecting message");
                    thinking.store(false, Ordering::Relaxed);
                    beep_busy();
                    update_live_for(&transcript, "Session is busy — switch to another session first.", &live_path);
                    return Ok(());
                }
            }

            // Mark this session as busy for the duration of the LLM call
            busy_sessions.lock().unwrap().insert(own_session_id.clone());

            // Normal message - send to OpenClaw with Sonnet
            let session = get_or_create_session();
            touch_active_session();
            log(&format!("📤 Sending to OpenClaw (session: {})...", session));

            let (system_prompt, max_tokens) = { 
                let cfg = config.lock().unwrap(); 
                if cfg.voice_enabled { 
                    ("You are talking to James via voice-only (foot pedal + TTS). Keep responses very short - 1-2 sentences max. Be direct and conversational. No long explanations.".to_string(), 150)
                } else { 
                    ("You are Alan, James's AI assistant.".to_string(), 2000) 
                } 
            };

            let payload = serde_json::json!({
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": &transcript}
                ],
                "stream": true,
                "max_tokens": max_tokens,
                "user": "stomp-claw"
            });

            let resp2 = client.post(OPENCLAW_URL)
                .header("Authorization", format!("Bearer {}", OPENCLAW_TOKEN))
                .header("Content-Type", "application/json")
                .header("x-openclaw-session-key", &session)
                .json(&payload)
                .send().await?;

            if !resp2.status().is_success() {
                let status = resp2.status();
                let body = resp2.text().await.unwrap_or_default();
                thinking.store(false, Ordering::Relaxed);
                log(&format!("❌ OpenClaw HTTP {}: {}", status, body));
                update_live_for(&transcript, &format!("❌ Error: HTTP {}", status), &live_path);
            } else {
                let mut full_reply = String::new();
                let mut stream = resp2.bytes_stream();
                let mut buffer = String::new();
                let mut stream_done = false;

                while let Some(chunk) = stream.next().await {
                    if stream_done { break; }
                    let chunk = chunk?;
                    buffer.push_str(&String::from_utf8_lossy(&chunk));

                    // Process complete SSE lines from the buffer
                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim().to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        if line.is_empty() || line.starts_with(':') {
                            continue;
                        }

                        if let Some(data) = line.strip_prefix("data: ") {
                            if data.trim() == "[DONE]" {
                                stream_done = true;
                                break;
                            }

                            if let Ok(parsed) = serde_json::from_str::<StreamChunk>(data) {
                                if let Some(choice) = parsed.choices.first() {
                                    if let Some(content) = &choice.delta.content {
                                        full_reply.push_str(content);
                                        thinking.store(false, Ordering::Relaxed);
                                        update_live_streaming_for(&transcript, &full_reply, &live_path);
                                    }
                                }
                            }
                        }
                    }
                }

                if full_reply.is_empty() {
                    thinking.store(false, Ordering::Relaxed);
                    log("❌ OpenClaw: empty streaming response");
                    update_live_for(&transcript, "❌ Error: empty response", &live_path);
                } else {
                    // Truncate if voice is enabled
                    let final_reply = {
                        let cfg = config.lock().unwrap();
                        if cfg.voice_enabled {
                            truncate_to_sentences(&full_reply, 2)
                        } else {
                            full_reply.clone()
                        }
                    };

                    thinking.store(false, Ordering::Relaxed);
                    log(&format!("💬 Alan: {}", final_reply));
                    update_live_for(&transcript, &final_reply, &live_path);
                    log_conversation_for(&transcript, &final_reply, &own_session_id);

                    // Clear live file after saving turn so history view doesn't duplicate
                    let _ = std::fs::write(&live_path, "Waiting for recording...\n");

                    // Speak if voice enabled, otherwise play notification chime
                    let cfg = config.lock().unwrap();
                    if cfg.voice_enabled {
                        speak(&final_reply);
                    } else {
                        notify();
                    }
                }
            }
            // Unmark session as busy
            busy_sessions.lock().unwrap().remove(&own_session_id);
        } else {
            thinking.store(false, Ordering::Relaxed);
            log("Empty transcript");
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;

    Ok(())
}
