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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use futures::StreamExt;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use tempfile::NamedTempFile;

fn base_dir() -> PathBuf {
    dirs::home_dir().expect("No home directory found").join(".stomp-claw")
}

static LOG_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("stomp-claw.log").to_string_lossy().to_string());
static CONVERSATION_LOG_DIR: LazyLock<String> = LazyLock::new(|| base_dir().join("conversations").to_string_lossy().to_string());
static LIVE_LOG: LazyLock<String> = LazyLock::new(|| base_dir().join("live.md").to_string_lossy().to_string());
static SESSION_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("session.txt").to_string_lossy().to_string());
static CONFIG_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("config.toml").to_string_lossy().to_string());
static VIEW_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("view.txt").to_string_lossy().to_string());
static SESSIONS_FILE: LazyLock<String> = LazyLock::new(|| base_dir().join("sessions.json").to_string_lossy().to_string());

const PEDAL_CC: u8 = 85;
const NEMO_URL: &str = "http://localhost:5051";
const TARGET_SAMPLE_RATE: u32 = 16000;
const OPENCLAW_URL: &str = "http://127.0.0.1:18789/v1/chat/completions";
const OPENCLAW_TOKEN: &str = "06b21a7fafad855670f81018f3a455edccaf5dedc470fa0b";
const AUDIO_SINK: &str = "alsa_output.pci-0000_0d_00.4.analog-stereo";
const VIEWER_PORT: &str = "localhost:8765";

