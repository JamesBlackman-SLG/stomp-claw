use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use rust_embed::Embed;

use crate::db;
use crate::events::{Event, EventSender, EventReceiver};
use crate::config as app_config;

#[derive(Clone)]
pub struct AppState {
    pub tx: EventSender,
    pub pool: SqlitePool,
}

// --- WebSocket message types ---

#[derive(Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum WsOutgoing {
    SessionList { sessions: Vec<db::Session> },
    SessionSwitched { session_id: String },
    SessionCreated { session: db::Session },
    SessionRenamed { session_id: String, name: String },
    SessionDeleted { session_id: String },
    TurnList { session_id: String, turns: Vec<db::Turn> },
    TurnCreated { turn: db::Turn },
    RecordingStarted { session_id: String },
    RecordingCancelled { session_id: String },
    PartialTranscript { session_id: String, text: String },
    LlmThinking { session_id: String, turn_id: i64 },
    LlmToken { session_id: String, turn_id: i64, token: String, accumulated: String },
    LlmDone { session_id: String, turn_id: i64, content: String, input_tokens: Option<u32>, output_tokens: Option<u32>, total_tokens: Option<u32> },
    LlmError { session_id: String, turn_id: i64, error: String },
    VoiceToggled { enabled: bool },
    ShowHelp,
    Config { voice_enabled: bool, active_session_id: String },
    ContextUsage { total_tokens: u32, context_window: u32 },
}

