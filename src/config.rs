use std::path::PathBuf;
use serde::{Deserialize, Serialize};

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

/// Read the current primary model from OpenClaw config and return its context window size.
pub fn openclaw_context_window() -> u32 {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".openclaw/openclaw.json");

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return 200_000, // safe default
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return 200_000,
    };

    let model_id = json
        .pointer("/agents/defaults/model/primary")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    model_context_window(model_id)
}

fn model_context_window(model_id: &str) -> u32 {
    match model_id {
        s if s.contains("MiniMax-M2.5") => 1_000_000,
        s if s.contains("claude-opus-4-6") => 1_000_000,
        s if s.contains("claude-sonnet-4-6") => 1_000_000,
        s if s.contains("claude-opus") => 200_000,
        s if s.contains("claude-sonnet") => 200_000,
        s if s.contains("claude-haiku") => 200_000,
        s if s.contains("gpt-4o") => 128_000,
        s if s.contains("gpt-4-turbo") => 128_000,
        _ => 200_000, // safe default
    }
}

pub fn base_dir() -> PathBuf {
    dirs::home_dir()
        .expect("No home directory found")
        .join(".stomp-claw")
}

pub fn db_path() -> PathBuf {
    base_dir().join("stomp-claw.db")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
}

/// Discover agents from OpenClaw config file.
pub fn discover_agents() -> Vec<Agent> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".openclaw/openclaw.json");

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Could not read OpenClaw config: {}", e);
            return vec![Agent { id: "main".into(), name: "main".into() }];
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Could not parse OpenClaw config: {}", e);
            return vec![Agent { id: "main".into(), name: "main".into() }];
        }
    };

    let default_workspace = json
        .pointer("/agents/defaults/workspace")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let agents_list = match json.pointer("/agents/list").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return vec![Agent { id: "main".into(), name: "main".into() }],
    };

    let mut agents = Vec::new();
    for entry in agents_list {
        let id = match entry.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        let workspace = entry.get("workspace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| default_workspace.clone());

        let name = workspace
            .as_ref()
            .and_then(|ws| read_identity_name(ws))
            .unwrap_or_else(|| id.clone());

        agents.push(Agent { id, name });
    }

    if agents.is_empty() {
        agents.push(Agent { id: "main".into(), name: "main".into() });
    }

    agents
}

fn read_identity_name(workspace_path: &str) -> Option<String> {
    let identity_path = std::path::Path::new(workspace_path).join("IDENTITY.md");
    let content = std::fs::read_to_string(&identity_path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("- **Name:**")
            .or_else(|| line.strip_prefix("**Name:**"))
        {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}
