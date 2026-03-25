# Agent Switching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow switching between OpenClaw agents (e.g. Alan, Meg) via a dropdown in the web UI, with sessions scoped per agent and the selection persisted across restarts.

**Architecture:** Agent discovery reads `~/.openclaw/openclaw.json` for agent IDs/workspaces, then parses each agent's `{workspace}/IDENTITY.md` to extract the display name. The active agent ID is stored in the SQLite config table. Sessions gain an `agent_id` column so the sidebar filters by the active agent. The LLM module reads the active agent from config when sending requests.

**Tech Stack:** Rust (Axum, sqlx, serde_json), React 19, Tailwind v4, WebSocket

---

## File Map

**Rust backend:**
- Modify: `src/config.rs` — add agent discovery functions
- Modify: `src/db.rs` — add `agent_id` column to sessions, filter queries, migration
- Modify: `src/events.rs` — add `AgentSwitched` event
- Modify: `src/server.rs` — add WS messages for agent list/switch, REST endpoints, include active agent in config
- Modify: `src/llm.rs` — use active agent ID from DB config instead of hardcoded `"main"`
- Modify: `src/main.rs` — initialize active agent on startup
- Modify: `src/audio.rs` — update `get_sessions` call to pass active agent
- Modify: `src/transcription.rs` — update `get_sessions` call to pass active agent

**React frontend:**
- Create: `ui/app/components/AgentSelector.tsx` — dropdown component
- Modify: `ui/app/lib/types.ts` — add agent types and WS message variants
- Modify: `ui/app/lib/state.tsx` — add agents/activeAgentId to state, handle new WS messages
- Modify: `ui/app/routes/index.tsx` — place AgentSelector in header

---

### Task 1: Agent Discovery in config.rs

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add the `Agent` struct and `discover_agents` function**

Add to `src/config.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
}

/// Discover agents from OpenClaw config file.
/// Reads ~/.openclaw/openclaw.json for agent list, then each agent's
/// IDENTITY.md to extract the display name.
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

        // Determine workspace: agent-specific or default
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

/// Parse IDENTITY.md to extract the agent's display name.
/// Looks for a line like "- **Name:** Alan"
fn read_identity_name(workspace_path: &str) -> Option<String> {
    let identity_path = std::path::Path::new(workspace_path).join("IDENTITY.md");
    let content = std::fs::read_to_string(&identity_path).ok()?;
    for line in content.lines() {
        // Match "- **Name:** <value>" or "**Name:** <value>"
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /home/jb/code/stomp-claw && cargo check 2>&1 | tail -5`
Expected: no errors (warnings ok)

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: add OpenClaw agent discovery from config + IDENTITY.md"
```

---

### Task 2: Database Migration — agent_id on Sessions

**Files:**
- Modify: `src/db.rs`

- [ ] **Step 1: Add agent_id column migration and update Session struct**

In `src/db.rs`, add the `agent_id` field to the `Session` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_used: String,
    #[serde(default = "default_agent_id")]
    pub agent_id: String,
}

fn default_agent_id() -> String { "main".to_string() }
```

In `run_migrations`, add after the existing `ALTER TABLE` statements:

```rust
    // Add agent_id column, defaulting existing sessions to "main"
    let _ = sqlx::query("ALTER TABLE sessions ADD COLUMN agent_id TEXT NOT NULL DEFAULT 'main'")
        .execute(pool).await;
```

- [ ] **Step 2: Update all Session queries to include agent_id**

Update `get_sessions` to accept an `agent_id` parameter and filter:

```rust
pub async fn get_sessions(pool: &SqlitePool, agent_id: &str) -> Result<Vec<Session>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, name, created_at, last_used, agent_id FROM sessions WHERE deleted_at IS NULL AND agent_id = ? ORDER BY last_used DESC")
        .bind(agent_id)
        .fetch_all(pool).await?;
    Ok(rows.iter().map(|r| Session {
        id: r.get("id"),
        name: r.get("name"),
        created_at: r.get("created_at"),
        last_used: r.get("last_used"),
        agent_id: r.get("agent_id"),
    }).collect())
}
```

