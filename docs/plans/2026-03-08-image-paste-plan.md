# Image Paste Support — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow users to paste, drag-drop, or pick images in the chat input, preview them, send alongside text, store on disk, display as thumbnails in conversation, and forward to OpenClaw as multimodal messages.

**Architecture:** Images flow as base64 over WebSocket from frontend to backend. Backend saves to `~/.stomp-claw/images/{uuid}.{ext}`, stores paths as JSON in the `turns.images` column. LLM requests use multimodal content arrays when images are present. Frontend serves stored images via the existing `/local-file` endpoint.

**Tech Stack:** React 19, Axum WebSocket, SQLite (sqlx), reqwest, serde_json, base64 crate, uuid crate (already in use)

---

### Task 1: DB migration — add `images` column to `turns`

**Files:**
- Modify: `src/db.rs`

**Step 1: Add images field to Turn struct**

In `src/db.rs`, update the `Turn` struct (line 14-23) to add:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub images: Option<String>, // JSON array of image paths, e.g. '["/path/to/img.png"]'
}
```

**Step 2: Add ALTER TABLE migration**

In `run_migrations()` (after the existing CREATE TABLE statements, before the PRAGMA lines), add:
```rust
    // Add images column to turns (idempotent — ignore error if column already exists)
    let _ = sqlx::query("ALTER TABLE turns ADD COLUMN images TEXT")
        .execute(pool).await;
```

**Step 3: Update all Turn queries to include images column**

Update `get_turns` (line 146-158), `get_turns_after` (line 161-174) — add `images` to SELECT and struct:
```rust
    // In SELECT: add ", images" after completed_at
    // In struct mapping: add
    images: r.get("images"),
```

**Step 4: Add `create_turn_with_images` function**

```rust
pub async fn create_turn_with_images(pool: &SqlitePool, session_id: &str, role: &str, content: &str, status: &str, images: Option<&str>) -> Result<i64, sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query("INSERT INTO turns (session_id, role, content, status, created_at, images) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(status)
        .bind(&now)
        .bind(images)
        .execute(pool).await?;
    Ok(result.last_insert_rowid())
}
```

**Step 5: Build and verify**

Run: `cargo build --release 2>&1 | grep error`
Expected: No errors (warnings OK)

**Step 6: Commit**

```bash
git add src/db.rs
git commit -m "feat: add images column to turns table"
```

---

### Task 2: Update events and WebSocket protocol

**Files:**
- Modify: `src/events.rs`
- Modify: `src/server.rs`

**Step 1: Update UserTextMessage event to carry images**

In `src/events.rs` (line 64), change:
```rust
    UserTextMessage { session_id: String, text: String, images: Vec<String> },
