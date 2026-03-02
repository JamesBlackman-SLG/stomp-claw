# Live Viewer Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Browser-based viewer for `/tmp/stomp-claw-live.md` with live SSE updates, dark theme.

**Architecture:** Rust HTTP server with SSE for file watching. Single HTML served with embedded JavaScript for markdown rendering.

**Tech Stack:** `rouille` (HTTP server), `notify` (file watching), embedded HTML/JS

---

### Task 1: Add dependencies to Cargo.toml

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add dependencies**

Add these dependencies to Cargo.toml:

```toml
[dependencies]
rouille = "3.6"
notify = "6.1"

[[bin]]
name = "stomp-claw-viewer"
path = "src/viewer.rs"
```

**Step 2: Verify cargo builds**

Run: `cargo build --release`
Expected: Builds successfully with new dependencies

---

### Task 2: Create the viewer binary

**Files:**
- Create: `src/viewer.rs`

**Step 1: Write viewer.rs**

```rust
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use rouille::Server;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const LIVE_FILE: &str = "/tmp/stomp-claw-live.md";
const PORT: &str = "localhost:8765";
const HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Stomp Claw Live</title>
    <script src="https://cdn.jsdelivr.net/npm/showdown@2.1.0/dist/showdown.min.js"></script>
    <style>
        * { box-sizing: border-box; }
        body {
            background: #000;
            color: #fff;
            font-family: 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
            font-size: 16px;
            line-height: 1.6;
            margin: 0;
            padding: 20px;
        }
        #content {
            max-width: 800px;
            margin: 0 auto;
            white-space: pre-wrap;
            word-wrap: break-word;
        }
        #content h1, #content h2, #content h3 {
            color: #58a6ff;
            margin-top: 1.5em;
        }
        #content code {
            background: #1a1a1a;
            padding: 2px 6px;
            border-radius: 3px;
        }
        #content pre {
            background: #1a1a1a;
            padding: 15px;
            border-radius: 5px;
            overflow-x: auto;
        }
        #content blockquote {
            border-left: 3px solid #58a6ff;
            margin-left: 0;
            padding-left: 15px;
            color: #8b949e;
        }
        #status {
            position: fixed;
            bottom: 10px;
            right: 10px;
            font-size: 12px;
            color: #666;
        }
        .connected { color: #3fb950; }
        .disconnected { color: #f85149; }
    </style>
</head>
<body>
    <div id="content">Waiting for recording...</div>
    <div id="status"><span class="disconnected">●</span> <span id="status-text">Disconnected</span></div>
    <script>
        const converter = new showdown.Converter();
        const contentEl = document.getElementById('content');
        const statusEl = document.getElementById('status-text');
        const dot = document.querySelector('#status span');

        function render(text) {
            contentEl.innerHTML = converter.makeHtml(text || 'Waiting for recording...');
        }

        function connect() {
            const evt = new EventSource('/events');

            evt.onmessage = (e) => {
                render(e.data);
            };

            evt.onopen = () => {
                statusEl.textContent = 'Connected';
                dot.className = 'connected';
            };

            evt.onerror = () => {
                statusEl.textContent = 'Reconnecting...';
                dot.className = 'disconnected';
                evt.close();
                setTimeout(connect, 1000);
            };
        }

        connect();
    </script>
</body>
</html>"#;

fn main() {
    println!("Starting Stomp Claw Viewer on http://localhost:8765");

    let (tx, rx) = channel::<PathBuf>();

    // Spawn file watcher thread
    thread::spawn(move || {
        let (watcher_tx, watcher_rx) = channel::<notify::Result<notify::RecommendedEvent>>();

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

    // Track current content to detect changes
    let current_content: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    let server = Server::http(PORT, move |request| {
        let current_content = current_content.clone();

        rouille::router!(request,
            (GET) ["/"] => {
                rouille::Response::from_html("text/html", HTML)
            },
            (GET) ["/events"] => {
                // SSE response
                let content = current_content.clone();

                rouille::Response::from_stream(
                    "text/event-stream",
                    Box::new(move || {
                        let (sender, receiver) = std::sync::mpsc::channel();

                        // Send initial content
                        let initial = fs::read_to_string(LIVE_FILE).unwrap_or_default();
                        let _ = sender.send(format!("data: {}\n\n", escape_sse(&initial)));

                        // Spawn thread to watch for changes
                        let tx_clone = sender.clone();
                        thread::spawn(move || {
                            while let Ok(path) = rx.recv() {
                                if let Ok(contents) = fs::read_to_string(&path) {
                                    let _ = tx_clone.send(format!("data: {}\n\n", escape_sse(&contents)));
                                }
                            }
                        });

                        // Stream messages
                        std::iter::from_fn(move || {
                            receiver.recv().ok()
                        })
                    }),
                ).with附加_headers(vec![
                    ("Cache-Control", "no-cache"),
                    ("Connection", "keep-alive"),
                ])
            },
            _ => rouille::Response::from_404().with_body("Not Found".as_bytes().to_vec()),
        )
    });

    server.run();
}

fn escape_sse(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
```

**Step 2: Verify compilation**

Run: `cargo build --release 2>&1 | head -20`
Expected: Compiles without errors

---

### Task 3: Add startup script

**Files:**
- Create: `start-viewer.sh`

**Step 1: Create start script**

```bash
#!/bin/bash
cargo run --release --bin stomp-claw-viewer
```

**Step 2: Make it executable**

Run: `chmod +x start-viewer.sh`

---

### Task 4: Test the viewer

**Step 1: Start the viewer**

Run: `./start-viewer.sh &`

**Step 2: Check it's running**

Run: `curl -s http://localhost:8765 | head -5`
Expected: Returns HTML with "Stomp Claw Live"

**Step 3: Open in browser**

Open: `http://localhost:8765`

**Step 4: Test live update**

Run: `echo "# Test\nNew content" > /tmp/stomp-claw-live.md`
Expected: Browser updates to show "Test" and "New content"

**Step 5: Stop viewer**

Run: `pkill -f stomp-claw-viewer`

---

### Task 5: Commit

Run:
```bash
git add Cargo.toml src/viewer.rs start-viewer.sh docs/plans/2026-03-02-live-viewer-design.md
git commit -m "feat: add live viewer with SSE for markdown display"
```
