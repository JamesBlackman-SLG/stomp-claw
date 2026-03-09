use std::path::PathBuf;

// External service endpoints
pub const NEMO_URL: &str = "http://localhost:5051";
pub const OPENCLAW_URL: &str = "http://127.0.0.1:18789/v1/responses";

pub fn openclaw_token() -> String {
    std::env::var("OPENCLAW_TOKEN").expect("OPENCLAW_TOKEN env var must be set")
}

// Audio
pub const TARGET_SAMPLE_RATE: u32 = 16000;
pub const AUDIO_SINK: &str = "alsa_output.pci-0000_0d_00.4.analog-stereo";

// MIDI
pub const PEDAL_CC: u8 = 85;

// Server
pub const SERVER_ADDR: &str = "0.0.0.0:8765";
pub const TLS_ADDR: &str = "0.0.0.0:8766";

// System prompts
pub const VOICE_SYSTEM_PROMPT: &str = "You are talking to James via voice-only (foot pedal + TTS). Keep responses very short - 1-2 sentences max. Be direct and conversational. No long explanations.";
pub const TEXT_SYSTEM_PROMPT: &str = "You are Alan, James's AI assistant. You are chatting via a web UI that renders full markdown. IMPORTANT: When discussing or showing an image file, you MUST always include its full absolute file path in your response (e.g. /home/jb/Pictures/photo.png). The UI will automatically render any image path as an inline image. Never say 'here it is' without including the actual path.";
pub const VOICE_MAX_TOKENS: u32 = 150;
pub const TEXT_MAX_TOKENS: u32 = 2000;

pub fn base_dir() -> PathBuf {
    dirs::home_dir()
        .expect("No home directory found")
        .join(".stomp-claw")
}

pub fn db_path() -> PathBuf {
    base_dir().join("stomp-claw.db")
}

pub fn log_path() -> PathBuf {
    base_dir().join("stomp-claw.log")
}