Update `create_session` to persist agent_id:

```rust
pub async fn create_session(pool: &SqlitePool, session: &Session) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO sessions (id, name, created_at, last_used, agent_id) VALUES (?, ?, ?, ?, ?)")
        .bind(&session.id)
        .bind(&session.name)
        .bind(&session.created_at)
        .bind(&session.last_used)
        .bind(&session.agent_id)
        .execute(pool).await?;
    Ok(())
}
```

- [ ] **Step 3: Add active_agent helpers**

```rust
pub async fn get_active_agent_id(pool: &SqlitePool) -> Result<Option<String>, sqlx::Error> {
    get_config(pool, "active_agent_id").await
}

pub async fn set_active_agent_id(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    set_config(pool, "active_agent_id", id).await
}
```

- [ ] **Step 4: Fix all call sites that use get_sessions and create_session**

Every call to `db::get_sessions(&pool)` must now pass an agent_id. Search the codebase for all call sites:

- `src/main.rs:41` — startup session check. Get active agent first:
  ```rust
  let active_agent_id = db::get_active_agent_id(&pool).await
      .ok().flatten()
      .unwrap_or_else(|| "main".to_string());
  // ... use active_agent_id in get_sessions and create_session calls
  ```
- `src/main.rs:44` — `generate_session_name`: pass `get_sessions(&pool, &active_agent_id)`
- `src/main.rs:46-50` — `Session { ... }`: add `agent_id: active_agent_id.clone()`
- `src/main.rs:127-136` — `handle_voice_commands` NewSession: get active agent from DB
- `src/main.rs:149-150` — SwitchSession: pass active agent
- `src/server.rs:274` — `get_sessions` REST handler: read active agent and pass to query
- `src/server.rs:582-583` — CreateSession handler: get active agent from DB
- `src/server.rs:612` — DeleteSession handler: get active agent to find remaining sessions
- `src/audio.rs:133` — session names for voice command parsing: read active agent from DB, pass to `get_sessions`
- `src/transcription.rs:64` — session names for voice command parsing: read active agent from DB, pass to `get_sessions`

Also update the v1 migration in `migrate_from_v1` — existing sessions get `agent_id: "main"`. The `#[serde(default)]` on `agent_id` handles deserialization of old JSON files that lack the field. The SQL DEFAULT handles existing DB rows.

- [ ] **Step 5: Verify it compiles**

Run: `cd /home/jb/code/stomp-claw && cargo check 2>&1 | tail -10`
Expected: no errors

- [ ] **Step 6: Commit**

```bash
git add src/db.rs src/main.rs src/server.rs src/audio.rs src/transcription.rs
git commit -m "feat: add agent_id column to sessions, filter by active agent"
```

---

### Task 3: Events — Add AgentSwitched Event

**Files:**
- Modify: `src/events.rs`

- [ ] **Step 1: Add AgentSwitched event variant**

Add to the `Event` enum in `src/events.rs`:

```rust
    // Agent
    AgentSwitched { agent_id: String },
```

- [ ] **Step 2: Commit**

```bash
git add src/events.rs
git commit -m "feat: add AgentSwitched event"
```

---

### Task 4: Server — WS + REST Agent Support

**Files:**
- Modify: `src/server.rs`

- [ ] **Step 1: Add agent types to WS messages**

Add to `WsOutgoing` enum:

```rust
    AgentList { agents: Vec<crate::config::Agent> },
    AgentSwitched { agent_id: String },
```

Add to `WsIncoming` enum:

```rust
    SwitchAgent { agent_id: String },
```

Update the existing `Config` variant in `WsOutgoing` to include the active agent:

```rust
    Config { voice_enabled: bool, active_session_id: String, active_agent_id: String },
```

- [ ] **Step 2: Update handle_ws to send agent list and active agent on connect**

In `handle_ws`, after sending the initial `Config` message, also send the agent list and include `active_agent_id` in the Config:

