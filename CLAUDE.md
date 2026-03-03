# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
cargo build --release       # Build release binary
./start.sh                  # Kill existing, launch daemon in background
./stop.sh                   # Kill running daemon
./tail-log.sh               # tail -f ~/.stomp-claw/stomp-claw.log
```

No test suite exists. Rust 2024 edition.

## What This Is

Stomp Claw is a voice assistant daemon triggered by a MIDI foot pedal (Boss FS-1-WL). Hold the pedal to record speech, release to get an LLM response. The entire app is a single file: `src/main.rs`.

## Architecture & Flow

1. **MIDI listener** (`midi_listener`) — connects to the FS-1-WL pedal, watches for CC 85 press/release events
2. **Audio capture** — cpal stream records 16kHz mono f32 samples into a shared buffer while pedal is held
3. **Partial transcription** — while recording, a thread periodically sends accumulated audio to NeMo for live transcription updates
4. **Processing** (`process`) — on pedal release: writes WAV to tempfile, sends to NeMo (`localhost:5051/transcribe/`) for final transcript, then sends transcript to OpenClaw (`127.0.0.1:18789`, OpenAI-compatible API) for LLM response
5. **Output** — response is written to live display file, logged to conversation history, and optionally spoken via TTS (`~/bin/speak`)

## Key Files & Paths

All state lives in `~/.stomp-claw/`:

- `~/.stomp-claw/stomp-claw.log` — daemon log
- `~/.stomp-claw/live.md` — live status display (current recording/thinking/response)
- `~/.stomp-claw/conversations/` — per-session conversation history files
- `~/.stomp-claw/session.txt` — session ID for OpenClaw continuity
- `~/.stomp-claw/view.txt` — current viewer tab (live/history)
- `~/.stomp-claw/config.toml` — persistent config (currently just `voice_enabled`)
- `beep-down.wav`, `beep-up.wav`, `beep-up2.wav` — audio feedback played via `paplay`

## Threading Model

The MIDI callback spawns threads for both recording (partial transcription loop) and processing (on pedal release). Each processing thread creates its own tokio runtime for async HTTP calls. Shared state uses `Arc<AtomicBool>` for flags and `Arc<Mutex<T>>` for data/config.

## External Services

- **NeMo** at `localhost:5051` — speech-to-text (multipart WAV upload)
- **OpenClaw** at `127.0.0.1:18789` — OpenAI-compatible chat completions API
- **paplay** — PulseAudio sound playback for beeps
- **~/bin/speak** — TTS command

## Voice Toggle

Users can say "voice on/off" to toggle TTS. When voice is enabled, responses are truncated to 2 sentences and max 150 tokens. When disabled, full responses (up to 2000 tokens) are returned text-only.
