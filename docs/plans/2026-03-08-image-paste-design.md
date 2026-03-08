# Image Paste Support

## User Flow

1. User pastes (Ctrl+V), drags, or picks an image file via button
2. Thumbnail preview appears above textarea with X to remove
3. User optionally types text alongside
4. On send: image uploaded via WebSocket as base64, backend saves to `~/.stomp-claw/images/{uuid}.{ext}`, path stored in DB

## Data Changes

### DB: `turns` table
- Add `images TEXT` column (nullable JSON array of absolute paths)
- e.g. `["/home/jb/.stomp-claw/images/abc123.png"]`

### WebSocket Protocol
- `send_message` command gains optional `images: string[]` field (base64 data URLs)
- No new server->client message types needed; `Turn` objects gain `images` field

### Storage
- Images saved to `~/.stomp-claw/images/` as `{uuid}.{ext}`
- Served via existing `/local-file` endpoint

## LLM Payload

When images present, use multimodal content array:
```json
{"role": "user", "content": [
  {"type": "text", "text": "What's this?"},
  {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}
]}
```

When no images, keep existing string format unchanged.

## Frontend Changes

### TextInput.tsx
- Add paste handler (onPaste) to detect image clipboard data
- Add drop handler (onDrop/onDragOver) for drag-and-drop
- Add file picker button (hidden input + click handler)
- State: `images: {file: File, preview: string}[]`
- Preview area above textarea showing thumbnails with X remove buttons
- On send: convert files to base64, include in WebSocket message, clear state
- Disable send if no text AND no images (allow image-only sends)

### MessageBubble.tsx
- If turn has images, render clickable thumbnails above text content
- Rewrite image paths through `/local-file` endpoint

### types.ts
- Turn: add `images: string[] | null`
- WsCommand send_message: add `images?: string[]`

## Backend Changes

### server.rs
- Update `WsIncoming::SendMessage` to accept optional `images` field
- On receive: save each base64 image to `~/.stomp-claw/images/{uuid}.{ext}`, collect paths
- Pass image paths through to LLM module via event

### events.rs
- Update `UserTextMessage` event to include `images: Vec<String>`

### llm.rs
- Read image files from disk, base64 encode, build multimodal content array
- Fall back to simple string content when no images

### db.rs
- Add `images` column to turns table (ALTER TABLE migration)
- Update `create_turn` to accept optional images JSON
- Update `Turn` struct to include images field