#[derive(Deserialize)]
struct WsDocument {
    data: String,
    filename: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum WsIncoming {
    SendMessage { session_id: String, text: String, #[serde(default)] images: Vec<String>, #[serde(default)] documents: Vec<WsDocument> },
    SwitchSession { session_id: String },
    CreateSession,
    RenameSession { session_id: String, name: String },
    DeleteSession { session_id: String },
    CancelRecording,
    ToggleVoice,
}

// --- Embedded frontend assets ---

#[derive(Embed)]
#[folder = "ui/dist/client"]
struct FrontendAssets;

async fn local_file_handler(
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let path = match query.get("path") {
        Some(p) => std::path::PathBuf::from(p),
        None => return axum::http::StatusCode::BAD_REQUEST.into_response(),
    };
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let base_dir = app_config::base_dir().canonicalize().unwrap_or_default();
    let openclaw_dir = dirs::home_dir().unwrap_or_default().join(".openclaw");
    let openclaw_dir = openclaw_dir.canonicalize().unwrap_or(openclaw_dir);
    if !canonical.starts_with(&base_dir) && !canonical.starts_with(&openclaw_dir) {
        return (axum::http::StatusCode::FORBIDDEN, "Access denied").into_response();
    }
    let ext = canonical.extension()
        .and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let allowed = matches!(ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" |
        "pdf" | "csv" | "txt" | "json" | "html" | "md" | "pptx"
    );
    if !allowed {
        return (axum::http::StatusCode::FORBIDDEN, "File type not allowed").into_response();
    }
    match tokio::fs::read(&canonical).await {
        Ok(data) => {
            let mime = mime_guess::from_path(&canonical).first_or_octet_stream();
            if matches!(ext.as_str(), "pdf" | "csv" | "txt" | "json" | "html" | "md" | "pptx") {
                let filename = canonical.file_name()
                    .and_then(|n| n.to_str()).unwrap_or("download");
                (
                    [
                        (axum::http::header::CONTENT_TYPE, mime.as_ref().to_string()),
                        (axum::http::header::CONTENT_DISPOSITION, format!("inline; filename=\"{}\"", filename)),
                    ],
                    data,
                ).into_response()
            } else {
                (
                    [(axum::http::header::CONTENT_TYPE, mime.as_ref().to_string())],
                    data,
                ).into_response()
            }
        }
        Err(_) => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

async fn static_handler(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match FrontendAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(axum::http::header::CONTENT_TYPE, mime.as_ref().to_string())],
                content.data.into_owned(),
            ).into_response()
        }
        None => {
            // SPA fallback — serve index.html for client-side routing
            match FrontendAssets::get("index.html") {
                Some(content) => {
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/html".to_string())],
                        content.data.into_owned(),
                    ).into_response()
                }
                None => axum::http::StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}

// --- Routes ---

pub async fn run(tx: EventSender, _rx: EventReceiver, pool: SqlitePool) {
    let state = AppState {
        tx: tx.clone(),
        pool: pool.clone(),
    };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/sessions", get(get_sessions))
        .route("/api/sessions/{id}/turns", get(get_turns))
        .route("/api/config", get(get_config))
        .route("/local-file", get(local_file_handler))
        .fallback(static_handler)
        .with_state(state);

    // Check for TLS cert/key files (e.g. Tailscale HTTPS)
    let (cert_path, key_path) = find_tls_files();

    if let (Some(cert_path), Some(key_path)) = (cert_path, key_path) {
        // Run both HTTP and HTTPS
        let http_app = app.clone();
        let tls_app = app;

        let http_handle = tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(app_config::SERVER_ADDR)
                .await
                .expect("Failed to bind HTTP server");
            tracing::info!("HTTP server listening on {}", app_config::SERVER_ADDR);
            axum::serve(listener, http_app).await.expect("HTTP server failed");
        });

        let tls_handle = tokio::spawn(async move {
            use std::io::BufReader;
            use tokio_rustls::TlsAcceptor;
            use rustls_pemfile::{certs, pkcs8_private_keys};

            let cert_file = std::fs::File::open(&cert_path).expect("Failed to open cert");
            let key_file = std::fs::File::open(&key_path).expect("Failed to open key");

            let certs: Vec<_> = certs(&mut BufReader::new(cert_file))
                .collect::<Result<Vec<_>, _>>()
                .expect("Failed to parse certs");

            let key = {
                // Try EC keys first, then PKCS8, then RSA
                use rustls_pemfile::ec_private_keys;
                let key_data = std::fs::read(&key_path).expect("Failed to read key file");

                let mut reader = BufReader::new(&key_data[..]);
                if let Some(Ok(k)) = ec_private_keys(&mut reader).next() {
                    rustls::pki_types::PrivateKeyDer::Sec1(k)
                } else {
                    let mut reader = BufReader::new(&key_data[..]);
                    if let Some(Ok(k)) = pkcs8_private_keys(&mut reader).next() {
                        rustls::pki_types::PrivateKeyDer::Pkcs8(k)
                    } else {
                        panic!("No supported private key found in key file");
                    }
                }
            };

            let tls_config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .expect("Failed to build TLS config");

            let tls_acceptor = TlsAcceptor::from(std::sync::Arc::new(tls_config));
            let tcp_listener = tokio::net::TcpListener::bind(app_config::TLS_ADDR)
                .await
                .expect("Failed to bind TLS server");

            tracing::info!("HTTPS server listening on {}", app_config::TLS_ADDR);

            loop {
                let (tcp_stream, _addr) = match tcp_listener.accept().await {
                    Ok(conn) => conn,
                    Err(e) => { tracing::warn!("TCP accept error: {e}"); continue; }
                };

                let acceptor = tls_acceptor.clone();
                let app = tls_app.clone();

                tokio::spawn(async move {
                    let tls_stream = match acceptor.accept(tcp_stream).await {
                        Ok(s) => s,
                        Err(e) => { tracing::debug!("TLS handshake failed: {e}"); return; }
                    };

                    let io = hyper_util::rt::TokioIo::new(tls_stream);
                    let service = hyper_util::service::TowerToHyperService::new(app.into_service());

                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                        .serve_connection_with_upgrades(io, service)
                        .await
                    {
                        tracing::debug!("Connection error: {e}");
                    }
                });
            }
        });

        tokio::select! {
            r = http_handle => r.expect("HTTP task panicked"),
            r = tls_handle => r.expect("HTTPS task panicked"),
        }
    } else {
        // HTTP only
        let listener = tokio::net::TcpListener::bind(app_config::SERVER_ADDR)
            .await
            .expect("Failed to bind server");
        tracing::info!("Server listening on {}", app_config::SERVER_ADDR);
        axum::serve(listener, app).await.expect("Server failed");
    }
}

