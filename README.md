# StompClaw

A voice assistant triggered by a MIDI foot pedal with a real-time web UI. Hold the pedal to record speech, release to get a streaming LLM response. Also supports typed input from the browser.

## Hardware

- **MIDI Pedal**: Boss FS-1-WL (wireless MIDI foot switch, CC 85)
- **Audio**: Any PulseAudio-compatible microphone

## Architecture

```
┌─────────────────┐
│ MIDI Pedal      │ CC 85 press/release
│ (FS-1-WL)       │
└────────┬────────┘
         │
         ▼
┌─────────────────────────┐
│ Audio Capture (cpal)    │ 16kHz mono f32
│ + partial transcription │ live updates every 300ms
└────────┬────────────────┘
         │ on release
         ▼
┌─────────────────────────┐
│ NeMo STT                │ localhost:5051/transcribe/
└────────┬────────────────┘
         │
    ┌────┴────────────────┐
    │                     │
    ▼                     ▼
 Voice Command       LLM Query
 (session mgmt,      (OpenClaw streaming)
  voice toggle,
  help, etc.)
    │                     │
    └─────────┬───────────┘
              │ events
              ▼
┌──────────────────────────┐     ┌───────────────────┐
│ Axum Server (8765)       │◄───►│ React Web UI      │
│ WebSocket + embedded SPA │     │ (embedded assets)  │
└──────────────────────────┘     └───────────────────┘
              │
              ▼
┌──────────────────────────┐
│ SQLite DB                │ sessions, turns, config
│ (~/.stomp-claw/)         │
└──────────────────────────┘
```

The backend is a Rust daemon with modular event-driven architecture. All modules communicate via a broadcast event bus. The React frontend is compiled and embedded into the Rust binary via `rust_embed`, so the single binary serves everything.

## Build & Run

Requires Rust 2024 edition and Node.js (for the frontend build).

```bash
# Full build (frontend + Rust)
./build-release.sh

# Or separately:
cd ui && npm run build && cd ..
cargo build --release

# Start/stop the daemon
./start.sh          # kills existing, launches in background
./stop.sh           # stops the daemon

# View logs
./tail-log.sh       # tail -f ~/.stomp-claw/stomp-claw.log
```

Access the web UI at **http://127.0.0.1:8765**

## Web UI

The React frontend provides:

- **Session sidebar** — create, rename, delete, and switch between conversations
- **Chat view** — messages with streaming LLM responses and auto-scroll
- **Rich markdown** — syntax-highlighted code blocks with copy button, GFM tables, KaTeX math
- **Status bar** — recording indicator, live partial transcript, thinking state, voice toggle
- **Text input** — type messages directly (Enter to send, Shift+Enter for newlines)
- **Help modal** — voice command reference

Built with React 19, Tailwind CSS v4, and WebSocket for real-time updates.

## Voice Commands

Say these while using the pedal:

| Command | Action |
|---------|--------|
| "new session" / "fresh start" | Create a new conversation |
| "list sessions" | List available sessions |
| "switch to [name]" | Switch session (fuzzy matched) |
| "rename session [name]" | Rename current session |
| "delete session" | Delete current session |
| "voice on" / "voice off" | Toggle TTS |
| "help" / "commands" | Show help modal |
| "never mind" / "scratch that" | Cancel current recording |

## Voice Mode

When **enabled**: responses truncated to 2 sentences (150 tokens) and spoken via TTS.
When **disabled**: full responses up to 2000 tokens, text only.

## External Services

| Service | Address | Purpose |
|---------|---------|---------|
| NeMo | `localhost:5051` | Speech-to-text (multipart WAV upload) |
| OpenClaw | `127.0.0.1:18789` | OpenAI-compatible streaming LLM API |
| paplay | PulseAudio | Audio feedback (beeps) |
| ~/bin/speak | Custom binary | Text-to-speech output |

## Project Structure

```
stomp-claw/
├── src/
│   ├── main.rs            # Entry point, module orchestration
│   ├── midi.rs            # MIDI pedal listener (CC 85)
│   ├── audio.rs           # Audio capture + partial transcription
│   ├── transcription.rs   # Final speech-to-text via NeMo
│   ├── llm.rs             # Streaming LLM requests to OpenClaw
│   ├── commands.rs        # Voice command parsing, session naming
│   ├── server.rs          # Axum web server, WebSocket handler
│   ├── db.rs              # SQLite schema, CRUD, v1 migration
│   ├── events.rs          # Event bus types (25+ event variants)
│   ├── beep.rs            # Audio feedback and TTS
│   └── config.rs          # Constants (URLs, ports, prompts)
├── ui/
│   ├── app/
│   │   ├── components/    # React components (ChatView, TextInput, etc.)
│   │   ├── lib/           # State management, WebSocket client, types
│   │   ├── routes/        # TanStack Router pages
│   │   └── styles/        # Tailwind CSS
│   ├── index.html
│   ├── package.json
│   └── vite.config.ts
├── build-release.sh       # Full build (frontend + Rust)
├── start.sh               # Start daemon
├── stop.sh                # Stop daemon
├── tail-log.sh            # Tail logs
├── beep-*.wav             # Audio feedback samples
└── Cargo.toml
```

## Data Storage

All state lives in `~/.stomp-claw/`:

| File | Purpose |
|------|---------|
| `stomp-claw.db` | SQLite database (sessions, turns, config) |
| `stomp-claw.log` | Daemon log |

The database stores sessions, conversation turns (with streaming status tracking), and key-value config. Automatic migration from v1 JSON files runs on first startup if legacy data is found.
