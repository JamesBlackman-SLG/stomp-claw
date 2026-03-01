use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};
use hound::{SampleFormat as HoundFormat, WavSpec, WavWriter};
use midir::{Ignore, MidiInput};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;

const LOG_FILE: &str = "/tmp/stomp-claw.log";
const CONVERSATION_LOG: &str = "/tmp/stomp-claw-conversation.md";
const LIVE_LOG: &str = "/tmp/stomp-claw-live.txt";
const PEDAL_CC: u8 = 85;
const NEMO_URL: &str = "http://localhost:5051";
const TARGET_SAMPLE_RATE: u32 = 16000;
const OPENCLAW_URL: &str = "http://127.0.0.1:18789/v1/chat/completions";
const OPENCLAW_TOKEN: &str = "06b21a7fafad855670f81018f3a455edccaf5dedc470fa0b";
const SESSION_FILE: &str = "/tmp/stomp-claw-session.txt";
const CONFIG_FILE: &str = "/home/jb/.config/stomp-claw/config.toml";

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

/// Check if the transcript is a voice toggle command
fn check_voice_command(transcript: &str) -> Option<bool> {
    let t = transcript.to_lowercase();
    let t = t.trim();
    
    if t.contains("voice") || t.contains("speech") || t.contains("talk") {
        if t.contains("on") && !t.contains("off") {
            return Some(true);
        }
        if t.contains("off") || t.contains("stop") || t.contains("disable") {
            return Some(false);
        }
    }
    None
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
    // Try binary's directory first
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let path = dir.join(format!("{}.wav", name));
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
    }
    // Fallback to /tmp
    format!("/tmp/{}.wav", name)
}

fn beep_down() {
    Command::new("paplay").arg(get_beep_path("beep-down")).spawn().ok();
}

fn beep_up() {
    Command::new("paplay").arg(get_beep_path("beep-up")).spawn().ok();
    std::thread::sleep(std::time::Duration::from_millis(150));
    Command::new("paplay").arg(get_beep_path("beep-up2")).spawn().ok();
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
    let audio_data: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    
    // Config shared between threads
    let config = Arc::new(Mutex::new(config));

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

    std::thread::spawn(move || {
        if let Err(e) = midi_listener(recording2, pedal_down2, audio2, config2) {
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
                    pedal_down.store(true, Ordering::Relaxed);
                    recording.store(true, Ordering::Relaxed);
                    audio_data.lock().unwrap().clear();
                } else if msg[2] == 0 && pedal_down.load(Ordering::Relaxed) {
                    log("👟 PEDAL UP");
                    beep_up();
                    recording.store(false, Ordering::Relaxed);
                    pedal_down.store(false, Ordering::Relaxed);
                    let samples = audio_data.lock().unwrap().clone();
                    let config = config.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = process(samples, config) {
                            log(&format!("Processing error: {}", e));
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

fn process(samples: Vec<f32>, config: Arc<Mutex<Config>>) -> Result<(), Box<dyn std::error::Error>> {
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
        .timeout(std::time::Duration::from_secs(30))
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
            update_live(&transcript, "...");

            // Check for voice toggle command
            let voice_was_enabled = {
                let cfg = config.lock().unwrap();
                cfg.voice_enabled
            };
            
            if let Some(new_voice_state) = check_voice_command(&transcript) {
                let mut cfg = config.lock().unwrap();
                let changed = cfg.voice_enabled != new_voice_state;
                cfg.voice_enabled = new_voice_state;
                save_config(&cfg);
                
                let msg = if new_voice_state {
                    "Voice enabled"
                } else {
                    "Voice disabled"
                };
                log(&format!("🔊 {}", msg));
                if voice_was_enabled != new_voice_state {
                    speak(msg);
                }
                return Ok::<_, Box<dyn std::error::Error>>(());
            }

            // Normal message - send to OpenClaw with Sonnet
            let session = get_or_create_session();
            log(&format!("📤 Sending to OpenClaw (session: {})...", session));

            let system_prompt = "You are talking to James via voice-only (foot pedal + TTS). Keep responses very short - 1-2 sentences max. Be direct and conversational. No long explanations.";

            let payload = serde_json::json!({
                "model": "openclaw:claude-sonnet-4-6",  // Use Sonnet for better understanding
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": &transcript}
                ],
                "stream": false,
                "max_tokens": 150,
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

            if let Ok(parsed) = serde_json::from_str::<OpenClawResponse>(&reply_text) {
                if let Some(choice) = parsed.choices.first() {
                    let full_reply = &choice.message.content;
                    let short_reply = truncate_to_sentences(full_reply, 2);
                    
                    log(&format!("💬 Alan: {}", short_reply));
                    update_live(&transcript, &short_reply);
                    log_conversation(&transcript, &short_reply);
                    
                    // Only speak if voice is enabled
                    let cfg = config.lock().unwrap();
                    if cfg.voice_enabled {
                        speak(&short_reply);
                    }
                }
            }
        } else {
            log("Empty transcript");
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;

    Ok(())
}
