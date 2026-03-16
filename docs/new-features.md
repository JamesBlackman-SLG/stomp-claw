# New Feature Ideas

Feature proposals that leverage stomp-claw's unique strengths: physical foot pedal, always-on local daemon, and local-first architecture.

## Top 10 Priorities

| # | Feature | Complexity | Why |
|---|---------|-----------|-----|
| 1 | Clipboard Integration | Small | Instant hands-free bridge to desktop workflow |
| 2 | Screenshot Capture | Medium | Killer hands-free debugging feature |
| 3 | Full-Text Search | Medium | Essential as conversation volume grows |
| 4 | System Prompt Per Session | Small | Dramatically improves multi-project utility |
| 5 | Double-Tap Pedal | Small | Doubles pedal capability with zero hardware cost |
| 6 | Timer/Reminders | Medium | Always-on daemon is perfectly suited |
| 7 | Quick Notes | Small | Not every pedal press should cost an LLM call |
| 8 | Configurable MIDI Mapping | Small | Removes the single-device limitation |
| 9 | Prompt Templates | Medium | Makes voice interaction far more powerful |
| 10 | Shell Command Execution | Medium | Transforms it into a true system controller |

---

## Workflow Features

### System Prompt Per Session
Let each session have its own custom system prompt (e.g., "You are a coding assistant" vs "You are a recipe helper"). Currently the system prompt is globally hardcoded in `config.rs` as either VOICE or TEXT.

**Complexity:** Small
**Implementation:** Add a `system_prompt` column to the `sessions` table (nullable, falls back to global default). Expose in the UI sidebar. Pass to `send_to_llm` instead of the hardcoded prompts. Voice command: "set prompt to coding assistant."

### Prompt Templates / Quick Actions
Predefined prompt templates triggered by short voice phrases (e.g., "summarize this," "translate to Spanish," "explain like I'm five"). The foot pedal is ideal for rapid short commands, but composing long prompts by voice is tedious.

**Complexity:** Medium
**Implementation:** Store templates in a `templates` table. Parse in `commands.rs` alongside existing voice commands. Templates could include `{input}` placeholders filled by the next utterance. UI needs a template manager.

### Chained Commands / Macros
Record a sequence of steps (e.g., "summarize, then translate to French, then email the result") as a single voice-triggered macro.

**Complexity:** Large
**Implementation:** Macro definition format (JSON/DSL), executor that chains LLM calls passing output N as input N+1, error handling for failed steps. Voice: "run macro daily standup."

### Clipboard Integration
Voice commands "read clipboard" / "use clipboard" to inject clipboard contents as context, and "copy response" to put the last answer on the clipboard.

**Complexity:** Small
**Implementation:** Use `xclip`/`wl-copy` on Linux (already calling `paplay` via `Command`). New voice commands in `commands.rs`. "Copy that" copies last assistant response.

### Timer / Reminder System
Voice-activated timers and reminders ("remind me in 10 minutes to check the oven," "set a 25-minute pomodoro").

**Complexity:** Medium
**Implementation:** Spawn a tokio timer task. Store reminders in SQLite with target timestamps. On trigger, play a distinctive sound and speak the reminder. Repeating timers add complexity.

### Quick Notes / Voice Memos
A "take a note" voice command that saves the utterance as a tagged, searchable note rather than sending it to the LLM.

**Complexity:** Small
**Implementation:** New voice command prefix. New `notes` table. Display in a separate UI tab or inline. Auto-tag with timestamp and session context.

---

## Integration Ideas

### Desktop Notifications
Send system notifications via `notify-send` when the LLM finishes responding, especially when the browser tab is not focused.

**Complexity:** Small
**Implementation:** Call `notify-send` from `beep.rs` on `LlmDone`. Include first ~100 chars of response. Make configurable.

### Shell Command Execution
Let the LLM execute shell commands on the local machine with user confirmation ("run `ls -la ~/projects`" -> shows output in chat).

**Complexity:** Medium
**Implementation:** Security: allowlist or confirmation step. Sandbox via timeout and output limits. Voice "confirm" or UI button. Display stdout/stderr as a new turn.

### File System Browser / Context Injection
Voice command "read file ~/notes/todo.txt" to feed file contents as context to the LLM.

**Complexity:** Medium
**Implementation:** New voice command in `commands.rs`. Read file, truncate if large, inject as user message. The `local-file` endpoint already exists — extend its allowed paths.

### Home Automation / MQTT Bridge
Bridge voice commands to an MQTT broker or Home Assistant API to control smart home devices via foot pedal.

**Complexity:** Medium-Large
**Implementation:** Add MQTT client (`rumqttc` crate) or HTTP calls to HA REST API. Could be LLM tool-use with entity discovery. Voice: "turn off the lights."

### Calendar / Schedule Integration
Query and add events to a calendar via voice ("what's on my calendar today?").

**Complexity:** Large
**Implementation:** OAuth2 for Google Calendar or CalDAV for self-hosted. Natural language date parsing via LLM. Credential management needed.

---

## Voice UX

### Configurable Pedal Modes (Toggle vs Hold)
Support both hold-to-record (current) and tap-to-toggle (press once to start, again to stop).