async fn get_sessions(State(state): State<AppState>) -> Json<Vec<db::Session>> {
    let sessions = db::get_sessions(&state.pool).await.unwrap_or_default();
    Json(sessions)
}

async fn get_turns(
    State(state): State<AppState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Json<Vec<db::Turn>> {
    let turns = db::get_turns(&state.pool, &session_id).await.unwrap_or_default();
    Json(turns)
}

async fn get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let voice = db::get_config(&state.pool, "voice_enabled").await
        .ok().flatten()
        .map(|v| v == "true")
        .unwrap_or(true);
    let session_id = db::get_active_session_id(&state.pool).await
        .ok().flatten()
        .unwrap_or_default();
    Json(serde_json::json!({
        "voice_enabled": voice,
        "active_session_id": session_id,
    }))
}

// --- WebSocket handler ---

async fn ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    // Validate Origin to prevent cross-site WebSocket hijacking
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        let allowed = origin.starts_with("http://127.0.0.1:")
            || origin.starts_with("http://localhost:")
            || origin.starts_with("http://192.168.")
            || origin.starts_with("http://10.")
            || origin.starts_with("http://100.")
            || origin.starts_with("http://172.")
            || origin.starts_with("https://127.0.0.1:")
            || origin.starts_with("https://localhost:")
            || origin.starts_with("https://192.168.")
            || origin.starts_with("https://10.")
            || origin.starts_with("https://100.")
            || origin.starts_with("https://172.")
            || origin.contains(".ts.net");
        if !allowed {
            tracing::warn!("Rejected WebSocket from origin: {}", origin);
            return (axum::http::StatusCode::FORBIDDEN, "Forbidden").into_response();
        }
    }
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: AppState) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut event_rx = state.tx.subscribe();

    // Send initial state
    let voice_enabled = db::get_config(&state.pool, "voice_enabled").await
        .ok().flatten()
        .map(|v| v == "true")
        .unwrap_or(true);
    let active_session_id = db::get_active_session_id(&state.pool).await
        .ok().flatten()
        .unwrap_or_default();

    let _ = send_ws(&mut ws_tx, &WsOutgoing::Config {
        voice_enabled,
        active_session_id: active_session_id.clone(),
    }).await;

    // Send session list
    if let Ok(sessions) = db::get_sessions(&state.pool).await {
        let _ = send_ws(&mut ws_tx, &WsOutgoing::SessionList { sessions }).await;
    }

    // Send turns for active session
    if !active_session_id.is_empty() {
        if let Ok(turns) = db::get_turns(&state.pool, &active_session_id).await {
            let _ = send_ws(&mut ws_tx, &WsOutgoing::TurnList {
                session_id: active_session_id.clone(),
                turns,
            }).await;
        }
        if let Ok(Some(tokens)) = db::get_session_tokens(&state.pool, &active_session_id).await {
            let _ = send_ws(&mut ws_tx, &WsOutgoing::ContextUsage { total_tokens: tokens, context_window: app_config::openclaw_context_window() }).await;
        }
    }

    // Spawn task to forward events to WebSocket
    let pool = state.pool.clone();
    let mut forward_tx = ws_tx;
    let forward_handle = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    let msg = match event {
                        Event::RecordingStarted { session_id } =>
                            Some(WsOutgoing::RecordingStarted { session_id }),
                        Event::RecordingCancelled { session_id } =>
                            Some(WsOutgoing::RecordingCancelled { session_id }),
                        Event::PartialTranscript { session_id, text } =>
                            Some(WsOutgoing::PartialTranscript { session_id, text }),
                        Event::LlmThinking { session_id, turn_id } => {
                            // LLM just created user + assistant turns — send full turn list so UI has the user message
                            if let Ok(turns) = db::get_turns(&pool, &session_id).await {
                                let _ = send_ws(&mut forward_tx, &WsOutgoing::TurnList {
                                    session_id: session_id.clone(),
                                    turns,
                                }).await;
                            }
                            Some(WsOutgoing::LlmThinking { session_id, turn_id })
                        }
                        Event::LlmToken { session_id, turn_id, token, accumulated } =>
                            Some(WsOutgoing::LlmToken { session_id, turn_id, token, accumulated }),
                        Event::LlmDone { session_id, turn_id, full_response, input_tokens, output_tokens, total_tokens } => {
                            // Send final turn list so UI has the completed assistant turn from DB
                            if let Ok(turns) = db::get_turns(&pool, &session_id).await {
                                let _ = send_ws(&mut forward_tx, &WsOutgoing::TurnList {
                                    session_id: session_id.clone(),
                                    turns,
                                }).await;
                            }
                            if let Some(tokens) = total_tokens {
                                let _ = send_ws(&mut forward_tx, &WsOutgoing::ContextUsage {
                                    total_tokens: tokens,
                                    context_window: app_config::openclaw_context_window(),
                                }).await;
                            }
                            Some(WsOutgoing::LlmDone { session_id, turn_id, content: full_response, input_tokens, output_tokens, total_tokens })
                        }
                        Event::LlmError { session_id, turn_id, error } => {
                            // Send updated turn list so UI sees the error status from DB
                            if let Ok(turns) = db::get_turns(&pool, &session_id).await {
                                let _ = send_ws(&mut forward_tx, &WsOutgoing::TurnList {
                                    session_id: session_id.clone(),
                                    turns,
                                }).await;
                            }
                            Some(WsOutgoing::LlmError { session_id, turn_id, error })
                        }
                        Event::SessionSwitched { session_id } => {
                            // Send SessionSwitched first (clears client turns), then TurnList
                            let _ = send_ws(&mut forward_tx, &WsOutgoing::SessionSwitched {
                                session_id: session_id.clone(),
                            }).await;
                            if let Ok(turns) = db::get_turns(&pool, &session_id).await {
                                let _ = send_ws(&mut forward_tx, &WsOutgoing::TurnList {
                                    session_id: session_id.clone(),
                                    turns,
                                }).await;
                            }
                            if let Ok(Some(tokens)) = db::get_session_tokens(&pool, &session_id).await {
                                let _ = send_ws(&mut forward_tx, &WsOutgoing::ContextUsage { total_tokens: tokens, context_window: app_config::openclaw_context_window() }).await;
                            }
                            None // Already sent manually
                        }
                        Event::SessionCreated { session } => {
                            let s = db::Session {
                                id: session.id, name: session.name,
                                created_at: session.created_at, last_used: session.last_used,
                            };
                            Some(WsOutgoing::SessionCreated { session: s })
                        }
                        Event::SessionRenamed { session_id, name } =>
                            Some(WsOutgoing::SessionRenamed { session_id, name }),
                        Event::SessionDeleted { session_id } =>
                            Some(WsOutgoing::SessionDeleted { session_id }),
                        Event::VoiceToggled { enabled } =>
                            Some(WsOutgoing::VoiceToggled { enabled }),
                        Event::ShowHelp => Some(WsOutgoing::ShowHelp),
                        Event::FinalTranscript { .. } => None,
                        _ => None,
                    };
                    if let Some(msg) = msg {
                        if send_ws(&mut forward_tx, &msg).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("WebSocket client lagged by {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Handle incoming WebSocket messages
    let tx = state.tx.clone();
    let pool = state.pool.clone();
    while let Some(Ok(msg)) = ws_rx.next().await {
        if let Message::Text(text) = msg {
            if let Ok(incoming) = serde_json::from_str::<WsIncoming>(&text) {
                handle_ws_message(incoming, &tx, &pool).await;
            }
        }
    }

    forward_handle.abort();
}

fn save_base64_image(data_url: &str, dir: &std::path::Path) -> Option<String> {
    let parts: Vec<&str> = data_url.splitn(2, ',').collect();
    if parts.len() != 2 { return None; }

    let header = parts[0];
    let b64_data = parts[1];

    let ext = if header.contains("image/png") { "png" }
        else if header.contains("image/jpeg") { "jpg" }
        else if header.contains("image/gif") { "gif" }
        else if header.contains("image/webp") { "webp" }
        else { "png" };

    use base64::Engine;
    let bytes = match base64::engine::general_purpose::STANDARD.decode(b64_data) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to decode base64 image: {}", e);
            return None;
        }
    };

    let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    let path = dir.join(&filename);
    match std::fs::write(&path, &bytes) {
        Ok(_) => Some(path.to_string_lossy().to_string()),
        Err(e) => {
            tracing::error!("Failed to write image: {}", e);
            None
        }
    }
}

