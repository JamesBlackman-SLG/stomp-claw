use reqwest::Client;
use hound::{SampleFormat as HoundFormat, WavSpec, WavWriter};
use sqlx::SqlitePool;
use tempfile::NamedTempFile;
use std::io::Read;

use crate::config;
use crate::db;
use crate::events::{Event, EventSender, EventReceiver};
use crate::commands;

async fn transcribe(samples: &[f32], client: &Client) -> Option<String> {
    let tmp = NamedTempFile::new().ok()?;
    {
        let spec = WavSpec {
            channels: 1,
            sample_rate: config::TARGET_SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: HoundFormat::Int,
        };
        let mut w = WavWriter::new(&tmp, spec).ok()?;
        for s in samples {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).ok()?;
        }
        w.finalize().ok()?;
    }

    let mut buf = Vec::new();
    std::fs::File::open(tmp.path()).ok()?.read_to_end(&mut buf).ok()?;

    let part = reqwest::multipart::Part::bytes(buf)
        .file_name("audio.wav")
        .mime_str("audio/wav").ok()?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = client.post(&format!("{}/transcribe/", config::NEMO_URL))
        .multipart(form)
        .send().await.ok()?;

    let text = resp.text().await.ok()?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

pub async fn run(tx: EventSender, mut rx: EventReceiver, pool: SqlitePool) {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    loop {
        match rx.recv().await {
            Ok(Event::RecordingComplete { session_id, samples }) => {
                tracing::info!("Transcribing {} samples for session {}", samples.len(), session_id);
                match transcribe(&samples, &client).await {
                    Some(text) => {
                        tracing::info!("Final transcript: {}", text);

                        // Get session names for fuzzy matching
                        let session_names: Vec<String> = db::get_sessions(&pool).await
                            .unwrap_or_default()
                            .iter()
                            .map(|s| s.name.clone())
                            .collect();

                        // Check for voice commands (including bare session name matching)
                        if let Some(cmd) = commands::parse_command_with_sessions(&text, &session_names) {
                            tracing::info!("Voice command detected: {:?}", cmd);
                            let _ = tx.send(Event::VoiceCommand { command: cmd });
                        } else {
                            tracing::debug!("No command match for '{}' (sessions: {:?})", text, session_names);
                            let _ = tx.send(Event::FinalTranscript {
                                session_id,
                                text,
                            });
                        }
                    }
                    None => {
                        tracing::warn!("Empty transcript");
                    }
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Transcription lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
