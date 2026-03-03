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
use std::sync::{Arc, Mutex};
use futures::StreamExt;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use tempfile::NamedTempFile;

const LOG_FILE: &str = "/tmp/stomp-claw.log";
const CONVERSATION_LOG_DIR: &str = "/tmp/stomp-claw-conversations";
const LIVE_LOG: &str = "/tmp/stomp-claw-live.md";
const PEDAL_CC: u8 = 85;
const NEMO_URL: &str = "http://localhost:5051";
const TARGET_SAMPLE_RATE: u32 = 16000;
const OPENCLAW_URL: &str = "http://127.0.0.1:18789/v1/chat/completions";
const OPENCLAW_TOKEN: &str = "06b21a7fafad855670f81018f3a455edccaf5dedc470fa0b";
const SESSION_FILE: &str = "/tmp/stomp-claw-session.txt";
const CONFIG_FILE: &str = "/home/jb/.config/stomp-claw/config.toml";
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

struct ViewerFileReader {
    last_content: String,
    first_read: bool,
}

impl ViewerFileReader {
    fn new() -> Self {
        let initial = fs::read_to_string(LIVE_LOG).unwrap_or_else(|_| "Waiting for recording...".to_string());
        Self { last_content: initial, first_read: true }
    }
}

impl std::io::Read for ViewerFileReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let content = fs::read_to_string(LIVE_LOG)
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

        let path = PathBuf::from(LIVE_LOG);
        if let Some(parent) = path.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }

        loop {
            if let Ok(Ok(event)) = watcher_rx.recv_timeout(std::time::Duration::from_millis(500)) {
                if event.paths.iter().any(|p| p.to_string_lossy() == LIVE_LOG) {
                    let _ = tx.send(PathBuf::from(LIVE_LOG));
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

fn load_config() -> Config {
    if let Ok(content) = std::fs::read_to_string(CONFIG_FILE) {
        if let Ok(c) = toml::from_str(&content) {
            return c;
        }
    }
    Config::default()
}

fn save_config(config: &Config) {
    if let Ok(content) = toml::to_string(config) {
        let _ = std::fs::write(CONFIG_FILE, content);
    }
}

fn log(msg: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG_FILE) {
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
        "## 🎙️ Recording... (transcribing)

{}

---
",
        text
    );
    let _ = std::fs::write(LIVE_LOG, content);
}

fn update_live_recording(seconds: f64) {
    let dots = ".".repeat((seconds as usize / 2) % 4);
    let content = format!(
        "## 🎙️ Recording{} ({}s)

Release pedal to transcribe... (say \"ignore this\" to cancel)
---
",
        dots, seconds
    );
    let _ = std::fs::write(LIVE_LOG, content);
}

fn update_live_cancelled() {
    let content = "## ❌ Transcription cancelled by user

---
".to_string();
    let _ = std::fs::write(LIVE_LOG, content);
}

fn update_live_thinking(user: &str) {
    let dots = get_thinking_dots();
    let content = format!(
        "## You said:
{}

### Alan is thinking{}
---
",
        user, dots
    );
    let _ = std::fs::write(LIVE_LOG, content);
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
    let _ = std::fs::write(LIVE_LOG, content);
}

fn conversation_log_path() -> String {
    let session = std::fs::read_to_string(SESSION_FILE).unwrap_or_else(|_| "unknown".to_string()).trim().to_string();
    let _ = std::fs::create_dir_all(CONVERSATION_LOG_DIR);
    format!("{}/{}.md", CONVERSATION_LOG_DIR, session)
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

/// Check if the transcript is a session reset command (must be the entire utterance)
fn is_session_reset_command(transcript: &str) -> bool {
    let words = command_words(transcript);
    matches!(words.iter().map(|w| w.as_str()).collect::<Vec<_>>().as_slice(),
        ["new", "session"]
        | ["reset", "session"] | ["reset", "context"]
        | ["clear", "session"] | ["clear", "context"]
        | ["start", "over"]
        | ["fresh", "start"]
    )
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

fn reset_session() -> String {
    let session = format!("stomp-{}", uuid::Uuid::new_v4());
    let _ = std::fs::write(SESSION_FILE, &session);
    log(&format!("🔄 New session created: {}", session));
    session
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

const VIEW_FILE: &str = "/tmp/stomp-claw-view.txt";

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
    let _ = std::fs::write(VIEW_FILE, view);
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
    let _ = std::fs::write(LIVE_LOG, content);
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
    if let Ok(s) = std::fs::read_to_string(SESSION_FILE) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    let session = format!("stomp-{}", uuid::Uuid::new_v4());
    let _ = std::fs::write(SESSION_FILE, &session);
    session
}

fn main() {
    let _ = File::create(LOG_FILE);
    let _ = std::fs::write(LIVE_LOG, "Hold the pedal and speak...\n");
    log("🎹 Stomp Claw starting...");
    
    let config = load_config();
    log(&format!("Voice enabled: {}", config.voice_enabled));
    
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

            // Handle session reset confirmation if awaiting
            if awaiting_session_reset.load(Ordering::Relaxed) {
                awaiting_session_reset.store(false, Ordering::Relaxed);
                match is_confirmation(&transcript) {
                    Some(true) => {
                        let session = reset_session();
                        let msg = format!("New session started: {}", &session[..session.len().min(20)]);
                        log(&format!("✅ {}", msg));
                        update_live("New session", &msg);
                        speak("New session started.");
                        return Ok(());
                    }
                    _ => {
                        log("❌ Session reset cancelled");
                        update_live("Session reset", "Cancelled.");
                        speak("Cancelled.");
                        return Ok(());
                    }
                }
            }

            // Check for session reset command
            if is_session_reset_command(&transcript) {
                log("🔄 Session reset requested, awaiting confirmation");
                awaiting_session_reset.store(true, Ordering::Relaxed);
                update_live(&transcript, "Start a new session? Press pedal and say **yes** or **no**.");
                speak("Start a new session? Say yes or no.");
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
