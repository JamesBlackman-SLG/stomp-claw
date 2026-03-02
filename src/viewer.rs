use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use rouille::Server;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
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

// Custom reader that streams SSE messages from a channel
struct SseReader {
    receiver: Arc<Mutex<Receiver<String>>>,
    buffer: Vec<u8>,
}

impl SseReader {
    fn new(receiver: Arc<Mutex<Receiver<String>>>) -> Self {
        Self {
            receiver,
            buffer: Vec::new(),
        }
    }
}

impl Read for SseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // If we have data in the buffer, return it
        if !self.buffer.is_empty() {
            let to_read = std::cmp::min(buf.len(), self.buffer.len());
            buf[..to_read].copy_from_slice(&self.buffer[..to_read]);
            self.buffer.drain(..to_read);
            return Ok(to_read);
        }

        // Try to receive a new message
        let receiver = self.receiver.lock().unwrap();
        match receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(msg) => {
                self.buffer.extend_from_slice(msg.as_bytes());
                let to_read = std::cmp::min(buf.len(), self.buffer.len());
                buf[..to_read].copy_from_slice(&self.buffer[..to_read]);
                self.buffer.drain(..to_read);
                Ok(to_read)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Send a comment line to keep connection alive
                let heartbeat = b": heartbeat\n\n";
                let to_read = std::cmp::min(buf.len(), heartbeat.len());
                buf[..to_read].copy_from_slice(&heartbeat[..to_read]);
                Ok(to_read)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Ok(0)
            }
        }
    }
}

fn main() {
    println!("Starting Stomp Claw Viewer on http://localhost:8765");

    let tx = channel::<PathBuf>().0;

    // Spawn file watcher thread
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

    // Create a channel for SSE messages that's separate from the file watcher
    let (sse_tx, sse_rx) = std::sync::mpsc::channel::<String>();
    let sse_receiver = Arc::new(Mutex::new(sse_rx));
    let sse_sender = Arc::new(sse_tx);

    // Spawn thread to watch for file changes and send SSE messages
    let tx_clone = sse_sender.clone();
    thread::spawn(move || {
        // Send initial content
        let initial = fs::read_to_string(LIVE_FILE).unwrap_or_else(|_| "Waiting for recording...".to_string());
        let _ = tx_clone.send(format!("data: {}\n\n", escape_sse(&initial)));

        // We can't easily share the file watcher, so let's just poll the file directly
        loop {
            thread::sleep(Duration::from_millis(500));
            if let Ok(contents) = fs::read_to_string(LIVE_FILE) {
                let _ = tx_clone.send(format!("data: {}\n\n", escape_sse(&contents)));
            }
        }
    });

    let server = Server::new(PORT, move |request| {
        let sse_receiver = sse_receiver.clone();

        rouille::router!(request,
            (GET) ["/"] => {
                rouille::Response::html(HTML)
            },
            (GET) ["/events"] => {
                // Clone the receiver for this request
                let receiver = sse_receiver.clone();
                let reader = SseReader::new(receiver);

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
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
