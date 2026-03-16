# Backend Improvements

Analysis of the Rust backend for code quality, performance, architecture, security, and reliability.

## Priority Summary

| # | Issue | Category | Impact |
|---|-------|----------|--------|
| 1 | MIDI reconnection — pedal disconnect kills voice input permanently | Reliability | Critical |
| 2 | Audio `expect()` panics — missing device crashes daemon | Reliability | Critical |
| 3 | Broadcast channel clones `Vec<f32>` to every subscriber | Performance | High |
| 4 | `accumulated` string cloned on every LLM token — O(n^2) | Performance | High |
| 5 | No graceful shutdown — unclean termination risks DB corruption | Reliability | High |
| 6 | Silent NeMo failures — voice stops working with no indication | Observability | High |
| 7 | Origin check too broad — allows non-private IPs | Security | Medium |
| 8 | Duplicated session creation logic in 3 places | Maintainability | Medium |

---

## 1. Code Quality Issues

### 1a. Panicking `unwrap()`/`expect()` in runtime paths

**`src/audio.rs` (lines 51-57, 75, 80, 82)**
Uses `.expect()` on device enumeration, config lookup, stream building, and stream start. If the audio device is unplugged or busy, the entire daemon crashes. The `.lock().unwrap()` on the Mutex (line 75, inside the cpal callback) will also panic if the Mutex is poisoned.

**Fix:** Return `Result` from audio setup, log the error, and have `main.rs` retry or run in degraded mode. For the Mutex lock, use `.lock().unwrap_or_else(|e| e.into_inner())` to recover from poisoned state.

### 1b. Panicking `expect()` in MIDI module

**`src/midi.rs` (lines 11, 16, 35)**
`MidiInput::new()`, port lookup, and `.connect()` all use `.expect()`. If the pedal is not connected at startup, the application crashes. The MIDI thread runs on a bare `std::thread::spawn` in `main.rs` (line 103), so the panic is unobserved — the thread silently dies with no recovery.

**Fix:** Loop with retry logic: attempt to find and connect to the pedal every few seconds, logging when it's absent.

### 1c. `dirs::home_dir().unwrap()` in beep::speak

**`src/beep.rs` (line 33)**
Will panic if `$HOME` is unset.

**Fix:** Use `unwrap_or_default()` or handle the `None` case gracefully.

### 1d. Hardcoded audio sink

**`src/config.rs` (line 13)**
`AUDIO_SINK` is hardcoded to `"alsa_output.pci-0000_0d_00.4.analog-stereo"`. Machine-specific — will silently fail on any other system.

**Fix:** Make configurable via env var or DB config, with fallback to the default PulseAudio sink.

---

## 2. Performance Improvements

### 2a. Cloning entire accumulated response on every LLM token

**`src/llm.rs` (line 263)**
Every SSE delta event clones `full_reply` (via `accumulated: full_reply.clone()`) and broadcasts it. For a 2000-token response, this means ~2000 increasingly large string clones — O(n^2) total memory allocation.

**Fix:** Remove `accumulated` from the `LlmToken` event and have clients accumulate locally. Or only include it every N tokens.

### 2b. `RecordingComplete` clones the entire sample buffer through broadcast

**`src/events.rs` (line 34)**
`Event::RecordingComplete { samples: Vec<f32> }` is broadcast-cloned for each receiver. With 5 subscribers and a 5-second recording at 16kHz: 5 x 80,000 x 4 bytes = 1.6 MB of cloning.

**Fix:** Wrap `samples` in `Arc<Vec<f32>>` so cloning only increments a reference count.

### 2c. Blocking I/O in async context

**`src/server.rs` (lines 503, 538)**
`save_base64_image` and `save_document` use `std::fs::write` (blocking) inside an async WebSocket handler. The base64 decoding is also CPU-intensive.

**Fix:** Use `tokio::fs::write` or spawn onto `tokio::task::spawn_blocking`.

### 2d. SSE buffer creates many intermediate strings

**`src/llm.rs` (line 237)**
`buffer = buffer[newline_pos + 1..].to_string()` creates a new String allocation for every line parsed.

**Fix:** Use `buffer.drain(..newline_pos + 1)` or track an offset into the buffer.

---

## 3. Architecture Improvements

### 3a. Duplicated session-creation logic in three places