fn save_document(data_url: &str, original_filename: &str, dir: &std::path::Path) -> Option<(String, String)> {
    let parts: Vec<&str> = data_url.splitn(2, ',').collect();
    if parts.len() != 2 { return None; }
    let header = parts[0];
    let b64_data = parts[1];
    let ext = if header.contains("application/pdf") { "pdf" }
        else if header.contains("text/csv") { "csv" }
        else if header.contains("application/json") { "json" }
        else if header.contains("text/html") { "html" }
        else if header.contains("text/markdown") { "md" }
        else if header.contains("text/plain") { "txt" }
        else if header.contains("presentationml.presentation") { "pptx" }
        else {
            std::path::Path::new(original_filename)
                .extension().and_then(|e| e.to_str()).unwrap_or("txt")
        };
    use base64::Engine;
    let bytes = match base64::engine::general_purpose::STANDARD.decode(b64_data) {
        Ok(b) => b,
        Err(e) => { tracing::error!("Failed to decode base64 document: {}", e); return None; }
    };
    if bytes.len() > 5 * 1024 * 1024 {
        tracing::warn!("Document too large: {} bytes (max 5MB)", bytes.len());
        return None;
    }
    let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    let path = dir.join(&filename);
    match std::fs::write(&path, &bytes) {
        Ok(_) => Some((path.to_string_lossy().to_string(), original_filename.to_string())),
        Err(e) => { tracing::error!("Failed to write document: {}", e); None }
    }
}

