# DevOps & Infrastructure Improvements

Analysis of build pipeline, testing, deployment, monitoring, configuration, and developer experience.

## Priority Summary

| # | Issue | Category | Impact |
|---|-------|----------|--------|
| 1 | Add systemd user unit with `Restart=on-failure` | Deployment | Critical |
| 2 | Write unit tests for `commands.rs` | Testing | High |
| 3 | Make MIDI and audio optional for development | DX | High |
| 4 | Move hardware-specific constants to config | Configuration | High |
| 5 | Add log rotation | Monitoring | Medium |
| 6 | Create `.env.example` | DX | Medium |
| 7 | Add `npm ci` to build script | Build | Medium |
| 8 | Embed or configure WAV file paths | Deployment | Medium |

---

## 1. Build Pipeline

### No version stamping
The code logs "stomp-claw v2" but there is no compile-time version injection from `Cargo.toml` or git SHA.

**Fix:** Use `env!("CARGO_PKG_VERSION")` or a `build.rs` that captures `git rev-parse --short HEAD`.

### `npm install` not in build script
If `node_modules/` is missing or stale, the build uses whatever is there or fails.

**Fix:** Run `npm ci` before `npm run build` in `build-release.sh`.

### Frontend build output not in `.gitignore`
`ui/dist/client/` could get accidentally committed, causing divergence with `rust-embed`.

**Fix:** Add `ui/dist/` to `.gitignore`.

### No CI pipeline
No GitHub Actions workflow at all.

**Fix:** At minimum: `cargo check`, `cargo clippy`, `cd ui && npm ci && npm run build`.

### WAV files as binary blobs
Seven `.wav` files in the repo root inflate clone size.

**Fix:** Consider Git LFS or embedding them via `include_bytes!`.

---

## 2. Testing Strategy

The project has zero tests. Recommended priority order:

### Tier 1: `commands.rs` unit tests (highest ROI)
Pure logic, no I/O. Functions to test:
- `parse_command()` / `parse_command_with_sessions()`
- `fuzzy_match_session()` / `is_cancel_keyword()`
- `truncate_to_sentences()` / `generate_session_name()`

30+ tests writable in an hour. Catches voice command regressions that are hard to debug in production.

### Tier 2: `db.rs` integration tests
Create an in-memory SQLite pool (`sqlite::memory:`) and test full CRUD lifecycle: create session, create turns, migration logic, config get/set. The v1-to-v2 migration path is especially fragile.

### Tier 3: SSE stream parsing in `llm.rs`
The manual SSE parser handles partial chunks, `[DONE]` sentinels, and `response.completed` events. Extract into a testable function.

### Tier 4: End-to-end smoke test
Start the daemon, connect WebSocket, send `create_session`, verify `session_created` response, shut down. Catches "it doesn't start" regressions.

---

## 3. Deployment

### `pkill -f stomp_claw` is too broad
Matches any process whose command line contains "stomp_claw" — including editors or grep.

**Fix:** Write a PID file to `~/.stomp-claw/stomp-claw.pid` on start. Use `kill $(cat pidfile)` on stop.

### No automatic restart on crash
MIDI device disconnect or audio error = process panics and stays down.

**Fix:** Create a systemd user unit:
```ini
[Unit]
Description=StompClaw Voice Assistant

[Service]
Type=exec
ExecStart=/path/to/stomp_claw
WorkingDirectory=/path/to/repo
EnvironmentFile=%h/.stomp-claw/.env
Restart=on-failure
RestartSec=5
StandardOutput=append:%h/.stomp-claw/stomp-claw.log

[Install]
WantedBy=default.target
```

### No health check endpoint
No `/health` or `/readyz` route.

**Fix:** Add an endpoint that checks DB pool liveness and returns version/uptime.

### No graceful shutdown
No signal handlers. SIGTERM causes hard drop.

**Fix:** Add `tokio::signal::ctrl_c()` handler that logs shutdown and flushes the tracing appender.

### Log rotation absent
`tracing_appender::rolling::never()` — single log file grows forever.

**Fix:** Switch to `rolling::daily()` with max file retention.

### Binary must run from repo directory
`beep.rs` uses `std::env::current_dir()` to find WAV files.

**Fix:** Embed WAV files via `include_bytes!`/`rust-embed`, or make the path configurable.

---

## 4. Monitoring & Observability

### No structured logging
Plain text output makes parsing difficult.

**Fix:** Add `.json()` to the tracing subscriber for machine-parseable output.

### No request/response timing
LLM call logs "HTTP 200" but not elapsed time. STT calls have no timing either.

**Fix:** Log elapsed time for both STT and LLM calls.

### Broadcast channel lag warnings are unactionable
Modules log "lagged by N events" with no accumulated visibility.

**Fix:** Add a counter that tracks total lagged events per module.

### No status endpoint
No way to query version, uptime, connection count, or error state.

**Fix:** Add `/api/status` returning version, uptime, active WebSocket count, total LLM requests, last error.

---

## 5. Configuration Management

### Hardcoded values that should be configurable

| Value | Location | Issue |
|-------|----------|-------|
| `NEMO_URL` | config.rs:4 | NeMo might run elsewhere |
| `OPENCLAW_URL` | config.rs:5 | Different port or remote host |
| `AUDIO_SINK` | config.rs:13 | Machine-specific PulseAudio sink |
| `PEDAL_CC = 85` | config.rs:16 | Different controllers use different CCs |
| `SERVER_ADDR` | config.rs:19 | Port conflicts |
| `TLS_ADDR` | config.rs:20 | Same |
| `VOICE_MAX_TOKENS` | config.rs:25 | Tuning parameter |
| `TEXT_MAX_TOKENS` | config.rs:26 | Tuning parameter |
| System prompts | config.rs:23-24 | Hardcoded name "James" and persona "Alan" |
| `~/bin/speak` | beep.rs:33 | Hardcoded path to TTS binary |
| `"FS-1-WL"` | midi.rs:15 | Only works with one specific pedal |

**Fix:** Create a `stomp-claw.toml` or extend `.env` to cover all external endpoints, hardware identifiers, and tuning parameters.

### No `.env.example`
Required `OPENCLAW_TOKEN` env var not documented. New setup requires reading source code.

**Fix:** Create `.env.example` with placeholder values and comments.

---

## 6. Developer Experience

### MIDI device required to start
`midi.rs` line 16 panics if the pedal is not connected. Development on a laptop without the pedal is impossible.

**Fix:** Add `--no-midi` flag or `STOMP_CLAW_NO_MIDI=1` env var that skips MIDI initialization.

### Audio device required to start
`audio.rs` line 51 panics on "No input device".

**Fix:** Same pattern — skip audio initialization when hardware is absent.

### No code formatting enforcement
No `rustfmt.toml`, no pre-commit hooks, no clippy in CI.

**Fix:** Add `.cargo/config.toml` alias for lint. Document in CLAUDE.md.

### Frontend dev server not documented
`npm run dev` works via Vite proxy but this isn't mentioned anywhere.

**Fix:** Add to CLAUDE.md or a dev guide.

### V1 migration code lives forever
`db.rs` (lines 284-363) will run on every startup checking for v1 schema.

**Fix:** Gate behind a feature flag or remove after all instances have migrated.

### No database backup mechanism
SQLite file at `~/.stomp-claw/stomp-claw.db` is the single copy of all history.

**Fix:** Add a `sqlite3 .dump` script or `/api/export` endpoint.
