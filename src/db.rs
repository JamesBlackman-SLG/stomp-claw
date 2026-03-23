use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use serde::{Deserialize, Serialize};
use crate::config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_used: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub images: Option<String>,
    pub documents: Option<String>,
}

pub async fn create_pool() -> Result<SqlitePool, sqlx::Error> {
    let db_path = config::db_path();

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;

    run_migrations(&pool).await?;
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_used TEXT NOT NULL
        )"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS turns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'complete',
            created_at TEXT NOT NULL,
            completed_at TEXT,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id, id)"
    ).execute(pool).await?;

    let _ = sqlx::query("ALTER TABLE turns ADD COLUMN images TEXT")
        .execute(pool).await;

    let _ = sqlx::query("ALTER TABLE turns ADD COLUMN documents TEXT")
        .execute(pool).await;

    let _ = sqlx::query("ALTER TABLE sessions ADD COLUMN deleted_at TEXT")
        .execute(pool).await;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )"
    ).execute(pool).await?;

    // Enable WAL mode for better concurrent access
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
    // Enable foreign keys
    sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;

    Ok(())
}

// --- Session CRUD ---

pub async fn get_sessions(pool: &SqlitePool) -> Result<Vec<Session>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, name, created_at, last_used FROM sessions WHERE deleted_at IS NULL ORDER BY last_used DESC")
        .fetch_all(pool).await?;
    Ok(rows.iter().map(|r| Session {
        id: r.get("id"),
        name: r.get("name"),
        created_at: r.get("created_at"),
        last_used: r.get("last_used"),
    }).collect())
}

pub async fn create_session(pool: &SqlitePool, session: &Session) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO sessions (id, name, created_at, last_used) VALUES (?, ?, ?, ?)")
        .bind(&session.id)
        .bind(&session.name)
        .bind(&session.created_at)
        .bind(&session.last_used)
        .execute(pool).await?;
    Ok(())
}

pub async fn rename_session(pool: &SqlitePool, id: &str, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sessions SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(pool).await?;
    Ok(())
}

pub async fn delete_session(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE sessions SET deleted_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(pool).await?;
    Ok(())
}

pub async fn touch_session(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE sessions SET last_used = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(pool).await?;
    Ok(())
}

// --- Turn CRUD ---

pub async fn get_turns(pool: &SqlitePool, session_id: &str) -> Result<Vec<Turn>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, session_id, role, content, status, created_at, completed_at, images, documents FROM turns WHERE session_id = ? ORDER BY id")
        .bind(session_id)
        .fetch_all(pool).await?;
    Ok(rows.iter().map(|r| Turn {
        id: r.get("id"),
        session_id: r.get("session_id"),
        role: r.get("role"),
        content: r.get("content"),
        status: r.get("status"),
        created_at: r.get("created_at"),
        completed_at: r.get("completed_at"),
        images: r.get("images"),
        documents: r.get("documents"),
    }).collect())
}

pub async fn create_turn(pool: &SqlitePool, session_id: &str, role: &str, content: &str, status: &str) -> Result<i64, sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query("INSERT INTO turns (session_id, role, content, status, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(status)
        .bind(&now)
        .execute(pool).await?;
    Ok(result.last_insert_rowid())
}


pub async fn create_turn_with_attachments(
    pool: &SqlitePool, session_id: &str, role: &str, content: &str, status: &str,
    images: Option<&str>, documents: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "INSERT INTO turns (session_id, role, content, status, created_at, images, documents) VALUES (?, ?, ?, ?, ?, ?, ?)"
    ).bind(session_id).bind(role).bind(content).bind(status).bind(&now).bind(images).bind(documents)
        .execute(pool).await?;
    Ok(result.last_insert_rowid())
}


pub async fn update_turn_content(pool: &SqlitePool, turn_id: i64, content: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE turns SET content = ? WHERE id = ?")
        .bind(content)
        .bind(turn_id)
        .execute(pool).await?;
    Ok(())
}

pub async fn complete_turn(pool: &SqlitePool, turn_id: i64, content: &str) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE turns SET content = ?, status = 'complete', completed_at = ? WHERE id = ?")
        .bind(content)
        .bind(&now)
        .bind(turn_id)
        .execute(pool).await?;
    Ok(())
}

