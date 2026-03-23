use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::config;
use crate::db;
use crate::events::{Event, EventSender, EventReceiver};

/// Extract text content from a .pptx file (which is a ZIP of XML slides).
fn extract_pptx_text(bytes: &[u8]) -> Result<String, String> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Not a valid zip/pptx: {}", e))?;

    // Collect slide filenames and sort by slide number
    let mut slide_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .collect();
    slide_names.sort();

    let mut output = String::new();
    for (i, name) in slide_names.iter().enumerate() {
        let mut file = archive.by_name(name).map_err(|e| format!("Can't read {}: {}", name, e))?;
        let mut xml = String::new();
        file.read_to_string(&mut xml).map_err(|e| format!("Read error {}: {}", name, e))?;

        // Extract text from <a:t>...</a:t> tags (DrawingML text runs)
        let mut slide_text = String::new();
        for chunk in xml.split("<a:t") {
            if let Some(rest) = chunk.strip_prefix(">").or_else(|| {
                // Handle <a:t xml:space="preserve"> etc
                chunk.find('>').map(|pos| &chunk[pos + 1..])
            })
                && let Some(text) = rest.split("</a:t>").next()
                && !text.is_empty()
            {
                if !slide_text.is_empty() { slide_text.push(' '); }
                slide_text.push_str(text);
            }
        }
        if !slide_text.is_empty() {
            if !output.is_empty() { output.push_str("\n\n"); }
            output.push_str(&format!("--- Slide {} ---\n{}", i + 1, slide_text));
        }
    }
    if output.is_empty() {
        return Err("No text content found in pptx".to_string());
    }
    Ok(output)
}

/// Usage data from the Responses API response.completed event
#[derive(Deserialize, Clone, Debug)]
struct UsageData {
    input_tokens: u32,
    output_tokens: u32,
    total_tokens: u32,
}

/// Inner response object (from response.completed)
#[derive(Deserialize)]
struct ResponseObject {
    id: Option<String>,
    usage: Option<UsageData>,
}

/// Responses API streaming event — we care about text deltas, completion, and usage
#[derive(Deserialize)]
struct ResponsesEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<String>,
    response: Option<ResponseObject>,
}

