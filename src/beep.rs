use std::process::Command as ProcessCommand;
use crate::commands;
use crate::config;
use crate::events::{Event, EventReceiver};

fn play_sound(filename: &str) {
    let path = std::env::current_dir()
        .unwrap_or_default()
        .join(filename);

    if path.exists() {
        let sink = config::AUDIO_SINK;
        std::thread::spawn(move || {
            let _ = ProcessCommand::new("paplay")
                .arg("--device")
                .arg(sink)
                .arg(path)
                .output();
        });
    }
}

pub fn beep_down() { play_sound("beep-down.wav"); }
pub fn beep_up() { play_sound("beep-up.wav"); }
pub fn beep_abort() { play_sound("beep-abort.wav"); }
pub fn notify() { play_sound("notify.wav"); }

pub fn speak(text: &str) {
    let text = text.to_string();
    std::thread::spawn(move || {
        let _ = ProcessCommand::new(
            dirs::home_dir().unwrap().join("bin/speak")
        )
        .arg(&text)
        .output();
    });
}

pub fn play_session_tone(index: usize) {
    let file = if index.is_multiple_of(2) { "beep-up.wav" } else { "beep-up2.wav" };
    play_sound(file);
}

pub async fn run(mut rx: EventReceiver, initial_voice_enabled: bool) {
    let mut voice_enabled = initial_voice_enabled;

    loop {
        match rx.recv().await {
            Ok(Event::PedalDown) => beep_down(),
            Ok(Event::PedalUp) => beep_up(),
            Ok(Event::RecordingCancelled { .. }) => beep_abort(),
            Ok(Event::VoiceCommand { .. }) => play_sound("command-ack.wav"),
            Ok(Event::LlmDone { full_response, .. }) => {
                if voice_enabled {
                    let truncated = commands::truncate_to_sentences(&full_response, 2);
                    speak(&truncated);
                } else {
                    notify();
                }
            }
            Ok(Event::VoiceToggled { enabled }) => {
                voice_enabled = enabled;
                speak(if enabled { "Voice enabled" } else { "Voice disabled" });
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Beep module lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