pub async fn error_turn(pool: &SqlitePool, turn_id: i64, content: &str) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE turns SET content = ?, status = 'error', completed_at = ? WHERE id = ?")
        .bind(content)
        .bind(&now)
        .bind(turn_id)
        .execute(pool).await?;
    Ok(())
}

// --- Config ---

pub async fn get_config(pool: &SqlitePool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("SELECT value FROM config WHERE key = ?")
        .bind(key)
        .fetch_optional(pool).await?;
    Ok(row.map(|r| r.get("value")))
}

pub async fn set_config(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO config (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = ?")
        .bind(key)
        .bind(value)
        .bind(value)
        .execute(pool).await?;
    Ok(())
}

// --- Active session tracking ---

pub async fn get_active_session_id(pool: &SqlitePool) -> Result<Option<String>, sqlx::Error> {
    get_config(pool, "active_session_id").await
}

pub async fn set_active_session_id(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    set_config(pool, "active_session_id", id).await
}

pub async fn get_session_tokens(pool: &SqlitePool, session_id: &str) -> Result<Option<u32>, sqlx::Error> {
    let key = format!("context_tokens:{}", session_id);
    let val = get_config(pool, &key).await?;
    Ok(val.and_then(|v| v.parse().ok()))
}

pub async fn set_session_tokens(pool: &SqlitePool, session_id: &str, total_tokens: u32) -> Result<(), sqlx::Error> {
    let key = format!("context_tokens:{}", session_id);
    set_config(pool, &key, &total_tokens.to_string()).await
}

// --- Migration from v1 ---

pub async fn migrate_from_v1(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let base = config::base_dir();
    let sessions_file = base.join("sessions.json");

    if !sessions_file.exists() {
        return Ok(());
    }

    // Check if we already migrated
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(pool).await?;
    if count > 0 {
        return Ok(());
    }

    tracing::info!("Migrating v1 data to SQLite...");

    // Read v1 sessions
    let content = std::fs::read_to_string(&sessions_file)?;
    let v1_sessions: Vec<Session> = serde_json::from_str(&content)?;

    for session in &v1_sessions {
        create_session(pool, session).await?;

        // Migrate turns from JSON files
        let turns_dir = base.join("conversations").join(&session.id);
        if turns_dir.exists() {
            let mut entries: Vec<_> = std::fs::read_dir(&turns_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .collect();
            entries.sort_by_key(|e| e.file_name());

            for entry in entries {
                let turn_content = std::fs::read_to_string(entry.path())?;
                if let Ok(v1_turn) = serde_json::from_str::<serde_json::Value>(&turn_content) {
                    let timestamp = v1_turn["timestamp"].as_str().unwrap_or("");
                    if let Some(user_msg) = v1_turn["user"].as_str() {
                        let now = timestamp.to_string();
                        sqlx::query("INSERT INTO turns (session_id, role, content, status, created_at, completed_at) VALUES (?, 'user', ?, 'complete', ?, ?)")
                            .bind(&session.id)
                            .bind(user_msg)
                            .bind(&now)
                            .bind(&now)
                            .execute(pool).await?;
                    }
                    if let Some(assistant_msg) = v1_turn["assistant"].as_str() {
                        let now = timestamp.to_string();
                        sqlx::query("INSERT INTO turns (session_id, role, content, status, created_at, completed_at) VALUES (?, 'assistant', ?, 'complete', ?, ?)")
                            .bind(&session.id)
                            .bind(assistant_msg)
                            .bind(&now)
                            .bind(&now)
                            .execute(pool).await?;
                    }
                }
            }
        }
    }

    // Migrate active session
    let session_file = base.join("session.txt");
    if session_file.exists() {
        let active_id = std::fs::read_to_string(&session_file)?.trim().to_string();
        set_active_session_id(pool, &active_id).await?;
    }

    // Migrate config
    let config_file = base.join("config.toml");
    if config_file.exists() {
        let config_str = std::fs::read_to_string(&config_file)?;
        if config_str.contains("voice_enabled = true") {
            set_config(pool, "voice_enabled", "true").await?;
        } else {
            set_config(pool, "voice_enabled", "false").await?;
        }
    }

    tracing::info!("v1 migration complete");
    Ok(())
}