const VIEWER_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
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
        #tabs {
            position: sticky;
            top: 0;
            z-index: 10;
            background: #161b22;
            border-bottom: 1px solid #30363d;
            display: flex;
            padding: 0 20px;
        }
        .tab {
            padding: 12px 24px;
            cursor: pointer;
            color: #8b949e;
            border-bottom: 2px solid transparent;
            transition: all 0.15s;
            user-select: none;
            font-size: 14px;
            font-family: inherit;
        }
        .tab:hover { color: #c9d1d9; }
        .tab.active {
            color: #58a6ff;
            border-bottom-color: #58a6ff;
        }
        .tab .dot {
            display: inline-block;
            width: 8px;
            height: 8px;
            border-radius: 50%;
            margin-right: 8px;
        }
        .tab .dot.live { background: #3fb950; animation: pulse 2s infinite; }
        .tab .dot.history { background: #8b949e; }
        @keyframes pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.4; }
        }
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
    </style>
</head>
<body>
    <div id="tabs">
        <div class="tab active" data-view="live" onclick="switchTab('live')">
            <span class="dot live"></span>Live
        </div>
        <div class="tab" data-view="history" onclick="switchTab('history')">
            <span class="dot history"></span>History
        </div>
    </div>
    <div id="session-bar"></div>
    <div id="content">Waiting for recording...</div>
    <div id="status"><span class="disconnected">●</span> <span id="status-text">Disconnected</span></div>
    <script>
        const contentEl = document.getElementById('content');
        const statusEl = document.getElementById('status-text');
        const dot = document.querySelector('#status span');

        let currentView = 'live';
        let liveContent = 'Waiting for recording...';
        let historyContent = '';
        let eventSource = null;

        function render(text) {
            if (!text || text.trim() === '') {
                text = currentView === 'live' ? 'Waiting for recording...' : 'No history yet.';
            }

            const lines = text.split('\\n');
            let html = '';
            let section = 'none';

            for (let line of lines) {
                line = line.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

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

            contentEl.innerHTML = html;
        }

        function switchTab(view) {
            currentView = view;
            document.querySelectorAll('.tab').forEach(t => {
                t.classList.toggle('active', t.dataset.view === view);
            });

            // Sync back so voice commands and manual clicks stay in agreement
            fetch('/view/set?v=' + view).catch(() => {});

            if (view === 'live') {
                render(liveContent);
            } else {
                fetchHistory();
            }
        }

        function fetchHistory() {
            fetch('/history')
                .then(r => r.text())
                .then(text => {
                    // Escape newlines to match SSE format so render() works the same
                    historyContent = text.replace(/\\/g, '\\\\').replace(/\n/g, '\\n');
                    if (currentView === 'history') {
                        render(historyContent);
                        window.scrollTo(0, document.body.scrollHeight);
                    }
                });
        }

        function connect() {
            eventSource = new EventSource('/events');

            eventSource.onmessage = (e) => {
                liveContent = e.data;
                if (currentView === 'live') {
                    render(liveContent);
                }
            };

            eventSource.onopen = () => {
                statusEl.textContent = 'Connected';
                dot.className = 'connected';
            };

            eventSource.onerror = () => {
                statusEl.textContent = 'Reconnecting...';
                dot.className = 'disconnected';
                eventSource.close();
                setTimeout(connect, 1000);
            };
        }

        // Refresh history periodically when on history tab
        setInterval(() => {
            if (currentView === 'history') fetchHistory();
        }, 5000);

        // Poll for voice-triggered view switches
        setInterval(() => {
            fetch('/view')
                .then(r => r.text())
                .then(view => {
                    view = view.trim();
                    if (view && view !== currentView) {
                        switchTab(view);
                    }
                })
                .catch(() => {});
        }, 500);

        // Session bar
        const sessionBar = document.getElementById('session-bar');
        let lastSessionJson = '';

        function renderSessions(sessions) {
            sessionBar.innerHTML = '';
            for (const s of sessions) {
                const btn = document.createElement('button');
                btn.className = 'session-btn' + (s.active ? ' active' : '');
                btn.textContent = s.name;
                btn.onclick = () => {
                    if (!s.active) {
                        fetch('/session/switch?id=' + encodeURIComponent(s.id), {method:'POST'})
                            .then(() => fetchSessions());
                    }
                };
                sessionBar.appendChild(btn);
            }
            const newBtn = document.createElement('button');
            newBtn.className = 'session-btn new-btn';
            newBtn.textContent = '+ New';
            newBtn.onclick = () => {
                fetch('/session/new', {method:'POST'})
                    .then(() => fetchSessions());
            };
            sessionBar.appendChild(newBtn);
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
    </script>
</body>
</html>"#;

struct ViewerFileReader {
    last_content: String,
    first_read: bool,
}

impl ViewerFileReader {
    fn new() -> Self {
        let initial = fs::read_to_string(LIVE_LOG.as_str()).unwrap_or_else(|_| "Waiting for recording...".to_string());
        Self { last_content: initial, first_read: true }
    }
}

impl std::io::Read for ViewerFileReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let content = fs::read_to_string(LIVE_LOG.as_str())
            .unwrap_or_else(|_| "Waiting for recording...".to_string());

        if self.first_read || content != self.last_content {
            self.first_read = false;
            self.last_content = content.clone();
            let msg = format!("data: {}\n\n", escape_sse(&content));
            let bytes = msg.as_bytes();
            let to_copy = std::cmp::min(buf.len(), bytes.len());
            buf[..to_copy].copy_from_slice(&bytes[..to_copy]);
            return Ok(to_copy);
        }

        let heartbeat = ": heartbeat\n\n";
        let bytes = heartbeat.as_bytes();
        let to_copy = std::cmp::min(buf.len(), bytes.len());
        buf[..to_copy].copy_from_slice(&bytes[..to_copy]);
        Ok(to_copy)
    }
}

fn escape_sse(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
}

fn start_viewer() {
    log("🌐 Starting viewer on http://localhost:8765");

    let tx = std::sync::mpsc::channel::<PathBuf>().0;

    std::thread::spawn(move || {
        let (watcher_tx, watcher_rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();

        let mut watcher = RecommendedWatcher::new(
            move |res| { let _ = watcher_tx.send(res); },
            NotifyConfig::default().with_poll_interval(std::time::Duration::from_secs(1)),
        ).unwrap();

        let path = PathBuf::from(LIVE_LOG.as_str());
        if let Some(parent) = path.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }

        loop {
            if let Ok(Ok(event)) = watcher_rx.recv_timeout(std::time::Duration::from_millis(500)) {
                if event.paths.iter().any(|p| p.to_string_lossy() == *LIVE_LOG) {
                    let _ = tx.send(PathBuf::from(LIVE_LOG.as_str()));
                }
            }
        }
    });

    let server = Server::new(VIEWER_PORT, move |request| {
        rouille::router!(request,
            (GET) ["/"] => {
                rouille::Response::html(VIEWER_HTML)
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
            (GET) ["/view"] => {
                let view = fs::read_to_string(VIEW_FILE.as_str())
                    .unwrap_or_else(|_| "live".to_string())
                    .trim().to_string();
                rouille::Response::text(view)
            },
            (GET) ["/view/set"] => {
                if let Some(v) = request.get_param("v") {
                    if v == "live" || v == "history" {
                        let _ = fs::write(VIEW_FILE.as_str(), &v);
                    }
                }
                rouille::Response::text("ok")
            },
            (GET) ["/history"] => {
                let session = fs::read_to_string(SESSION_FILE.as_str())
                    .unwrap_or_else(|_| "unknown".to_string())
                    .trim().to_string();
                let path = format!("{}/{}.md", *CONVERSATION_LOG_DIR, session);
                let content = fs::read_to_string(&path)
                    .unwrap_or_else(|_| "No history for this session yet.".to_string());
                rouille::Response::text(content)
            },
            (GET) ["/sessions"] => {
                let sessions = load_sessions();
                let active_id = get_active_session_id();
                let items: Vec<serde_json::Value> = sessions.iter().map(|s| {
                    serde_json::json!({
                        "id": s.id,
                        "name": s.name,
                        "active": s.id == active_id,
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
                name: "Session 1".to_string(),
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
        name: "Session 1".to_string(),
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
    let next_num = sessions.len() + 1;
    let session = Session {
        id: format!("stomp-{}", uuid::Uuid::new_v4()),
        name: format!("Session {}", next_num),
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
            name: "Session 1".to_string(),
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
    let content = format!(
        "{}## 🎙️ Recording... (transcribing)

{}

---
",
        session_header(), text
    );
    let _ = std::fs::write(LIVE_LOG.as_str(), content);
}

fn update_live_recording(seconds: f64) {
    let dots = ".".repeat((seconds as usize / 2) % 4);
    let content = format!(
        "{}## 🎙️ Recording{} ({}s)

Release pedal to transcribe... (say \"ignore this\" to cancel)
---
",
        session_header(), dots, seconds
    );
    let _ = std::fs::write(LIVE_LOG.as_str(), content);
}

fn update_live_cancelled() {
    let content = format!("{}## ❌ Transcription cancelled by user

---
", session_header());
    let _ = std::fs::write(LIVE_LOG.as_str(), content);
}

fn update_live_thinking(user: &str) {
    let dots = get_thinking_dots();
    let content = format!(
        "{}## You said:
{}

### Alan is thinking{}
---
",
        session_header(), user, dots
    );
    let _ = std::fs::write(LIVE_LOG.as_str(), content);
}

fn update_live(user: &str, assistant: &str) {
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
    let _ = std::fs::write(LIVE_LOG.as_str(), content);
}

fn conversation_log_path() -> String {
    let session = std::fs::read_to_string(SESSION_FILE.as_str()).unwrap_or_else(|_| "unknown".to_string()).trim().to_string();
    let _ = std::fs::create_dir_all(CONVERSATION_LOG_DIR.as_str());
    format!("{}/{}.md", *CONVERSATION_LOG_DIR, session)
}

fn log_conversation(user: &str, assistant: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let path = conversation_log_path();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "## {} - You said:\n{}\n\n### Alan replied:\n{}\n---", timestamp, user, assistant);
    }
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
        _ => None,
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

/// Check if the transcript is a viewer switch command
fn check_view_command(transcript: &str) -> Option<&'static str> {
    let words = command_words(transcript);
    match words.iter().map(|w| w.as_str()).collect::<Vec<_>>().as_slice() {
        ["live", "view"] | ["show", "live"] | ["view", "live"] | ["live"] => Some("live"),
        ["history", "view"] | ["show", "history"] | ["view", "history"] | ["history"] => Some("history"),
        _ => None,
    }
}

fn switch_view(view: &str) {
    let _ = std::fs::write(VIEW_FILE.as_str(), view);
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
    let content = format!(
        "## You said:
{}

### Alan:
{}

---
",
        user, partial_response
    );
    let _ = std::fs::write(LIVE_LOG.as_str(), content);
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
    let _ = File::create(LOG_FILE.as_str());
    let _ = std::fs::write(LIVE_LOG.as_str(), "Hold the pedal and speak...\n");
    log("🎹 Stomp Claw starting...");
    
    let config = load_config();
    log(&format!("Voice enabled: {}", config.voice_enabled));
    
    migrate_sessions();
    let session = get_or_create_session();
    log(&format!("Using session: {}", session));

    if let Err(e) = run(config) {
        log(&format!("Fatal error: {}", e));
    }
}

fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    // Start the web viewer in a background thread
    std::thread::spawn(|| start_viewer());

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
    let processing = Arc::new(AtomicBool::new(false));

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

    std::thread::spawn(move || {
        let abort_recording2 = abort_recording.clone();
        let processing2 = processing.clone();
        if let Err(e) = midi_listener(recording2, pedal_down2, audio2, config2, recording_start2, thinking2, awaiting_session_reset2, abort_recording2, processing2) {
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
    processing: Arc<AtomicBool>,
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
                    // Check if already processing
                    if processing.load(Ordering::Relaxed) {
                        log("⏳ Still processing, playing busy beep");
                        beep_busy();
                        return;
                    }
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
                                        update_live_with_partial(&text);
                                        // Check for cancel keywords (full phrases)
                                        let lower = text.to_lowercase();
                                        if lower.contains("ignore this") || lower.contains("never mind") || lower.contains("forget it") || lower.contains("scratch that") {
                                            log("🛑 CANCEL keyword detected");
                                            beep_abort();
                                            update_live_cancelled();
                                            abort.store(true, Ordering::Relaxed);
                                            break;
                                        }
                                    }
                                } else {
                                    update_live_recording(elapsed);
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
                        let processing_clone = processing.clone();
                        
                        // Mark as processing
                        processing_clone.store(true, Ordering::Relaxed);
                        
                        std::thread::spawn(move || {
                            let result = process(samples, config, thinking.clone(), awaiting_session_reset);
                            // Mark as done processing
                            processing_clone.store(false, Ordering::Relaxed);
                            
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

fn process(samples: Vec<f32>, config: Arc<Mutex<Config>>, thinking: Arc<AtomicBool>, awaiting_session_reset: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
    if samples.is_empty() { log("Empty recording"); return Ok(()); }
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
                        speak(&msg);
                        return Ok(());
                    }
                    _ => {
                        log("❌ Delete session cancelled");
                        update_live("Delete session", "Cancelled.");
                        speak("Cancelled.");
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
                    }
                    SessionCommand::SwitchSession(query) => {
                        match handle_switch_session(&query) {
                            Ok(name) => {
                                let msg = format!("Switched to {}", name);
                                log(&format!("✅ {}", msg));
                                update_live("Switch session", &msg);
                            }
                            Err(e) => {
                                log(&format!("❌ {}", e));
                                update_live("Switch session", &e);
                                speak(&e);
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
                                speak(&e);
                            }
                        }
                    }
                    SessionCommand::DeleteSession => {
                        log("🗑️ Delete session requested, awaiting confirmation");
                        awaiting_session_reset.store(true, Ordering::Relaxed);
                        update_live(&transcript, "Delete this session? Say **yes** or **no**.");
                        speak("Delete this session? Say yes or no.");
                    }
                }
                return Ok(());
            }

            // Spawn thread to animate thinking (stops when thinking flag is cleared)
            thinking.store(true, Ordering::Relaxed);
            let user_for_thread = transcript.clone();
            let thinking_flag = thinking.clone();
            std::thread::spawn(move || {
                while thinking_flag.load(Ordering::Relaxed) {
                    update_live_thinking(&user_for_thread);
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

            // Check for view switch command
            if let Some(view) = check_view_command(&transcript) {
                switch_view(view);
                let msg = format!("Switched to {} view.", view);
                log(&format!("👁️ {}", msg));
                thinking.store(false, Ordering::Relaxed);
                update_live(&transcript, &msg);
                speak(&msg);
                return Ok::<_, Box<dyn std::error::Error>>(());
            }

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
                update_live(&transcript, &format!("❌ Error: HTTP {}", status));
            } else {
                let mut full_reply = String::new();
                let mut stream = resp2.bytes_stream();
                let mut buffer = String::new();

                while let Some(chunk) = stream.next().await {
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
                                break;
                            }

                            if let Ok(parsed) = serde_json::from_str::<StreamChunk>(data) {
                                if let Some(choice) = parsed.choices.first() {
                                    if let Some(content) = &choice.delta.content {
                                        full_reply.push_str(content);
                                        thinking.store(false, Ordering::Relaxed);
                                        update_live_streaming(&transcript, &full_reply);
                                    }
                                }
                            }
                        }
                    }
                }

                if full_reply.is_empty() {
                    thinking.store(false, Ordering::Relaxed);
                    log("❌ OpenClaw: empty streaming response");
                    update_live(&transcript, "❌ Error: empty response");
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
                    update_live(&transcript, &final_reply);
                    log_conversation(&transcript, &final_reply);

                    // Speak if voice enabled, otherwise play notification chime
                    let cfg = config.lock().unwrap();
                    if cfg.voice_enabled {
                        speak(&final_reply);
                    } else {
                        notify();
                    }
                }
            }
        } else {
            thinking.store(false, Ordering::Relaxed);
            log("Empty transcript");
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;

    Ok(())
}
