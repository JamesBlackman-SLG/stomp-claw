mod events;
mod midi;
mod audio;
mod transcription;
mod llm;
mod commands;
mod db;
mod server;
mod config;
mod beep;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Ensure base directory exists
    std::fs::create_dir_all(config::base_dir()).ok();

    // Initialize logging
    let log_dir = config::base_dir();
    let file_appender = tracing_appender::rolling::never(&log_dir, "stomp-claw.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("stomp_claw=info".parse().unwrap()))
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    tracing::info!("Starting stomp-claw v2");

    // Initialize database
    let pool = db::create_pool().await.expect("Failed to create database pool");

    // Migrate from v1 if needed
    if let Err(e) = db::migrate_from_v1(&pool).await {
        tracing::warn!("v1 migration error (non-fatal): {}", e);
    }

    // Ensure at least one session exists
    let sessions = db::get_sessions(&pool).await.unwrap_or_default();
    if sessions.is_empty() {
        let now = chrono::Utc::now().to_rfc3339();
        let name = commands::generate_session_name(&[]);
        let session = db::Session {
            id: format!("stomp-{}", uuid::Uuid::new_v4()),
            name,
            created_at: now.clone(),
            last_used: now,
        };
        db::create_session(&pool, &session).await.expect("Failed to create initial session");
        db::set_active_session_id(&pool, &session.id).await.expect("Failed to set active session");
        tracing::info!("Created initial session: {}", session.name);
    } else if db::get_active_session_id(&pool).await.ok().flatten().is_none() {
        // Set first session as active if none set
        db::set_active_session_id(&pool, &sessions[0].id).await.ok();
    }

    // Create event bus
    let (tx, _rx) = events::create_event_bus(2048);

    // Spawn modules
    let voice_enabled = db::get_config(&pool, "voice_enabled").await
        .ok().flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    let beep_rx = tx.subscribe();
    tokio::spawn(beep::run(beep_rx, voice_enabled));

    // Audio runs on a dedicated thread (cpal::Stream is not Send)
    let audio_tx = tx.clone();
    let audio_rx = tx.subscribe();
    let audio_pool = pool.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create audio runtime");
        rt.block_on(audio::run(audio_tx, audio_rx, audio_pool));
    });

    let trans_tx = tx.clone();
    let trans_rx = tx.subscribe();
    let trans_pool = pool.clone();
    tokio::spawn(transcription::run(trans_tx, trans_rx, trans_pool));

    let llm_tx = tx.clone();
    let llm_rx = tx.subscribe();
    let llm_pool = pool.clone();
    tokio::spawn(llm::run(llm_tx, llm_rx, llm_pool));

    // Voice command handler (session management, voice toggle)
    let cmd_tx = tx.clone();
    let cmd_rx = tx.subscribe();
    let cmd_pool = pool.clone();
    tokio::spawn(handle_voice_commands(cmd_tx, cmd_rx, cmd_pool));

    // Emit initial SessionSwitched so all modules know the active session
    if let Some(active_id) = db::get_active_session_id(&pool).await.ok().flatten() {
        tracing::info!("Active session: {}", active_id);
        let _ = tx.send(events::Event::SessionSwitched { session_id: active_id });
    }

    // MIDI runs on a std::thread (midir callback requirement)
    let midi_tx = tx.clone();
    std::thread::spawn(move || {
        midi::run(midi_tx);
    });

    // Startup sound
    beep::beep_up();
    tracing::info!("stomp-claw v2 ready");

    // Server runs on the main tokio task (blocks)
    let server_tx = tx.clone();
    let server_rx = tx.subscribe();
    server::run(server_tx, server_rx, pool).await;
}

