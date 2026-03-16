# Frontend Improvements

Analysis of the React 19 + Tailwind v4 frontend for UX, performance, state management, and missing features.

## Priority Summary

| # | Issue | Category | Impact |
|---|-------|----------|--------|
| 1 | No `React.memo` anywhere — every streaming token re-renders all messages | Performance | Critical |
| 2 | Auto-scroll yanks user to bottom during streaming | UX | High |
| 3 | Single monolithic state context — all components re-render on every WS message | Performance | High |
| 4 | No confirmation for session deletion | UX | High |
| 5 | Sidebar actions invisible on touch devices | UX | Medium |
| 6 | Full markdown re-parse on every streaming token | Performance | Medium |
| 7 | Unused `@tanstack/react-router` dependency | Bundle Size | Low |
| 8 | WsContext is null on first render | DX | Low |

---

## 1. UX Improvements

### 1a. No focus management after actions

**`TextInput.tsx`, `SessionSidebar.tsx`**
After sending a message, switching sessions, or closing the sidebar, focus is not returned to the textarea. Users must manually click back.

**Fix:** Call `textareaRef.current?.focus()` after `send()` and after sidebar close.

### 1b. No aria-labels on interactive elements

**All components**
Sidebar rename button says "r", delete says "x" — screen readers see unlabeled buttons. HelpModal overlay lacks `role="dialog"` and `aria-modal="true"`. Connection status dot has no label.

**Fix:** Add `aria-label` to all icon/letter buttons. Add proper dialog roles.

### 1c. Auto-scroll snaps to bottom with no escape

**`ChatView.tsx` (line 14)**
`el.scrollTop = el.scrollHeight` fires on every streaming token. If a user scrolls up to read earlier content, they get yanked back on every token.

**Fix:** Track whether the user is "near the bottom" (within ~100px) and only auto-scroll if they are.

### 1d. No confirmation for session deletion

**`SessionSidebar.tsx` (line 86)**
Clicking the "x" button immediately sends `delete_session` — no confirmation. Accidental deletion loses all history.

**Fix:** Add `window.confirm()` or an inline "are you sure?" state.

### 1e. Delete/rename buttons hidden behind hover

**`SessionSidebar.tsx` (line 77)**
`hidden group-hover:flex` means mobile/tablet users can never see these controls.

**Fix:** Always show actions on the active session row. Use long-press or swipe for mobile.

### 1f. Empty state message references pedal

**`ChatView.tsx` (line 21)**
"Hold the pedal to speak, or type below" — confusing for users without a MIDI pedal.

**Fix:** Make the empty state contextual based on whether voice is enabled.

---

## 2. State Management

### 2a. Every WS message re-renders the entire tree

**`lib/state.tsx`**
Single monolithic `AppState` in one context. Every `useAppState()` consumer re-renders on every state change — including every `llm_token` during streaming (dozens per second). Components like `ConnectionStatus`, `StatusBar`, and `ContextBar` re-render on every token despite only needing a single field.

**Fix:** Split into multiple contexts (`StreamingContext`, `SessionContext`, `UIContext`) or use `useSyncExternalStore` with selectors.

### 2b. `turns` Map creates referential identity issues

**`lib/state.tsx` (line 8)**
Every reducer case touching turns creates `new Map(state.turns)`, triggering re-renders even for non-active sessions. `Map` also doesn't serialize well for debugging.

**Fix:** Use a plain `Record<string, Turn[]>` object instead.

### 2c. `session_switched` clears all cached turns

**`lib/state.tsx` (line 72)**
When switching sessions, `turns: new Map()` discards cached turns for all sessions. Switching back requires the server to re-send.

**Fix:** Only clear streaming state on switch. Let the server's `turn_list` message update naturally.

### 2d. WsContext is null on first render

**`lib/state.tsx` (line 174)**
`wsRef.current` is captured at render time, but WebSocketManager is created in `useEffect` (after render). The context value never updates because ref mutation doesn't trigger re-render.