**Complexity:** Small
**Implementation:** Add `pedal_mode` config. Modify `midi.rs` toggle logic. Voice command: "toggle mode" / "hold mode."

### Double-Tap for Commands
Quick double-tap of the pedal triggers a specific action (repeat last response, new session, enter command mode).

**Complexity:** Small-Medium
**Implementation:** Detect in `midi.rs` by timing gap between PedalUp and next PedalDown (<300ms). Emit `PedalDoubleTap` event. Map to configurable action.

### Continuous Listening Mode
Always-listening mode with wake word ("Hey Alan") that starts recording without the pedal.

**Complexity:** Large
**Implementation:** Continuous audio to ring buffer. Local wake word detection (Porcupine or small Whisper model). VAD for end-of-utterance. High CPU/memory without dedicated engine.

### Voice Speed / TTS Voice Selection
Configure TTS voice, speed, and pitch via voice command or UI.

**Complexity:** Small (if speak script supports flags)
**Implementation:** Pass parameters to `~/bin/speak`. Store in config table. Voice: "speak faster."

### Audio Recording Playback
Save recorded audio clips and allow replaying them ("play back what I said").

**Complexity:** Small
**Implementation:** Write `Vec<f32>` samples to WAV in `~/.stomp-claw/recordings/`. Add "play recording" command or UI button. Use `paplay` for playback.

---

## Knowledge Management

### Full-Text Search Across Sessions
Search all conversation history from UI or voice ("search for that recipe I asked about").

**Complexity:** Medium
**Implementation:** FTS5 virtual table on `turns`. New `/api/search?q=...` endpoint. UI search bar. Voice: "search for [query]." Return session name, snippet, timestamp.

### Bookmarks / Starred Messages
Star individual messages for quick retrieval.

**Complexity:** Small
**Implementation:** Add `bookmarked` boolean to `turns`. Star icon on each bubble. "Bookmarks" tab in sidebar. Voice: "bookmark that."

### Session Tags / Categories
Tag sessions with categories (#coding, #music, #recipes).

**Complexity:** Small
**Implementation:** New `session_tags` table. Tag chips in sidebar. Filter dropdown. Voice: "tag session coding."

### Export Conversations
Export a session as Markdown, JSON, or plain text.

**Complexity:** Small
**Implementation:** New `/api/sessions/{id}/export?format=md`. UI: export button on session context menu.

### Auto-Summarize Sessions
Automatically generate a short summary after the first few turns and use as session subtitle.

**Complexity:** Small-Medium
**Implementation:** After N turns, send summarization prompt to LLM. Update session name or `summary` column. Run as background task.

---

## Multi-Modal

### Screenshot Capture
Voice command "screenshot" captures the screen and sends it to the LLM for analysis.

**Complexity:** Medium
**Implementation:** Use `grim` (Wayland) or `scrot`/`maim` (X11). Save to `~/.stomp-claw/screenshots/`. Inject as image attachment. Voice: "take screenshot" or "capture screen."

### Code Execution Sandbox
Execute code blocks from LLM responses in a sandboxed environment and show output inline.

**Complexity:** Large
**Implementation:** Docker/podman for sandboxing. Parse code blocks from last response. Timeout and resource limits. Voice: "run that." Never auto-execute.

### Webcam Capture
Capture a photo from the webcam for visual Q&A ("what is this component?").

**Complexity:** Medium
**Implementation:** `ffmpeg` or `v4l2` to capture from `/dev/video0`. Feed into existing image pipeline. Voice: "take photo."

---

## Collaboration

### Shared Sessions via Link
Generate a shareable read-only link to a session.

**Complexity:** Medium
**Implementation:** Random share token per session. New `/shared/{token}` endpoint with read-only view. Optional expiry.

### Multi-User Turn-Taking
Multiple users join the same session, each identified by name.

**Complexity:** Large
**Implementation:** Add user identity to turns. WebSocket connections need user ID. Extend `role` to "user:james", "user:alice". Multiple MIDI devices.

---

## Customization

### Configurable MIDI Mapping
Map any MIDI CC, note, or device to actions (not just CC 85 from FS-1-WL).

**Complexity:** Small
**Implementation:** Move CC number, device name, channel to config table. Add "MIDI learn" mode. Voice: "learn pedal."

### Theme / UI Customization
Light/dark themes and accent color selection.

**Complexity:** Small
**Implementation:** Tailwind CSS variables already in use. Theme switcher swaps variable values. Store in config. Sync via WebSocket.

### Plugin / Extension System
Plugin API for custom voice commands, integrations, and event handlers.

**Complexity:** Large
**Implementation:** The broadcast event bus is a natural extension point. Plugins as separate processes connecting via WebSocket or Unix socket. WASM for in-process. Need manifest format and lifecycle management.

### Per-Session LLM Model Selection
Choose different models per session (fast for quick questions, powerful for complex analysis).

**Complexity:** Small
**Implementation:** Add `model` column to sessions. Pass model name in OpenClaw payload. UI: model selector dropdown. Voice: "use Claude for this session."