```rust
    let active_agent_id = db::get_active_agent_id(&state.pool).await
        .ok().flatten()
        .unwrap_or_else(|| "main".to_string());

    let _ = send_ws(&mut ws_tx, &WsOutgoing::Config {
        voice_enabled,
        active_session_id: active_session_id.clone(),
        active_agent_id: active_agent_id.clone(),
    }).await;

    // Send agent list
    let agents = crate::config::discover_agents();
    let _ = send_ws(&mut ws_tx, &WsOutgoing::AgentList { agents }).await;
```

- [ ] **Step 3: Handle SwitchAgent in handle_ws_message**

Add the `SwitchAgent` match arm:

```rust
        WsIncoming::SwitchAgent { agent_id } => {
            let _ = db::set_active_agent_id(pool, &agent_id).await;
            // Switch to the most recent session for this agent, or create one
            let sessions = db::get_sessions(pool, &agent_id).await.unwrap_or_default();
            if let Some(session) = sessions.first() {
                let _ = db::set_active_session_id(pool, &session.id).await;
                let _ = tx.send(Event::AgentSwitched { agent_id });
                let _ = tx.send(Event::SessionSwitched { session_id: session.id.clone() });
            } else {
                // Create first session for this agent
                let name = crate::commands::generate_session_name(&[]);
                let now = chrono::Utc::now().to_rfc3339();
                let session = db::Session {
                    id: format!("stomp-{}", uuid::Uuid::new_v4()),
                    name: name.clone(),
                    created_at: now.clone(),
                    last_used: now,
                    agent_id: agent_id.clone(),
                };
                let _ = db::create_session(pool, &session).await;
                let _ = db::set_active_session_id(pool, &session.id).await;
                let _ = tx.send(Event::AgentSwitched { agent_id });
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
```

- [ ] **Step 4: Forward AgentSwitched event to WebSocket clients**

In the event forwarding loop inside `handle_ws`, add:

```rust
                        Event::AgentSwitched { agent_id } => {
                            // Send agent switched, then updated session list for this agent
                            let _ = send_ws(&mut forward_tx, &WsOutgoing::AgentSwitched {
                                agent_id: agent_id.clone(),
                            }).await;
                            if let Ok(sessions) = db::get_sessions(&pool, &agent_id).await {
                                let _ = send_ws(&mut forward_tx, &WsOutgoing::SessionList { sessions }).await;
                            }
                            None // Already sent manually
                        }
```

- [ ] **Step 5: Update get_config REST endpoint to include active_agent_id**

```rust
async fn get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let voice = db::get_config(&state.pool, "voice_enabled").await
        .ok().flatten()
        .map(|v| v == "true")
        .unwrap_or(true);
    let session_id = db::get_active_session_id(&state.pool).await
        .ok().flatten()
        .unwrap_or_default();
    let agent_id = db::get_active_agent_id(&state.pool).await
        .ok().flatten()
        .unwrap_or_else(|| "main".to_string());
    Json(serde_json::json!({
        "voice_enabled": voice,
        "active_session_id": session_id,
        "active_agent_id": agent_id,
    }))
}
```

- [ ] **Step 6: Update /api/sessions to filter by active agent**

```rust
async fn get_sessions(State(state): State<AppState>) -> Json<Vec<db::Session>> {
    let agent_id = db::get_active_agent_id(&state.pool).await
        .ok().flatten()
        .unwrap_or_else(|| "main".to_string());
    let sessions = db::get_sessions(&state.pool, &agent_id).await.unwrap_or_default();
    Json(sessions)
}
```

- [ ] **Step 7: Add /api/agents REST endpoint**

Add the handler:

```rust
async fn get_agents() -> Json<Vec<crate::config::Agent>> {
    Json(crate::config::discover_agents())
}
```

Add route in the Router:

```rust
        .route("/api/agents", get(get_agents))
```

- [ ] **Step 8: Update session list sent on WS connect to filter by active agent**

