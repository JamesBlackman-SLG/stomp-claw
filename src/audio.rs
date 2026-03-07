use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};
use hound::{SampleFormat as HoundFormat, WavSpec, WavWriter};
use reqwest::Client;
use std::io::Read;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use tempfile::NamedTempFile;

use crate::config;
use crate::events::{Event, EventSender, EventReceiver};
use crate::commands;

async fn partial_transcribe(samples: &[f32], client: &Client) -> Option<String> {
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

pub async fn run(tx: EventSender, mut rx: EventReceiver) {
    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device");
    tracing::info!("Audio device: {:?}", device.name());

    let _supported = device.supported_input_configs()
        .expect("No supported configs")
        .find(|c| c.sample_format() == SampleFormat::F32 && c.channels() == 1)
        .expect("No mono f32 config");

    let stream_config: StreamConfig = StreamConfig {
        channels: 1,
        sample_rate: SampleRate(config::TARGET_SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let recording = Arc::new(AtomicBool::new(false));

    let samples_writer = samples.clone();
    let recording_flag = recording.clone();

    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[f32], _| {
            if recording_flag.load(Ordering::Relaxed) {
                samples_writer.lock().unwrap().extend_from_slice(data);
            }
        },
        |err| tracing::error!("Audio stream error: {}", err),
        None,
    ).expect("Failed to build audio stream");

    stream.play().expect("Failed to start audio stream");
    tracing::info!("Audio stream ready");

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to create HTTP client");

    // Track current session for partial transcription
    let mut current_session_id = String::new();

    loop {
        match rx.recv().await {
            Ok(Event::PedalDown) => {
                tracing::info!("Recording started");
                samples.lock().unwrap().clear();
                recording.store(true, Ordering::Relaxed);

                let session_id = current_session_id.clone();
                let _ = tx.send(Event::RecordingStarted { session_id: session_id.clone() });

                // Spawn partial transcription loop
                let tx2 = tx.clone();
                let samples2 = samples.clone();
                let recording2 = recording.clone();
                let client2 = client.clone();
                let sid = session_id.clone();

                tokio::spawn(async move {
                    let start = std::time::Instant::now();
                    while recording2.load(Ordering::Relaxed) {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        if start.elapsed().as_secs_f64() > 0.5 {
                            let snapshot = samples2.lock().unwrap().clone();
                            if let Some(text) = partial_transcribe(&snapshot, &client2).await {
                                // Check for cancel keywords
                                if commands::is_cancel_keyword(&text) {
                                    tracing::info!("Cancel keyword detected: {}", text);
                                    recording2.store(false, Ordering::Relaxed);
                                    let _ = tx2.send(Event::RecordingCancelled { session_id: sid.clone() });
                                    break;
                                }
                                let _ = tx2.send(Event::PartialTranscript {
                                    session_id: sid.clone(),
                                    text,
                                });
                            }
                        }
                    }
                });
            }
            Ok(Event::PedalUp) => {
                recording.store(false, Ordering::Relaxed);
                let captured = samples.lock().unwrap().clone();
                if !captured.is_empty() {
                    let _ = tx.send(Event::RecordingComplete {
                        session_id: current_session_id.clone(),
                        samples: captured,
                    });
                }
            }
            Ok(Event::SessionSwitched { session_id }) => {
                current_session_id = session_id;
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Audio lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }

    // Keep stream alive (it's dropped when this function returns)
    drop(stream);
}