async fn handle_ws_message(msg: WsIncoming, tx: &EventSender, pool: &SqlitePool) {
    match msg {
        WsIncoming::SendMessage { session_id, text, images, documents } => {
            let mut image_paths: Vec<String> = Vec::new();
            if !images.is_empty() {
                let images_dir = app_config::base_dir().join("images");
                let _ = std::fs::create_dir_all(&images_dir);
                for data_url in &images {
                    if let Some(saved) = save_base64_image(data_url, &images_dir) {
                        image_paths.push(saved);
                    }
                }
            }
            let mut doc_entries: Vec<(String, String)> = Vec::new();
            if !documents.is_empty() {
                let docs_dir = app_config::base_dir().join("documents");
                let _ = std::fs::create_dir_all(&docs_dir);
                for doc in &documents {
                    if let Some(entry) = save_document(&doc.data, &doc.filename, &docs_dir) {
                        doc_entries.push(entry);
                    }
                }
            }
            let _ = tx.send(Event::UserTextMessage { session_id, text, images: image_paths, documents: doc_entries });
        }
        WsIncoming::SwitchSession { session_id } => {
            let _ = db::set_active_session_id(pool, &session_id).await;
            let _ = tx.send(Event::SessionSwitched { session_id });
        }
        WsIncoming::CreateSession => {
            let sessions = db::get_sessions(pool).await.unwrap_or_default();
            let existing_names: Vec<String> = sessions.iter().map(|s| s.name.clone()).collect();
            let name = crate::commands::generate_session_name(&existing_names);
            let now = chrono::Utc::now().to_rfc3339();
            let session = db::Session {
                id: format!("stomp-{}", uuid::Uuid::new_v4()),
                name: name.clone(),
                created_at: now.clone(),
                last_used: now,
            };
            let _ = db::create_session(pool, &session).await;
            let _ = db::set_active_session_id(pool, &session.id).await;
            let _ = tx.send(Event::SessionCreated {
                session: crate::events::SessionInfo {
                    id: session.id.clone(),
                    name: session.name,
                    created_at: session.created_at,
                    last_used: session.last_used,
                },
            });
            let _ = tx.send(Event::SessionSwitched { session_id: session.id });
        }
        WsIncoming::RenameSession { session_id, name } => {
            let _ = db::rename_session(pool, &session_id, &name).await;
            let _ = tx.send(Event::SessionRenamed { session_id, name });
        }
        WsIncoming::DeleteSession { session_id } => {
            let _ = db::delete_session(pool, &session_id).await;
            let _ = tx.send(Event::SessionDeleted { session_id });
            // Switch to next available session or create one
            let remaining = db::get_sessions(pool).await.unwrap_or_default();
            if let Some(next) = remaining.first() {
                let _ = db::set_active_session_id(pool, &next.id).await;
                let _ = tx.send(Event::SessionSwitched { session_id: next.id.clone() });
            } else {
                // No sessions left — create one
                let name = crate::commands::generate_session_name(&[]);
                let now = chrono::Utc::now().to_rfc3339();
                let session = db::Session {
                    id: format!("stomp-{}", uuid::Uuid::new_v4()),
                    name,
                    created_at: now.clone(),
                    last_used: now,
                };
                let _ = db::create_session(pool, &session).await;
                let _ = db::set_active_session_id(pool, &session.id).await;
                let _ = tx.send(Event::SessionCreated {
                    session: crate::events::SessionInfo {
                        id: session.id.clone(),
                        name: session.name,
                        created_at: session.created_at,
                        last_used: session.last_used,
                    },
                });
                let _ = tx.send(Event::SessionSwitched { session_id: session.id });
            }
        }
        WsIncoming::CancelRecording => {
            let _ = tx.send(Event::CancelRecording);
        }
        WsIncoming::ToggleVoice => {
            let current = db::get_config(pool, "voice_enabled").await
                .ok().flatten()
                .map(|v| v == "true")
                .unwrap_or(true);
            let new_val = !current;
            let _ = db::set_config(pool, "voice_enabled", if new_val { "true" } else { "false" }).await;
            let _ = tx.send(Event::VoiceToggled { enabled: new_val });
        }
    }
}