async fn handle_voice_commands(
    tx: events::EventSender,
    mut rx: events::EventReceiver,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Ok(events::Event::VoiceCommand { command }) => {
                match command {
                    events::Command::NewSession => {
                        let sessions = db::get_sessions(&pool).await.unwrap_or_default();
                        let names: Vec<String> = sessions.iter().map(|s| s.name.clone()).collect();
                        let name = commands::generate_session_name(&names);
                        let now = chrono::Utc::now().to_rfc3339();
                        let session = db::Session {
                            id: format!("stomp-{}", uuid::Uuid::new_v4()),
                            name: name.clone(),
                            created_at: now.clone(),
                            last_used: now,
                        };
                        let _ = db::create_session(&pool, &session).await;
                        let _ = db::set_active_session_id(&pool, &session.id).await;
                        let _ = tx.send(events::Event::SessionCreated {
                            session: events::SessionInfo {
                                id: session.id.clone(), name: session.name,
                                created_at: session.created_at, last_used: session.last_used,
                            },
                        });
                        let _ = tx.send(events::Event::SessionSwitched { session_id: session.id });
                        beep::play_session_tone(0);
                    }
                    events::Command::SwitchSession(query) => {
                        tracing::info!("Processing SwitchSession command: '{}'", query);
                        let sessions = db::get_sessions(&pool).await.unwrap_or_default();
                        let names: Vec<String> = sessions.iter().map(|s| s.name.clone()).collect();

                        // Try number first
                        if let Ok(num) = query.parse::<usize>() {
                            if num > 0 && num <= sessions.len() {
                                let session = &sessions[num - 1];
                                tracing::info!("Switching to session #{}: {}", num, session.name);
                                let _ = db::set_active_session_id(&pool, &session.id).await;
                                let _ = tx.send(events::Event::SessionSwitched { session_id: session.id.clone() });
                                beep::play_session_tone(num - 1);
                            }
                        } else if let Some(matched_name) = commands::fuzzy_match_session(&query, &names) {
                            if let Some(session) = sessions.iter().find(|s| s.name == matched_name) {
                                tracing::info!("Switching to session by name: '{}'", matched_name);
                                let _ = db::set_active_session_id(&pool, &session.id).await;
                                let _ = tx.send(events::Event::SessionSwitched { session_id: session.id.clone() });
                            }
                        } else {
                            tracing::warn!("No session matched for query: '{}' (available: {:?})", query, names);
                        }
                    }
                    events::Command::ListSessions => {
                        // Sessions are visible in UI; voice feedback could be added later
                    }
                    events::Command::RenameSession(new_name) => {
                        if let Some(id) = db::get_active_session_id(&pool).await.ok().flatten() {
                            let _ = db::rename_session(&pool, &id, &new_name).await;
                            let _ = tx.send(events::Event::SessionRenamed { session_id: id, name: new_name });
                        }
                    }
                    events::Command::DeleteSession => {
                        tracing::info!("Delete session requested");
                        if let Some(id) = db::get_active_session_id(&pool).await.ok().flatten() {
                            let session_name = db::get_session(&pool, &id).await
                                .ok().flatten()
                                .map(|s| s.name)
                                .unwrap_or_default();
                            let _ = db::delete_session(&pool, &id).await;
                            let _ = tx.send(events::Event::SessionDeleted { session_id: id });
                            beep::speak(&format!("Deleted {}", session_name));
                            // Switch to another session or create new
                            let remaining = db::get_sessions(&pool).await.unwrap_or_default();
                            if let Some(next) = remaining.first() {
                                let _ = db::set_active_session_id(&pool, &next.id).await;
                                let _ = tx.send(events::Event::SessionSwitched { session_id: next.id.clone() });
                            } else {
                                let _ = tx.send(events::Event::VoiceCommand { command: events::Command::NewSession });
                            }
                        }
                    }
                    events::Command::VoiceOn => {
                        let _ = db::set_config(&pool, "voice_enabled", "true").await;
                        let _ = tx.send(events::Event::VoiceToggled { enabled: true });
                    }
                    events::Command::VoiceOff => {
                        let _ = db::set_config(&pool, "voice_enabled", "false").await;
                        let _ = tx.send(events::Event::VoiceToggled { enabled: false });
                    }
                    events::Command::Help => {
                        let _ = tx.send(events::Event::ShowHelp);
                    }
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Command handler lagged by {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
