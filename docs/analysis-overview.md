# Stomp-Claw Analysis Overview

Comprehensive analysis by 4 specialist agents: backend architect, frontend architect, feature strategist, and DevOps engineer.

**Date:** 2026-03-15

## Documents

- **[Backend Improvements](improvements-backend.md)** — Code quality, performance, architecture, security, reliability (8 high-priority findings)
- **[Frontend Improvements](improvements-frontend.md)** — UX, state management, WebSocket handling, performance (8 high-priority findings)
- **[DevOps & Infrastructure](improvements-devops.md)** — Build pipeline, testing, deployment, monitoring, configuration (8 high-priority findings)
- **[New Features](new-features.md)** — 27 feature proposals across 7 categories with complexity estimates

---

## Critical Issues (fix first)

| Issue | File(s) | Why Critical |
|-------|---------|-------------|
| MIDI `expect()` panics — no reconnection | `midi.rs` | Pedal disconnect kills voice input permanently |
| Audio `expect()` panics | `audio.rs` | Missing device crashes entire daemon |
| No `React.memo` — full tree re-renders on every token | All components | Severe UI jank during streaming |
| No systemd unit / auto-restart | `start.sh` | Crash = manual restart required |

## High-Impact Quick Wins (small effort, big value)

| Improvement | Effort | Impact |
|-------------|--------|--------|
| Wrap `Vec<f32>` in `Arc` in events | 10 min | Eliminate 1.6 MB of cloning per recording |
| Add `React.memo` to `MessageBubble` | 10 min | Biggest single frontend perf improvement |
| Fix auto-scroll to respect user position | 30 min | Stop yanking users to bottom during streaming |
| Add delete confirmation dialog | 10 min | Prevent accidental data loss |
| Create `.env.example` | 10 min | Unblock new developer onboarding |
| Validate `OPENCLAW_TOKEN` at startup | 10 min | Fail fast instead of runtime panic |
| Clipboard integration | 1 hr | Zero-friction desktop bridge |
| Per-session system prompts | 1 hr | Multi-project utility leap |

## Feature Highlights

The most exciting feature opportunities that leverage stomp-claw's unique design:

1. **Clipboard Integration** — "read clipboard" / "copy that" bridges hands-free voice with desktop workflow
2. **Screenshot Capture** — "screenshot, what's wrong here?" is the killer debugging feature
3. **Double-Tap Pedal** — doubles the pedal's capability with zero hardware cost
4. **Quick Notes** — not every pedal press should cost an LLM call
5. **Timer/Reminders** — an always-on daemon with audio output is the perfect timer platform
6. **Shell Command Execution** — transforms from Q&A tool to hands-free system controller

## Architecture Themes

Across all analyses, these themes recurred:

1. **Graceful degradation over hard crashes** — MIDI, audio, NeMo, and OpenClaw should all be optional/recoverable
2. **Observable failures** — too many errors are silently swallowed (`let _ =`, `.ok()?`, `if let Ok`)
3. **Performance at scale** — O(n^2) string cloning, full-tree re-renders, and broadcast cloning of large buffers will bite as usage grows
4. **Configuration over hardcoding** — hardware IDs, service URLs, personas, and thresholds are all baked in
5. **Testing foundation** — `commands.rs` pure-logic functions are the ideal beachhead for a test suite