async fn send_ws(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    msg: &WsOutgoing,
) -> Result<(), String> {
    if let Ok(json) = serde_json::to_string(msg) {
        tx.send(Message::Text(json.into())).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Find TLS cert/key files. Checks TLS_CERT/TLS_KEY env vars first,
/// then looks for any *.crt/*.key pair in the home directory.
fn find_tls_files() -> (Option<std::path::PathBuf>, Option<std::path::PathBuf>) {
    use std::path::PathBuf;

    // Check environment variables first
    if let (Ok(cert), Ok(key)) = (std::env::var("TLS_CERT"), std::env::var("TLS_KEY")) {
        let cert = PathBuf::from(cert);
        let key = PathBuf::from(key);
        if cert.exists() && key.exists() {
            return (Some(cert), Some(key));
        }
    }

    // Fall back to scanning home dir for *.ts.net.crt / *.ts.net.key
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (None, None),
    };

    let Ok(entries) = std::fs::read_dir(&home) else {
        return (None, None);
    };

    let mut cert = None;
    let mut key = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".ts.net.crt") {
            cert = Some(entry.path());
        } else if name.ends_with(".ts.net.key") {
            key = Some(entry.path());
        }
        if cert.is_some() && key.is_some() {
            break;
        }
    }
    (cert, key)
}
