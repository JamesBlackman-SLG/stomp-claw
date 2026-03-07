use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
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
    LlmDone { session_id: String, turn_id: i64, content: String },
    LlmError { session_id: String, turn_id: i64, error: String },
    VoiceToggled { enabled: bool },
    Config { voice_enabled: bool, active_session_id: String },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum WsIncoming {
    SendMessage { session_id: String, text: String },
    SwitchSession { session_id: String },
    CreateSession,
    RenameSession { session_id: String, name: String },
    DeleteSession { session_id: String },
    ToggleVoice,
}

// --- Embedded frontend assets ---

#[derive(Embed)]
#[folder = "ui/dist/client"]
struct FrontendAssets;

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
        .fallback(static_handler)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(app_config::SERVER_ADDR)
        .await
        .expect("Failed to bind server");

    tracing::info!("Server listening on {}", app_config::SERVER_ADDR);
    axum::serve(listener, app).await.expect("Server failed");
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
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
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
                session_id: active_session_id,
                turns,
            }).await;
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
                        Event::LlmThinking { session_id, turn_id } =>
                            Some(WsOutgoing::LlmThinking { session_id, turn_id }),
                        Event::LlmToken { session_id, turn_id, token, accumulated } =>
                            Some(WsOutgoing::LlmToken { session_id, turn_id, token, accumulated }),
                        Event::LlmDone { session_id, turn_id, full_response } =>
                            Some(WsOutgoing::LlmDone { session_id, turn_id, content: full_response }),
                        Event::LlmError { session_id, turn_id, error } =>
                            Some(WsOutgoing::LlmError { session_id, turn_id, error }),
                        Event::SessionSwitched { session_id } => {
                            // Also send turns for new session
                            if let Ok(turns) = db::get_turns(&pool, &session_id).await {
                                let _ = send_ws(&mut forward_tx, &WsOutgoing::TurnList {
                                    session_id: session_id.clone(),
                                    turns,
                                }).await;
                            }
                            Some(WsOutgoing::SessionSwitched { session_id })
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
                        Event::FinalTranscript { session_id, text: _ } => {
                            // Send user turn creation to UI
                            if let Ok(turns) = db::get_turns(&pool, &session_id).await {
                                if let Some(turn) = turns.last() {
                                    let _ = send_ws(&mut forward_tx, &WsOutgoing::TurnCreated {
                                        turn: turn.clone(),
                                    }).await;
                                }
                            }
                            None
                        }
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

async fn handle_ws_message(msg: WsIncoming, tx: &EventSender, pool: &SqlitePool) {
    match msg {
        WsIncoming::SendMessage { session_id, text } => {
            let _ = tx.send(Event::UserTextMessage { session_id, text });
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