#[allow(clippy::too_many_arguments)]
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

                // Convert pptx to extracted text since OpenClaw can't process raw pptx
                let (send_bytes, media_type) = if ext == "pptx" {
                    match extract_pptx_text(&bytes) {
                        Ok(text) => {
                            tracing::info!("Extracted {} chars from pptx: {}", text.len(), filename);
                            (text.into_bytes(), "text/plain")
                        }
                        Err(e) => {
                            tracing::warn!("Failed to extract pptx text: {}", e);
                            (bytes, "text/plain")
                        }
                    }
                } else {
                    let mt = match ext {
                        "pdf" => "application/pdf",
                        "csv" => "text/csv",
                        "json" => "application/json",
                        "html" => "text/html",
                        "md" => "text/markdown",
                        _ => "text/plain",
                    };
                    (bytes, mt)
                };

                let b64 = base64::engine::general_purpose::STANDARD.encode(&send_bytes);
                user_parts.push(serde_json::json!({
                    "type": "input_file",
                    "source": { "type": "base64", "media_type": media_type, "data": b64, "filename": filename }
                }));
            }
        }
    }



    // Load previous response ID for conversation chaining
    let prev_response_id = db::get_config(pool, &format!("prev_response_id:{}", session_id))
        .await.ok().flatten();

    // Build Responses API payload
    let mut payload = serde_json::json!({
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
        "store": true,
        "max_output_tokens": max_tokens,
        "user": "stomp-claw"
    });
    if let Some(ref prev_id) = prev_response_id {
        payload.as_object_mut().unwrap().insert(
            "previous_response_id".to_string(),
            serde_json::Value::String(prev_id.clone()),
        );
    }

    let resp = match client.post(config::OPENCLAW_URL)
        .header("Authorization", format!("Bearer {}", config::openclaw_token()))
        .header("Content-Type", "application/json")
        .header("x-openclaw-session-key", format!("agent:main:{}", session_id))
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

    tracing::info!("OpenClaw HTTP {}", resp.status());

    // Stream the response (Responses API SSE format)
    let mut full_reply = String::new();
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut stream_done = false;
    let mut token_count = 0u32;
    let mut last_db_update = std::time::Instant::now();
    let mut usage: Option<UsageData> = None;

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

                match serde_json::from_str::<ResponsesEvent>(data) {
                    Ok(evt) => {
                    if evt.event_type == "response.output_text.delta" {
                        if let Some(delta) = &evt.delta {
                            full_reply.push_str(delta);
                            token_count += 1;

                            let _ = tx.send(Event::LlmToken {
                                session_id: session_id.to_string(),
                                turn_id: assistant_turn_id,
                                token: delta.clone(),
                                accumulated: full_reply.clone(),
                            });

                            // Debounced DB update: every 10 tokens or 500ms
                            if token_count.is_multiple_of(10) || last_db_update.elapsed().as_millis() > 500 {
                                let _ = db::update_turn_content(pool, assistant_turn_id, &full_reply).await;
                                last_db_update = std::time::Instant::now();
                            }
                        }
                    } else if evt.event_type == "response.completed" {
                        tracing::info!("response.completed raw: {}", &data[..data.len().min(500)]);
                        if let Some(ref resp) = evt.response {
                            // Save response ID for conversation chaining
                            if let Some(ref id) = resp.id {
                                tracing::info!("Saving response ID for session {}: {}", session_id, id);
                                let _ = db::set_config(pool, &format!("prev_response_id:{}", session_id), id).await;
                            }
                            if let Some(ref u) = resp.usage {
                                tracing::info!("Usage: input={}, output={}, total={}", u.input_tokens, u.output_tokens, u.total_tokens);
                                usage = Some(u.clone());
                            }
                        }
                        tracing::info!("Received response.completed");
                        stream_done = true;
                        break;
                    }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse SSE data: {} — raw: {}", e, &data[..data.len().min(200)]);
                    }
                }
            }
        }
    }

    if full_reply.is_empty() {
        let error = "Empty response from OpenClaw".to_string();
        tracing::warn!("{}", error);
        let _ = db::error_turn(pool, assistant_turn_id, &error).await;
        let _ = tx.send(Event::LlmError {
            session_id: session_id.to_string(),
            turn_id: assistant_turn_id,
            error,
        });
    } else {
        let _ = db::complete_turn(pool, assistant_turn_id, &full_reply).await;
        if let Some(ref u) = usage {
            let _ = db::set_session_tokens(pool, session_id, u.total_tokens).await;
        }
        let _ = tx.send(Event::LlmDone {
            session_id: session_id.to_string(),
            turn_id: assistant_turn_id,
            full_response: full_reply,
            input_tokens: usage.as_ref().map(|u| u.input_tokens),
            output_tokens: usage.as_ref().map(|u| u.output_tokens),
            total_tokens: usage.as_ref().map(|u| u.total_tokens),
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

    loop {
        match rx.recv().await {
            Ok(Event::FinalTranscript { session_id, text }) => {
                tracing::info!("LLM: Received FinalTranscript: '{}'", text);
                let tx = tx.clone();
                let pool = pool.clone();
                let client = client.clone();
                tokio::spawn(async move {
                    let _ = db::touch_session(&pool, &session_id).await;
                    send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &[], &[]).await;
                });
            }
            Ok(Event::UserTextMessage { session_id, text, images, documents }) => {
                let tx = tx.clone();
                let pool = pool.clone();
                let client = client.clone();
                tokio::spawn(async move {
                    let _ = db::touch_session(&pool, &session_id).await;
                    send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &images, &documents).await;
                });
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