```

**Step 2: Update WsIncoming to accept optional images**

In `src/server.rs`, update `WsIncoming::SendMessage` (line 57):
```rust
    SendMessage { session_id: String, text: String, #[serde(default)] images: Vec<String> },
```

**Step 3: Update handle_ws_message to save images and forward**

In `src/server.rs`, update the `WsIncoming::SendMessage` handler (around line 300-301). Replace:
```rust
        WsIncoming::SendMessage { session_id, text } => {
            let _ = tx.send(Event::UserTextMessage { session_id, text });
        }
```
With:
```rust
        WsIncoming::SendMessage { session_id, text, images } => {
            // Save base64 images to disk, collect paths
            let mut image_paths: Vec<String> = Vec::new();
            if !images.is_empty() {
                let images_dir = app_config::base_dir().join("images");
                let _ = std::fs::create_dir_all(&images_dir);
                for data_url in &images {
                    if let Some(saved) = save_base64_image(data_url, &images_dir) {
                        image_paths.push(saved);
                    }
                }
            }
            let _ = tx.send(Event::UserTextMessage { session_id, text, images: image_paths });
        }
```

**Step 4: Add save_base64_image helper in server.rs**

Add this function (above `handle_ws_message`):
```rust
fn save_base64_image(data_url: &str, dir: &std::path::Path) -> Option<String> {
    // Parse "data:image/png;base64,iVBOR..." format
    let parts: Vec<&str> = data_url.splitn(2, ',').collect();
    if parts.len() != 2 { return None; }

    let header = parts[0]; // "data:image/png;base64"
    let b64_data = parts[1];

    // Extract extension from mime type
    let ext = if header.contains("image/png") { "png" }
        else if header.contains("image/jpeg") { "jpg" }
        else if header.contains("image/gif") { "gif" }
        else if header.contains("image/webp") { "webp" }
        else { "png" }; // fallback

    let bytes = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64_data) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to decode base64 image: {}", e);
            return None;
        }
    };

    let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    let path = dir.join(&filename);
    match std::fs::write(&path, &bytes) {
        Ok(_) => Some(path.to_string_lossy().to_string()),
        Err(e) => {
            tracing::error!("Failed to write image: {}", e);
            None
        }
    }
}
```

**Step 5: Add `base64` to Cargo.toml**

Check if base64 is already a dependency. If not, add:
```toml
base64 = "0.22"
```

**Step 6: Build and verify**

Run: `cargo build --release 2>&1 | grep error`
Expected: No errors

**Step 7: Commit**

```bash
git add src/events.rs src/server.rs Cargo.toml Cargo.lock
git commit -m "feat: WebSocket image upload with disk storage"
```

---

### Task 3: Update LLM to send multimodal content

**Files:**
- Modify: `src/llm.rs`

**Step 1: Update send_to_llm signature**

Change `send_to_llm` (line 25-31) to accept images:
```rust
async fn send_to_llm(
    tx: &EventSender,
    pool: &SqlitePool,
    client: &Client,
    session_id: &str,
    user_message: &str,
    voice_enabled: bool,
    images: &[String],
) {
```

**Step 2: Store images JSON on user turn**

Replace the user turn creation (line 34) with:
```rust
    let images_json = if images.is_empty() { None } else {
        Some(serde_json::to_string(images).unwrap_or_default())
    };
    let _user_turn_id = match db::create_turn_with_images(pool, session_id, "user", user_message, "complete", images_json.as_deref()).await {
```

**Step 3: Build multimodal content array**

Replace the payload construction (lines 62-70) with:
```rust
    let user_content = if images.is_empty() {
        serde_json::json!(user_message)
    } else {
        let mut parts = vec![serde_json::json!({"type": "text", "text": user_message})];
        for img_path in images {
            if let Ok(bytes) = tokio::fs::read(img_path).await {
                let ext = std::path::Path::new(img_path)
                    .extension().and_then(|e| e.to_str()).unwrap_or("png");
                let mime = match ext {
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "webp" => "image/webp",
                    _ => "image/png",
                };
                let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                parts.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": {"url": format!("data:{};base64,{}", mime, b64)}
                }));
            }
        }
        serde_json::json!(parts)
    };

    let payload = serde_json::json!({
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_content}
        ],
        "stream": true,
        "max_tokens": max_tokens,
        "user": "stomp-claw"
    });
```

**Step 4: Update callers of send_to_llm**

In the `run()` function, update both call sites (FinalTranscript around line 235, UserTextMessage around line 252):

For `FinalTranscript` (voice — no images):
```rust
    send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &[]).await;
```

For `UserTextMessage`:
```rust
    Ok(Event::UserTextMessage { session_id, text, images }) => {
        // ...existing busy check...
        send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &images).await;
```

**Step 5: Build and verify**

Run: `cargo build --release 2>&1 | grep error`
Expected: No errors

**Step 6: Commit**

```bash
git add src/llm.rs
git commit -m "feat: multimodal LLM requests with image support"
```

---

### Task 4: Update frontend types and WebSocket command

**Files:**
- Modify: `ui/app/lib/types.ts`

**Step 1: Update Turn interface**

```typescript
export interface Turn {
  id: number
  session_id: string
  role: 'user' | 'assistant'
  content: string
  status: 'pending' | 'streaming' | 'complete' | 'error'
  created_at: string
  completed_at: string | null
  images: string[] | null
}
```

**Step 2: Update WsCommand send_message**

```typescript
  | { type: 'send_message'; session_id: string; text: string; images?: string[] }
