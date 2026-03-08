# CLAUDE.md

## Build & Run

```bash
./build-release.sh              # Build frontend + Rust binary
cargo build --release           # Rust only (if UI unchanged)
cd ui && npm run build && cd .. # Frontend only
./start.sh                      # Kill existing, launch daemon
./stop.sh                       # Stop daemon
```

No test suite. Rust 2024 edition. Frontend assets are embedded into the binary via `rust_embed`, so `cargo build` is needed after UI changes.

## Overview

Voice assistant daemon with web UI. MIDI foot pedal (Boss FS-1-WL, CC 85) triggers recording; speech goes to NeMo STT then OpenClaw LLM. Also accepts typed input via the browser at `127.0.0.1:8765`.

## Structure

- `src/` — Rust backend: `main.rs` (entry), `midi.rs`, `audio.rs`, `transcription.rs`, `llm.rs`, `commands.rs`, `server.rs` (Axum + WebSocket), `db.rs` (SQLite), `events.rs` (broadcast bus), `beep.rs`, `config.rs`
- `ui/app/` — React 19 + Tailwind v4 + Vite frontend: components in `components/`, state/ws in `lib/`
- State in `~/.stomp-claw/stomp-claw.db` (SQLite) and `stomp-claw.log`
