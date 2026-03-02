# StompClaw

A voice assistant daemon triggered by a MIDI foot pedal. Hold the pedal to record speech, release to get an LLM response.

## Hardware

- **MIDI Pedal**: Boss FS-1-WL (wireless MIDI foot switch)
- **Audio**: Any PulseAudio-compatible microphone

## Architecture

```
MIDI Foot Pedal (CC 85)
        │
        ▼
┌───────────────────┐
│  MIDI Listener    │ ──── Wait for pedal press/release
└───────────────────┘
        │
        ▼ (while held)
┌───────────────────┐
│  Audio Capture    │ ──── 16kHz mono, cpal stream
│  (partial trans)  │ ──── Thread sends to NeMo every 2s
└───────────────────┘
        │
        ▼ (on release)
┌───────────────────┐
│  NeMo STT         │ ──── localhost:5051/transcribe/
└───────────────────┘
        │
        ▼
┌───────────────────┐
│  OpenClaw API     │ ──── LLM response
└───────────────────┘
        │
        ▼
┌───────────────────┐
│  Output           │ ──── Live file + TTS (optional)
└───────────────────┘
```

## Build & Run

```bash
# Build
cargo build --release

# Start the daemon
./start.sh

# Stop the daemon
./stop.sh

# View logs
./tail-log.sh
```

## Viewer

The viewer is a separate binary that serves a web page displaying the live conversation:

```bash
# Start viewer (in separate terminal)
cargo run --release --bin stomp-claw-viewer

# Or use the script
./start-viewer.sh
```

Access at http://localhost:8765

## Files & Paths

| File | Purpose |
|------|---------|
| `/tmp/stomp-claw.log` | Daemon log |
| `/tmp/stomp-claw-live.md` | Live status for viewer |
| `/tmp/stomp-claw-conversation.md` | Conversation history |
| `/tmp/stomp-claw-session.txt` | OpenClaw session ID |
| `~/.config/stomp-claw/config.toml` | Persistent config |

## Configuration

Edit `~/.config/stomp-claw/config.toml`:

```toml
voice_enabled = true  # Set to false to disable TTS
```

Voice can also be toggled by saying "voice on" or "voice off" during a conversation.

## External Services

- **NeMo** (localhost:5051) — Speech-to-text
- **OpenClaw** (localhost:18789) — LLM API
- **paplay** — PulseAudio sound playback for beeps
- **~/bin/speak** — TTS command

## Project Structure

```
stomp-claw/
├── src/
│   ├── main.rs       # Main daemon (MIDI, audio, processing)
│   └── viewer.rs    # Web viewer server
├── start.sh          # Start daemon
├── stop.sh           # Stop daemon
├── start-viewer.sh   # Start viewer
├── tail-log.sh       # Tail logs
└── Cargo.toml
```

## Dependencies

- Rust 2024 edition
- midir (MIDI input)
- cpal (audio capture)
- hound (WAV writing)
- reqwest (HTTP client)
- rouille (HTTP server for viewer)
- notify (file watching)

## Voice Mode

When voice is **enabled**:
- Responses truncated to 2 sentences, max 150 tokens
- Spoken via TTS

When voice is **disabled**:
- Full responses up to 2000 tokens
- Text only