```

**Step 3: Commit**

```bash
git add ui/app/lib/types.ts
git commit -m "feat: add images to Turn and WsCommand types"
```

---

### Task 5: TextInput — paste, drag-drop, file picker, preview

**Files:**
- Modify: `ui/app/components/TextInput.tsx`

**Step 1: Rewrite TextInput with image support**

```tsx
import { useState, useCallback, useRef, useEffect } from 'react'
import { useAppState, useWs } from '../lib/state'

interface PendingImage {
  file: File
  preview: string
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = reject
    reader.readAsDataURL(file)
  })
}

function isImageFile(file: File): boolean {
  return file.type.startsWith('image/')
}

export function TextInput() {
  const [text, setText] = useState('')
  const [images, setImages] = useState<PendingImage[]>([])
  const [dragOver, setDragOver] = useState(false)
  const { activeSessionId, thinking, streamingTurnId } = useAppState()
  const ws = useWs()
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const busy = thinking || streamingTurnId !== null
  const hasContent = text.trim().length > 0 || images.length > 0

  const addImages = useCallback((files: File[]) => {
    const imageFiles = files.filter(isImageFile)
    const newImages = imageFiles.map(file => ({
      file,
      preview: URL.createObjectURL(file),
    }))
    setImages(prev => [...prev, ...newImages])
  }, [])

  const removeImage = useCallback((index: number) => {
    setImages(prev => {
      const removed = prev[index]
      if (removed) URL.revokeObjectURL(removed.preview)
      return prev.filter((_, i) => i !== index)
    })
  }, [])

  const send = useCallback(async () => {
    if (!hasContent || !activeSessionId || busy) return

    let imageData: string[] | undefined
    if (images.length > 0) {
      imageData = await Promise.all(images.map(img => fileToBase64(img.file)))
    }

    ws?.send({
      type: 'send_message',
      session_id: activeSessionId,
      text: text.trim(),
      ...(imageData && { images: imageData }),
    })

    // Cleanup
    images.forEach(img => URL.revokeObjectURL(img.preview))
    setImages([])
    setText('')
  }, [text, images, activeSessionId, busy, ws, hasContent])

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    const files = Array.from(e.clipboardData.files)
    if (files.some(isImageFile)) {
      e.preventDefault()
      addImages(files)
    }
  }, [addImages])

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(false)
    const files = Array.from(e.dataTransfer.files)
    addImages(files)
  }, [addImages])

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(true)
  }, [])

  const handleDragLeave = useCallback(() => setDragOver(false), [])

  // Auto-resize textarea to fit content
  useEffect(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = Math.min(el.scrollHeight, 200) + 'px'
  }, [text])

  return (
    <div
      className={`border-t border-border px-4 py-3 ${dragOver ? 'bg-accent/10' : ''}`}
      onDrop={handleDrop}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
    >
      {images.length > 0 && (
        <div className="flex gap-2 mb-2 flex-wrap">
          {images.map((img, i) => (
            <div key={i} className="relative group">
              <img
                src={img.preview}
                alt=""
                className="w-16 h-16 object-cover rounded border border-border"
              />
              <button
                onClick={() => removeImage(i)}
                className="absolute -top-1.5 -right-1.5 w-5 h-5 bg-error text-white rounded-full text-xs flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
              >
                x
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="flex gap-2">
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          multiple
          className="hidden"
          onChange={e => {
            if (e.target.files) addImages(Array.from(e.target.files))
            e.target.value = ''
          }}
        />
        <button
          onClick={() => fileInputRef.current?.click()}
          disabled={busy}
          className="px-2 py-2 text-text-dim hover:text-text transition-colors disabled:opacity-30"
          title="Attach image"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect width="18" height="18" x="3" y="3" rx="2" ry="2"/>
            <circle cx="9" cy="9" r="2"/>
            <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>
          </svg>
        </button>
        <textarea
          ref={textareaRef}
          className="flex-1 bg-surface border border-border rounded px-3 py-2 text-sm text-text outline-none focus:border-accent placeholder:text-text-dim resize-none"
          placeholder={busy ? 'Waiting for response...' : 'Type a message...'}
          value={text}
          rows={1}
          onChange={e => setText(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
          onPaste={handlePaste}
          disabled={busy}
        />
        <button
          onClick={send}
          disabled={busy || !hasContent}
          className="px-4 py-2 bg-accent/20 text-accent border border-accent/30 rounded text-sm font-medium hover:bg-accent/30 disabled:opacity-30 disabled:cursor-not-allowed transition-colors self-end"
        >
          Send
        </button>
      </div>
    </div>
  )
}
```

**Step 2: Build frontend**

Run: `cd ui && npx vite build 2>&1 | tail -3`
Expected: "built in" success message

**Step 3: Commit**

```bash
git add ui/app/components/TextInput.tsx
git commit -m "feat: image paste, drag-drop, and file picker in TextInput"
```

---

### Task 6: MessageBubble — render image thumbnails

**Files:**
- Modify: `ui/app/components/MessageBubble.tsx`

**Step 1: Update MessageBubble to render images**

```tsx
import { MarkdownRenderer } from './MarkdownRenderer'
import type { Turn } from '../lib/types'

function localFileUrl(path: string): string {
  return `/local-file?path=${encodeURIComponent(path)}`
}

export function MessageBubble({ turn }: { turn: Turn }) {
  const isUser = turn.role === 'user'
  const images = turn.images ? (typeof turn.images === 'string' ? JSON.parse(turn.images) : turn.images) as string[] : []

  return (
    <div className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}>
      <div className={`max-w-[80%] px-4 py-2.5 rounded-lg text-sm leading-relaxed ${
        isUser
          ? 'bg-user-bg border border-border text-text'
          : 'bg-surface border border-border text-text'
      } ${turn.status === 'error' ? 'border-error/50' : ''} break-words overflow-hidden`}>
        {images.length > 0 && (
          <div className="flex gap-2 flex-wrap mb-2">
            {images.map((img, i) => (
              <a key={i} href={localFileUrl(img)} target="_blank" rel="noopener noreferrer">
                <img
                  src={localFileUrl(img)}
                  alt=""
                  className="max-w-[200px] max-h-[200px] object-cover rounded border border-border hover:opacity-90 transition-opacity cursor-pointer"
                  loading="lazy"
                />
              </a>
            ))}
          </div>
        )}
        {isUser ? (
          turn.content && <p className="whitespace-pre-wrap">{turn.content}</p>
        ) : (
          <MarkdownRenderer content={turn.content} />
        )}
        {turn.status === 'error' && (
          <div className="mt-1 text-xs text-error">Error</div>
        )}
      </div>
    </div>
  )
}
```

**Step 2: Build frontend**

Run: `cd ui && npx vite build 2>&1 | tail -3`
Expected: Success

**Step 3: Commit**

```bash
git add ui/app/components/MessageBubble.tsx
git commit -m "feat: render image thumbnails in message bubbles"
```

---

### Task 7: Full build and integration test

**Files:** None (verification only)

**Step 1: Build everything**

Run: `./build-release.sh`
Expected: Frontend and Rust binary both build successfully

**Step 2: Manual integration test**

1. `./start.sh`
2. Open `http://127.0.0.1:8765`
3. Test: paste an image from clipboard (Ctrl+V) — should show thumbnail preview
4. Test: drag an image file onto the input area — should show thumbnail preview
5. Test: click the image button, pick a file — should show thumbnail preview
6. Test: click X on a thumbnail — should remove it
7. Test: type text + image, hit send — should show image in conversation and get LLM response
8. Test: send image-only (no text) — should work
9. Test: reload page — images should persist in conversation history

**Step 3: Final commit**

```bash
git add -A
git commit -m "feat: image paste, drag-drop, and multimodal LLM support"
```
