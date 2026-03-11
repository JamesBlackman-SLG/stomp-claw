use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::config;
use crate::db;
use crate::events::{Event, EventSender, EventReceiver};

/// Responses API streaming event — we only care about text deltas and completion
#[derive(Deserialize)]
struct ResponsesEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<String>,
}

async fn send_to_llm(
    tx: &EventSender,
    pool: &SqlitePool,
    client: &Client,
    session_id: &str,
    user_message: &str,
    voice_enabled: bool,
    images: &[String],
    documents: &[(String, String)],
) {
    // Create user turn in DB
    let images_json = if images.is_empty() { None } else {
        Some(serde_json::to_string(images).unwrap_or_default())
    };
    let documents_json = if documents.is_empty() { None } else {
        let doc_objects: Vec<serde_json::Value> = documents.iter().map(|(path, filename)| {
            serde_json::json!({"path": path, "filename": filename})
        }).collect();
        Some(serde_json::to_string(&doc_objects).unwrap_or_default())
    };
    let _user_turn_id = match db::create_turn_with_attachments(pool, session_id, "user", user_message, "complete", images_json.as_deref(), documents_json.as_deref()).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Failed to create user turn: {}", e);
            return;
        }
    };

    // Create assistant turn with streaming status
    let assistant_turn_id = match db::create_turn(pool, session_id, "assistant", "", "streaming").await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Failed to create assistant turn: {}", e);
            return;
        }
    };

    let _ = tx.send(Event::LlmThinking {
        session_id: session_id.to_string(),
        turn_id: assistant_turn_id,
    });

    let (system_prompt, max_tokens) = if voice_enabled {
        (config::VOICE_SYSTEM_PROMPT, config::VOICE_MAX_TOKENS)
    } else {
        (config::TEXT_SYSTEM_PROMPT, config::TEXT_MAX_TOKENS)
    };

    // Build user content parts for Responses API
    // API requires an input_text part — use placeholder for attachment-only messages
    let text = if user_message.is_empty() && !images.is_empty() {
        "Describe this image."
    } else if user_message.is_empty() && !documents.is_empty() {
        "Analyze this document."
    } else {
        user_message
    };
    let mut user_parts = vec![serde_json::json!({
        "type": "input_text",
        "text": text
    })];

    if !images.is_empty() {
        use base64::Engine;
        for img_path in images {
            if let Ok(bytes) = tokio::fs::read(img_path).await {
                let ext = std::path::Path::new(img_path)
                    .extension().and_then(|e| e.to_str()).unwrap_or("png");
                let media_type = match ext {
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "webp" => "image/webp",
                    _ => "image/png",
                };
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                user_parts.push(serde_json::json!({
                    "type": "input_image",
                    "source": {
                        "type": "base64",
                        "media_type": media_type,
                        "data": b64
                    }
                }));
            }
        }
    }

    // Add document parts inside user message content
    if !documents.is_empty() {
        use base64::Engine;
        for (doc_path, filename) in documents {
            if let Ok(bytes) = tokio::fs::read(doc_path).await {
                let ext = std::path::Path::new(doc_path)
                    .extension().and_then(|e| e.to_str()).unwrap_or("txt");
                let media_type = match ext {
                    "pdf" => "application/pdf",
                    "csv" => "text/csv",
                    "json" => "application/json",
                    "html" => "text/html",
                    "md" => "text/markdown",
                    _ => "text/plain",
                };
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                user_parts.push(serde_json::json!({
                    "type": "input_file",
                    "source": { "type": "base64", "media_type": media_type, "data": b64, "filename": filename }
                }));
            }
        }
    }

    // Include documents from prior turns in this session
    if let Ok(prior_doc_jsons) = db::get_session_documents(pool, session_id).await {
        use base64::Engine;
        let mut total_size: usize = 0;
        for doc_json in &prior_doc_jsons {
            if let Ok(docs) = serde_json::from_str::<Vec<serde_json::Value>>(doc_json) {
                for doc in &docs {
                    let path = doc["path"].as_str().unwrap_or("");
                    let filename = doc["filename"].as_str().unwrap_or("document");
                    if documents.iter().any(|(p, _)| p == path) { continue; }
                    if let Ok(bytes) = tokio::fs::read(path).await {
                        total_size += bytes.len();
                        if total_size > 20 * 1024 * 1024 {
                            tracing::warn!("Session document context exceeds 20MB, skipping remaining");
                            break;
                        }
                        let ext = std::path::Path::new(path)
                            .extension().and_then(|e| e.to_str()).unwrap_or("txt");
                        let media_type = match ext {
                            "pdf" => "application/pdf",
                            "csv" => "text/csv",
                            "json" => "application/json",
                            "html" => "text/html",
                            "md" => "text/markdown",
                            _ => "text/plain",
                        };
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        user_parts.push(serde_json::json!({
                            "type": "input_file",
                            "source": { "type": "base64", "media_type": media_type, "data": b64, "filename": filename }
                        }));
                    }
                }
            }
            if total_size > 20 * 1024 * 1024 { break; }
        }
    }

    // Build Responses API payload
    let payload = serde_json::json!({
        "model": "openclaw",
        "instructions": system_prompt,
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": user_parts
            }
        ],
        "stream": true,
        "max_output_tokens": max_tokens,
        "user": "stomp-claw"
    });

    let resp = match client.post(config::OPENCLAW_URL)
        .header("Authorization", format!("Bearer {}", config::openclaw_token()))
        .header("Content-Type", "application/json")
        .header("x-openclaw-session-key", session_id)
        .json(&payload)
        .send().await
    {
        Ok(r) => r,
        Err(e) => {
            let error = format!("OpenClaw request failed: {:?}", e);
            tracing::error!("{}", error);
            let _ = db::error_turn(pool, assistant_turn_id, &error).await;
            let _ = tx.send(Event::LlmError {
                session_id: session_id.to_string(),
                turn_id: assistant_turn_id,
                error,
            });
            return;
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let error = format!("HTTP {}: {}", status, body);
        tracing::error!("OpenClaw error: {}", error);
        let _ = db::error_turn(pool, assistant_turn_id, &error).await;
        let _ = tx.send(Event::LlmError {
            session_id: session_id.to_string(),
            turn_id: assistant_turn_id,
            error,
        });
        return;
    }

    // Stream the response (Responses API SSE format)
    let mut full_reply = String::new();
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut stream_done = false;
    let mut first_token = true;
    let mut token_count = 0u32;
    let mut last_db_update = std::time::Instant::now();

    loop {
        if stream_done { break; }

        // Generous timeout: OpenClaw may pause between text tokens while
        // executing tool calls. Stream ends via response.completed / [DONE].
        let timeout_secs = 120;
        let chunk = match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            stream.next()
        ).await {
            Ok(Some(Ok(chunk))) => chunk,
            Ok(Some(Err(e))) => {
                tracing::error!("Stream error: {}", e);
                break;
            }
            Ok(None) => {
                tracing::info!("Stream ended naturally");
                break;
            }
            Err(_) => {
                tracing::info!("Stream timed out ({}s), treating as done", timeout_secs);
                break;
            }
        };

        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if line.is_empty() { continue; }

            // Responses API uses "event: <type>" + "data: <json>" format
            // Skip event: lines, process data: lines
            if line.starts_with("event:") || line.starts_with(':') { continue; }

            if let Some(data) = line.strip_prefix("data: ") {
                if data.trim() == "[DONE]" {
                    tracing::info!("Received [DONE]");
                    stream_done = true;
                    break;
                }

                if let Ok(evt) = serde_json::from_str::<ResponsesEvent>(data) {
                    if evt.event_type == "response.output_text.delta" {
                        if let Some(delta) = &evt.delta {
                            full_reply.push_str(delta);
                            first_token = false;
                            token_count += 1;

                            let _ = tx.send(Event::LlmToken {
                                session_id: session_id.to_string(),
                                turn_id: assistant_turn_id,
                                token: delta.clone(),
                                accumulated: full_reply.clone(),
                            });

                            // Debounced DB update: every 10 tokens or 500ms
                            if token_count % 10 == 0 || last_db_update.elapsed().as_millis() > 500 {
                                let _ = db::update_turn_content(pool, assistant_turn_id, &full_reply).await;
                                last_db_update = std::time::Instant::now();
                            }
                        }
                    } else if evt.event_type == "response.completed" {
                        tracing::info!("Received response.completed");
                        stream_done = true;
                        break;
                    }
                }
            }
        }
    }

    if full_reply.is_empty() {
        let error = "Empty response from OpenClaw".to_string();
        let _ = db::error_turn(pool, assistant_turn_id, &error).await;
        let _ = tx.send(Event::LlmError {
            session_id: session_id.to_string(),
            turn_id: assistant_turn_id,
            error,
        });
    } else {
        let _ = db::complete_turn(pool, assistant_turn_id, &full_reply).await;
        let _ = tx.send(Event::LlmDone {
            session_id: session_id.to_string(),
            turn_id: assistant_turn_id,
            full_response: full_reply,
        });
    }
}

