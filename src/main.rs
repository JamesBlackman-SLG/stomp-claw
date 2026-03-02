use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};
use hound::{SampleFormat as HoundFormat, WavSpec, WavWriter};
use midir::{Ignore, MidiInput};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;

const LOG_FILE: &str = "/tmp/stomp-claw.log";
const CONVERSATION_LOG: &str = "/tmp/stomp-claw-conversation.md";
const LIVE_LOG: &str = "/tmp/stomp-claw-live.md";
const PEDAL_CC: u8 = 85;
const NEMO_URL: &str = "http://localhost:5051";
const TARGET_SAMPLE_RATE: u32 = 16000;
const OPENCLAW_URL: &str = "http://127.0.0.1:18789/v1/chat/completions";
const OPENCLAW_TOKEN: &str = "06b21a7fafad855670f81018f3a455edccaf5dedc470fa0b";
const SESSION_FILE: &str = "/tmp/stomp-claw-session.txt";
const CONFIG_FILE: &str = "/home/jb/.config/stomp-claw/config.toml";
const AUDIO_SINK: &str = "alsa_output.pci-0000_0d_00.4.analog-stereo";

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

Release pedal to transcribe...
---
",
        dots, seconds
    );
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

fn log_conversation(user: &str, assistant: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(CONVERSATION_LOG) {
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

#[derive(Deserialize, Debug)]
struct OpenClawResponse {
    choices: Vec<OpenClawChoice>,
}

#[derive(Deserialize, Debug)]
struct OpenClawChoice {
    message: OpenClawMessage,
}

#[derive(Deserialize, Debug)]
struct OpenClawMessage {
    content: String,
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
        if let Err(e) = midi_listener(recording2, pedal_down2, audio2, config2, recording_start2, thinking2, awaiting_session_reset2) {
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
                    recording.store(false, Ordering::Relaxed);
                    pedal_down.store(false, Ordering::Relaxed);
                    let samples = audio_data.lock().unwrap().clone();
                    let config = config.clone();
                    let thinking = thinking.clone();
                    let awaiting_session_reset = awaiting_session_reset.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = process(samples, config, thinking.clone(), awaiting_session_reset) {
                            thinking.store(false, Ordering::Relaxed);
                            log(&format!("Processing error: {}", e));
                            update_live("Error", &format!("Something went wrong: {}", e));
                        }
                    });
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
                "stream": false,
                "max_tokens": max_tokens,
                "user": "stomp-claw"
            });

            let resp2 = client.post(OPENCLAW_URL)
                .header("Authorization", format!("Bearer {}", OPENCLAW_TOKEN))
                .header("Content-Type", "application/json")
                .header("x-openclaw-session-key", &session)
                .json(&payload)
                .send().await?;

            let reply_text = resp2.text().await?;
            log(&format!("OpenClaw raw: {}", &reply_text[..reply_text.len().min(200)]));

            match serde_json::from_str::<OpenClawResponse>(&reply_text) {
                Ok(parsed) if !parsed.choices.is_empty() => {
                    let full_reply = &parsed.choices[0].message.content;

                    // Only truncate if voice is enabled
                    let final_reply = {
                        let cfg = config.lock().unwrap();
                        if cfg.voice_enabled {
                            truncate_to_sentences(full_reply, 2)
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
                _ => {
                    thinking.store(false, Ordering::Relaxed);
                    log(&format!("❌ OpenClaw error: {}", reply_text));
                    update_live(&transcript, &format!("❌ Error: {}", &reply_text[..reply_text.len().min(100)]));
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
