use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use rouille::Server;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

const LIVE_FILE: &str = "/tmp/stomp-claw-live.md";
const SESSION_FILE: &str = "/tmp/stomp-claw-session.txt";
const CONVERSATION_LOG_DIR: &str = "/tmp/stomp-claw-conversations";
const VIEW_FILE: &str = "/tmp/stomp-claw-view.txt";
const PORT: &str = "localhost:8765";

const HTML: &str = r#"<!DOCTYPE html>
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

        connect();
    </script>
</body>
</html>"#;

struct FileReader {
    last_content: String,
    first_read: bool,
}

impl FileReader {
    fn new() -> Self {
        let initial = fs::read_to_string(LIVE_FILE).unwrap_or_else(|_| "Waiting for recording...".to_string());
        Self { last_content: initial, first_read: true }
    }
}

impl Read for FileReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Read current file content
        let content = fs::read_to_string(LIVE_FILE)
            .unwrap_or_else(|_| "Waiting for recording...".to_string());

        // Always send content on first read, or if content changed
        if self.first_read || content != self.last_content {
            self.first_read = false;
            self.last_content = content.clone();
            let msg = format!("data: {}\n\n", escape_sse(&content));
            let bytes = msg.as_bytes();
            let to_copy = std::cmp::min(buf.len(), bytes.len());
            buf[..to_copy].copy_from_slice(&bytes[..to_copy]);
            return Ok(to_copy);
        }

        // No new data - return a heartbeat comment to keep connection alive
        let heartbeat = ": heartbeat\n\n";
        let bytes = heartbeat.as_bytes();
        let to_copy = std::cmp::min(buf.len(), bytes.len());
        buf[..to_copy].copy_from_slice(&bytes[..to_copy]);
        Ok(to_copy)
    }
}

fn main() {
    println!("Starting Stomp Claw Viewer on http://localhost:8765");

    let tx = channel::<PathBuf>().0;

    // Spawn file watcher thread (for future use if needed)
    thread::spawn(move || {
        let (watcher_tx, watcher_rx) = channel::<notify::Result<notify::Event>>();

        let mut watcher = RecommendedWatcher::new(
            move |res| { let _ = watcher_tx.send(res); },
            Config::default().with_poll_interval(Duration::from_secs(1)),
        ).unwrap();

        let path = PathBuf::from(LIVE_FILE);
        if let Some(parent) = path.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }

        loop {
            if let Ok(Ok(event)) = watcher_rx.recv_timeout(Duration::from_millis(500)) {
                if event.paths.iter().any(|p| p.to_string_lossy() == LIVE_FILE) {
                    let _ = tx.send(PathBuf::from(LIVE_FILE));
                }
            }
        }
    });

    let server = Server::new(PORT, move |request| {
        rouille::router!(request,
            (GET) ["/"] => {
                rouille::Response::html(HTML)
            },
            (GET) ["/events"] => {
                let reader = FileReader::new();

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
                let view = fs::read_to_string(VIEW_FILE)
                    .unwrap_or_else(|_| "live".to_string())
                    .trim().to_string();
                rouille::Response::text(view)
            },
            (GET) ["/view/set"] => {
                if let Some(v) = request.get_param("v") {
                    if v == "live" || v == "history" {
                        let _ = fs::write(VIEW_FILE, &v);
                    }
                }
                rouille::Response::text("ok")
            },
            (GET) ["/history"] => {
                let session = fs::read_to_string(SESSION_FILE)
                    .unwrap_or_else(|_| "unknown".to_string())
                    .trim().to_string();
                let path = format!("{}/{}.md", CONVERSATION_LOG_DIR, session);
                let content = fs::read_to_string(&path)
                    .unwrap_or_else(|_| "No history for this session yet.".to_string());
                rouille::Response::text(content)
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
    }).expect("Failed to create server");

    server.run();
}

fn escape_sse(s: &str) -> String {
    // Escape for SSE: escape backslash first, then escape newlines as \n
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
}