pub async fn run(tx: EventSender, mut rx: EventReceiver, pool: SqlitePool) {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .expect("Failed to create HTTP client");

    // Track voice state
    let mut voice_enabled = db::get_config(&pool, "voice_enabled").await
        .ok().flatten()
        .map(|v| v == "true")
        .unwrap_or(true);

    // Track busy sessions
    let mut busy_sessions = std::collections::HashSet::new();

    loop {
        match rx.recv().await {
            Ok(Event::FinalTranscript { session_id, text }) => {
                if busy_sessions.contains(&session_id) {
                    tracing::warn!("Session {} is busy, rejecting message", session_id);
                    let _ = tx.send(Event::LlmError {
                        session_id: session_id.clone(),
                        turn_id: 0,
                        error: "Session is busy".to_string(),
                    });
                    continue;
                }

                busy_sessions.insert(session_id.clone());
                let _ = db::touch_session(&pool, &session_id).await;

                send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &[], &[]).await;

                busy_sessions.remove(&session_id);
            }
            Ok(Event::UserTextMessage { session_id, text, images, documents }) => {
                if busy_sessions.contains(&session_id) {
                    let _ = tx.send(Event::LlmError {
                        session_id: session_id.clone(),
                        turn_id: 0,
                        error: "Session is busy".to_string(),
                    });
                    continue;
                }

                busy_sessions.insert(session_id.clone());
                let _ = db::touch_session(&pool, &session_id).await;

                send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &images, &documents).await;

                busy_sessions.remove(&session_id);
            }
            Ok(Event::VoiceToggled { enabled }) => {
                voice_enabled = enabled;
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("LLM lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