In `handle_ws`, the session list sent on connect should filter by active agent:

```rust
    if let Ok(sessions) = db::get_sessions(&state.pool, &active_agent_id).await {
        let _ = send_ws(&mut ws_tx, &WsOutgoing::SessionList { sessions }).await;
    }
```

- [ ] **Step 9: Verify it compiles**

Run: `cd /home/jb/code/stomp-claw && cargo check 2>&1 | tail -10`
Expected: no errors

- [ ] **Step 10: Commit**

```bash
git add src/server.rs
git commit -m "feat: agent switching via WebSocket and REST API"
```

---

### Task 5: LLM — Use Active Agent ID

**Files:**
- Modify: `src/llm.rs`

- [ ] **Step 1: Pass agent_id through to send_to_llm and use it in the session key header**

Add `agent_id` parameter to `send_to_llm`:

```rust
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
    agent_id: &str,
) {
```

Replace the hardcoded `"main"` in the session key header (line 239):

```rust
        .header("x-openclaw-session-key", format!("agent:{}:{}", agent_id, session_id))
```

- [ ] **Step 2: Update the run loop to read active agent from DB**

In the `run` function, when handling `FinalTranscript` and `UserTextMessage`, read the active agent:

```rust
            Ok(Event::FinalTranscript { session_id, text }) => {
                tracing::info!("LLM: Received FinalTranscript: '{}'", text);
                let tx = tx.clone();
                let pool = pool.clone();
                let client = client.clone();
                tokio::spawn(async move {
                    let agent_id = db::get_active_agent_id(&pool).await
                        .ok().flatten()
                        .unwrap_or_else(|| "main".to_string());
                    let _ = db::touch_session(&pool, &session_id).await;
                    send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &[], &[], &agent_id).await;
                });
            }
            Ok(Event::UserTextMessage { session_id, text, images, documents }) => {
                let tx = tx.clone();
                let pool = pool.clone();
                let client = client.clone();
                tokio::spawn(async move {
                    let agent_id = db::get_active_agent_id(&pool).await
                        .ok().flatten()
                        .unwrap_or_else(|| "main".to_string());
                    let _ = db::touch_session(&pool, &session_id).await;
                    send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &images, &documents, &agent_id).await;
                });
            }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/jb/code/stomp-claw && cargo check 2>&1 | tail -10`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src/llm.rs