**`src/main.rs` (43-53), `src/server.rs` (573-594, 610-628), `src/main.rs` handle_voice_commands (126-146)**
Session creation (generate name, UUID, create in DB, set active, send events) is copy-pasted with slight variations.

**Fix:** Extract a `create_and_activate_session(pool, tx, existing_names)` helper.

### 3b. Duplicated WAV-encode-and-transcribe logic

**`src/audio.rs` (partial_transcribe) and `src/transcription.rs` (transcribe)**
Nearly identical: both create a temp WAV file, encode f32 samples, and POST to NeMo.

**Fix:** Extract a shared `wav_transcribe(samples, client, url)` function.

### 3c. Event bus carries too many concerns

**`src/events.rs`**
A single broadcast channel carries everything: large audio buffers, high-frequency LLM tokens, low-frequency session management, and UI commands. Every subscriber receives every event. A slow subscriber can cause lag, dropping events for all.

**Fix:** Consider splitting into separate channels by concern (audio, LLM, commands). At minimum, wrap `Vec<f32>` in `Arc`.

---

## 4. Security Concerns

### 4a. Overly broad origin check for WebSocket

**`src/server.rs` (lines 308-326)**
- `http://172.*` includes the full `172.0.0.0/8` range, but RFC 1918 only covers `172.16.0.0/12`
- `http://100.*` allows the entire range, not just Tailscale's CGNAT `100.64.0.0/10`
- `origin.contains(".ts.net")` could match `evil.ts.net.attacker.com`

**Fix:** Parse the origin as a URL, extract the host, check against proper CIDR ranges. Verify the host *ends with* `.ts.net`.

### 4b. No size limit on base64 images

**`src/server.rs` (save_base64_image)**
Documents have a 5MB limit but images have no size check.

**Fix:** Add a size check matching the document handler.

### 4c. No rate limiting on WebSocket messages

**`src/server.rs`**
Any connected client can send unlimited `SendMessage` events, each triggering an LLM request.

**Fix:** Add basic rate limiting per WebSocket connection.

### 4d. OPENCLAW_TOKEN panics on missing env var

**`src/config.rs` (line 8)**
Called lazily on first LLM request — misconfiguration discovered at runtime, not startup.

**Fix:** Validate at startup in `main.rs` and fail fast with a clear error message.

---

## 5. Reliability

### 5a. No graceful shutdown

**`src/main.rs`**
No signal handler (SIGTERM/SIGINT). SQLite writes may be interrupted. Audio stream never cleanly closed. Spawned tasks not awaited.

**Fix:** Use `tokio::signal::ctrl_c()` to trigger graceful shutdown: close the broadcast channel, await tasks, drop the DB pool cleanly.

### 5b. No reconnection for MIDI device

**`src/midi.rs`**
If MIDI connection drops, the `_conn` handle is dropped and the thread sits in an infinite sleep loop. No detection or recovery.

**Fix:** Periodically check connection liveness, or catch disconnect and retry.

### 5c. No reconnection for NeMo STT service

**`src/audio.rs`, `src/transcription.rs`**
If NeMo is down, functions silently return `None` via `.ok()?`. No logging, no backoff, no health check.

**Fix:** Log HTTP errors before converting to None. Consider a periodic health check that emits a warning event to the UI.

### 5d. Spawned std::thread panics are unobserved

**`src/main.rs` (lines 74, 103)**
Both `std::thread::spawn` calls (audio, MIDI) have no panic handler. If either panics, the thread dies silently.

**Fix:** Use `std::thread::Builder::new().name("audio").spawn()` and check the JoinHandle, or install a panic hook.

---

## 6. Silent Error Swallowing

### 6a. `let _ = tx.send(...)` everywhere

Nearly every `tx.send()` discards the result. Events are lost silently if the channel has no receivers.

**Fix:** Log at `debug` level when a send fails. Treat a closed channel as a shutdown signal.

### 6b. WebSocket parse failures silently ignored

**`src/server.rs` (line 470)**
Malformed messages dropped with no logging.

**Fix:** Log parse errors at `debug`/`warn` level.

### 6c. Partial transcription errors completely invisible

**`src/audio.rs` (line 119)**
Every failure returns `None` with zero logging. HTTP 500 from NeMo? Full temp filesystem? No indication.

**Fix:** Use `inspect_err` to log before converting to Option.