**Fix:** Store WebSocketManager in state via `useState` instead of a ref.

---

## 3. WebSocket Handling

### 3a. Messages sent while disconnected are silently dropped

**`lib/ws.ts` (lines 49-53)**
`send()` checks `readyState === OPEN` and silently does nothing otherwise. User loses their message with no feedback.

**Fix:** Queue messages and flush on reconnect, or show a toast/notice.

### 3b. No heartbeat/ping mechanism

**`lib/ws.ts`**
Relies solely on TCP-level close detection. Half-open connections (laptop sleep/wake) can go undetected for minutes.

**Fix:** Implement periodic ping (every 30s) and mark dead if no pong within timeout.

### 3c. No reconnection status indicator

**`ConnectionStatus.tsx`**
Shows "connected" or "disconnected" but not "reconnecting..." or time until next attempt.

**Fix:** Track reconnect state (attempt count, next retry time) and display it.

---

## 4. Performance

### 4a. No `React.memo` anywhere

**All components**
Since every WS message triggers a top-level state change, every component re-renders. With 50 visible messages, all 50 `MessageBubble`s re-render on every streaming token.

**Fix:** Wrap `MessageBubble`, `ConnectionStatus`, `ContextBar`, `StatusBar`, `SessionSidebar`, and `HelpModal` in `React.memo`. Single highest-impact performance fix.

### 4b. MarkdownRenderer re-created on every streaming token

**`StreamingMessage.tsx`**
Every `llm_token` re-renders `StreamingMessage`, which re-renders `MarkdownRenderer`. Full markdown re-parse including highlight.js and KaTeX on every token.

**Fix:** During streaming, render plain text (or debounce to re-render at most every 200ms). Run full MarkdownRenderer once on `llm_done`.

### 4c. MessageBubble parses JSON on every render

**`MessageBubble.tsx` (lines 10-15)**
`JSON.parse(turn.images)` and `JSON.parse(turn.documents)` called directly in render with no memoization.

**Fix:** Wrap in `React.memo` and memoize JSON parsing with `useMemo`.

### 4d. Unused `@tanstack/react-router` dependency

**`package.json` (line 12)**
Never imported anywhere. Free bundle size reduction.

**Fix:** Remove it.

### 4e. External font blocks rendering

**`styles/app.css` (line 1)**
JetBrains Mono loaded from Google Fonts via `@import url(...)`. Render-blocking external request.

**Fix:** Self-host the font via Vite or use `font-display: swap`.

---

## 5. Missing Features

### 5a. No message search or filtering
Users cannot search through past conversation history. Essential as conversations accumulate.

### 5b. No message copy or export
Individual messages have no "copy" button (only code blocks). No way to export a session.

### 5c. No error retry mechanism
Error-status turns show red text but offer no way to retry the failed request.

### 5d. No keyboard shortcuts
Only Enter-to-send and Escape-to-close-help. No shortcuts for new session, switch session, toggle voice, etc.

### 5e. No theme or appearance settings
Dark theme is hardcoded. No light mode, font size adjustment, or accent color customization.

### 5f. No timestamps on messages
Turns have `created_at` and `completed_at` but they're never displayed.

### 5g. No background tab notification
No notification when a response arrives while the browser tab is backgrounded.

---

## 6. Component Architecture

### 6a. TextInput is too large (~300 lines)
Handles text input, image upload with resize/encoding, document upload, drag-and-drop, paste handling, recording state display, and send logic.

**Fix:** Extract `FilePreviewStrip`, `RecordingIndicator`, and file-processing utilities into separate files.

### 6b. Inline SVG icons duplicated
SVG markup copied inline across `TextInput.tsx`, `MessageBubble.tsx`, `SessionSidebar.tsx`, and `routes/index.tsx`.

**Fix:** Extract an `Icon` component or icon constants.