git commit -m "feat: use active agent ID in OpenClaw session key header"
```

---

### Task 6: Startup — Initialize Active Agent

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Set default active agent on startup if not set**

After database initialization and v1 migration, before the session existence check, add:

```rust
    // Initialize active agent if not set
    if db::get_active_agent_id(&pool).await.ok().flatten().is_none() {
        let agents = config::discover_agents();
        if let Some(first) = agents.first() {
            db::set_active_agent_id(&pool, &first.id).await.ok();
            tracing::info!("Set initial active agent: {} ({})", first.name, first.id);
        }
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /home/jb/code/stomp-claw && cargo check 2>&1 | tail -10`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: initialize active agent on startup"
```

---

### Task 7: Frontend Types — Agent WS Messages

**Files:**
- Modify: `ui/app/lib/types.ts`

- [ ] **Step 1: Add Agent interface and WS message types**

Add the `Agent` interface:

```typescript
export interface Agent {
  id: string
  name: string
}
```

Add to `WsMessage` union:

```typescript
  | { type: 'agent_list'; agents: Agent[] }
  | { type: 'agent_switched'; agent_id: string }
```

Update the existing `config` message type:

```typescript
  | { type: 'config'; voice_enabled: boolean; active_session_id: string; active_agent_id: string }
```

Add to `WsCommand` union:

```typescript
  | { type: 'switch_agent'; agent_id: string }
```

- [ ] **Step 2: Commit**

```bash
git add ui/app/lib/types.ts
git commit -m "feat: add agent types to frontend WS messages"
```

---

### Task 8: Frontend State — Agent State Management

**Files:**
- Modify: `ui/app/lib/state.tsx`

- [ ] **Step 1: Add agent fields to AppState**

Add to `AppState` interface:

```typescript
  agents: Agent[]
  activeAgentId: string
```

Import `Agent` from types. Add to `initialState`:

```typescript
  agents: [],
  activeAgentId: '',
```

- [ ] **Step 2: Handle new WS messages in reducer**

In the `ws_message` case, add handlers:

```typescript
        case 'agent_list':
          return { ...state, agents: msg.agents }
        case 'agent_switched':
          return { ...state, activeAgentId: msg.agent_id }
```

Update the existing `config` handler to also set `activeAgentId`:

```typescript
        case 'config':
          return { ...state, voiceEnabled: msg.voice_enabled, activeSessionId: msg.active_session_id, activeAgentId: msg.active_agent_id }
```

- [ ] **Step 3: Commit**

```bash
git add ui/app/lib/state.tsx
git commit -m "feat: add agent state management to frontend"
```

---

### Task 9: Frontend Component — AgentSelector Dropdown

**Files:**
- Create: `ui/app/components/AgentSelector.tsx`

- [ ] **Step 1: Create the AgentSelector component**

```tsx
import { memo, useState, useRef, useEffect } from 'react'
import { useAppState, useWs } from '../lib/state'

export const AgentSelector = memo(function AgentSelector() {
  const { agents, activeAgentId } = useAppState()
  const ws = useWs()
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [])

  const activeAgent = agents.find(a => a.id === activeAgentId)

  if (agents.length < 2) return null

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1.5 text-xs text-text-dim hover:text-accent border border-border rounded-full px-2.5 py-0.5 hover:border-accent transition-colors"
      >
        {activeAgent?.name ?? 'Agent'}
        <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 bg-surface border border-border rounded-lg py-1 min-w-[120px] z-50">
          {agents.map(agent => (
            <button
              key={agent.id}
              onClick={() => {
                if (agent.id !== activeAgentId) {
                  ws?.send({ type: 'switch_agent', agent_id: agent.id })
                }
                setOpen(false)
              }}
              className={`block w-full text-left px-3 py-1.5 text-xs transition-colors ${
                agent.id === activeAgentId
                  ? 'text-accent'
                  : 'text-text-dim hover:text-text hover:bg-surface-hover'
              }`}
            >
              {agent.name}
            </button>
          ))}
        </div>
      )}
    </div>
  )
})
```

- [ ] **Step 2: Commit**

```bash
git add ui/app/components/AgentSelector.tsx
git commit -m "feat: create AgentSelector dropdown component"
```

---

### Task 10: Frontend Integration — Place Dropdown in Header

**Files:**
- Modify: `ui/app/routes/index.tsx`

- [ ] **Step 1: Add AgentSelector to the header**

Import and place between the help button and ConnectionStatus:

```tsx
import { AgentSelector } from '../components/AgentSelector'
```

Update the right side of the header:

```tsx
        <div className="flex items-center gap-2 sm:gap-3">
          <AgentSelector />
          <button
            onClick={() => dispatch({ type: 'set_show_help', show: true })}
            className="text-text-dim hover:text-accent text-xs border border-border rounded-full px-2 sm:px-2.5 py-0.5 hover:border-accent transition-colors"
          >
            help
          </button>
          <ConnectionStatus />
        </div>
```

- [ ] **Step 2: Commit**

```bash
git add ui/app/routes/index.tsx
git commit -m "feat: add agent selector to header"
```

---

### Task 11: Build and Smoke Test

- [ ] **Step 1: Build frontend**

Run: `cd /home/jb/code/stomp-claw/ui && npm run build 2>&1 | tail -5`
Expected: build succeeds

- [ ] **Step 2: Build Rust binary**

Run: `cd /home/jb/code/stomp-claw && cargo build --release 2>&1 | tail -5`
Expected: compiles without errors

- [ ] **Step 3: Final commit if any fixes needed**

```bash
git add -A
git commit -m "fix: build fixes for agent switching feature"
```
