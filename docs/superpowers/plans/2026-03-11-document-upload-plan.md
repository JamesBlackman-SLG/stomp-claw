# Document Upload Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add file upload support for PDF, CSV, TXT, JSON, HTML, and Markdown documents, leveraging OpenClaw's native `input_file` content type.

**Architecture:** Documents flow through the same pipeline as images: UI encodes to base64 → WebSocket → server saves to disk → DB stores metadata → LLM sends as `input_file` parts to OpenClaw. No local text extraction needed.

**Tech Stack:** Rust/Axum backend, React 19 + TypeScript frontend, SQLite, OpenClaw Responses API

**Spec:** `docs/superpowers/specs/2026-03-11-document-upload-design.md`

---

## Chunk 1: Backend (Rust)

### Task 1: Database — Add `documents` column

**Files:**
- Modify: `src/db.rs:14-24` (Turn struct)
- Modify: `src/db.rs:44-87` (migrations)
- Modify: `src/db.rs:150-206` (turn queries)

- [ ] **Step 1: Add `documents` field to Turn struct**

In `src/db.rs`, add `documents` to the `Turn` struct (line 23, after `images`):

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
    pub images: Option<String>,
    pub documents: Option<String>,
}
```

- [ ] **Step 2: Add migration for `documents` column**

In `run_migrations()`, after the existing `ALTER TABLE turns ADD COLUMN images TEXT` (line 71-72), add:

```rust
    let _ = sqlx::query("ALTER TABLE turns ADD COLUMN documents TEXT")
        .execute(pool).await;
```

- [ ] **Step 3: Update all turn queries to include `documents`**

Every query that reads turns needs `documents` in the SELECT and the struct mapping. Update these functions:

`get_turns` (line 151): Add `documents` to SELECT and mapping:
```rust
let rows = sqlx::query("SELECT id, session_id, role, content, status, created_at, completed_at, images, documents FROM turns WHERE session_id = ? ORDER BY id")
```
Add to the map closure: `documents: r.get("documents"),`

`get_turns_after` (line 167): Same change — add `documents` to SELECT and mapping.

- [ ] **Step 4: Add `create_turn_with_attachments` function**

After `create_turn_with_images` (line 206), add:

```rust
pub async fn create_turn_with_attachments(
    pool: &SqlitePool,
    session_id: &str,
    role: &str,
    content: &str,
    status: &str,
    images: Option<&str>,
    documents: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "INSERT INTO turns (session_id, role, content, status, created_at, images, documents) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(status)
        .bind(&now)
        .bind(images)
        .bind(documents)
        .execute(pool).await?;
    Ok(result.last_insert_rowid())
}
```

- [ ] **Step 5: Add `get_session_documents` function**

After `create_turn_with_attachments`, add:

```rust
pub async fn get_session_documents(pool: &SqlitePool, session_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query("SELECT documents FROM turns WHERE session_id = ? AND role = 'user' AND documents IS NOT NULL ORDER BY id")
        .bind(session_id)
        .fetch_all(pool).await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("documents")).collect())
}
```

- [ ] **Step 6: Compile check**

Run: `cargo check 2>&1 | head -30`
Expected: Errors in `server.rs` and `llm.rs` about missing `documents` field (these files still use the old Turn struct). That's fine — we fix those in the next tasks.

- [ ] **Step 7: Commit**

```bash
git add src/db.rs
git commit -m "feat: add documents column to turns table"
```

---

### Task 2: Events — Add documents to UserTextMessage

**Files:**
- Modify: `src/events.rs:64` (UserTextMessage variant)

- [ ] **Step 1: Add `documents` field to `Event::UserTextMessage`**

In `src/events.rs` line 64, change:

```rust
    UserTextMessage { session_id: String, text: String, images: Vec<String> },
```

to:

```rust
    UserTextMessage { session_id: String, text: String, images: Vec<String>, documents: Vec<(String, String)> },
```

The `documents` field is `Vec<(String, String)>` where each tuple is `(path, filename)`.

- [ ] **Step 2: Commit**

```bash
git add src/events.rs
git commit -m "feat: add documents field to UserTextMessage event"
```

---

### Task 3: Server — Document saving and WebSocket handling

**Files:**
- Modify: `src/server.rs:56-57` (WsIncoming)
- Modify: `src/server.rs:72-100` (local_file_handler)
- Modify: `src/server.rs:440-471` (add save_document after save_base64_image)
- Modify: `src/server.rs:473-487` (handle_ws_message SendMessage arm)

- [ ] **Step 1: Add documents to `WsIncoming::SendMessage`**

In `src/server.rs` line 57, change:

```rust
    SendMessage { session_id: String, text: String, #[serde(default)] images: Vec<String> },
```

to:

```rust
    SendMessage { session_id: String, text: String, #[serde(default)] images: Vec<String>, #[serde(default)] documents: Vec<WsDocument> },
```

Add the `WsDocument` struct above the `#[derive(Deserialize)]` for `WsIncoming` (before line 53):

```rust
#[derive(Deserialize)]
struct WsDocument {
    data: String,
    filename: String,
}
```

- [ ] **Step 2: Add `save_document` function**

After `save_base64_image` (after line 471), add:

```rust
fn save_document(data_url: &str, original_filename: &str, dir: &std::path::Path) -> Option<(String, String)> {
    let parts: Vec<&str> = data_url.splitn(2, ',').collect();
    if parts.len() != 2 { return None; }

    let header = parts[0];
    let b64_data = parts[1];

    let ext = if header.contains("application/pdf") { "pdf" }
        else if header.contains("text/csv") { "csv" }
        else if header.contains("application/json") { "json" }
        else if header.contains("text/html") { "html" }
        else if header.contains("text/markdown") { "md" }
        else if header.contains("text/plain") { "txt" }
        else {
            // Fall back to extension from original filename
            std::path::Path::new(original_filename)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("txt")
        };

    use base64::Engine;
    let bytes = match base64::engine::general_purpose::STANDARD.decode(b64_data) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to decode base64 document: {}", e);
            return None;
        }
    };

    // Enforce 5MB limit
    if bytes.len() > 5 * 1024 * 1024 {
        tracing::warn!("Document too large: {} bytes (max 5MB)", bytes.len());
        return None;
    }

    let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    let path = dir.join(&filename);
    match std::fs::write(&path, &bytes) {
        Ok(_) => Some((path.to_string_lossy().to_string(), original_filename.to_string())),
        Err(e) => {
            tracing::error!("Failed to write document: {}", e);
            None
        }
    }
}
```

- [ ] **Step 3: Update `handle_ws_message` to process documents**

In `handle_ws_message`, update the `SendMessage` arm (around line 475). Change:

```rust
        WsIncoming::SendMessage { session_id, text, images } => {
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

to:

```rust
        WsIncoming::SendMessage { session_id, text, images, documents } => {
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
            let mut doc_entries: Vec<(String, String)> = Vec::new();
            if !documents.is_empty() {
                let docs_dir = app_config::base_dir().join("documents");
                let _ = std::fs::create_dir_all(&docs_dir);
                for doc in &documents {
                    if let Some(entry) = save_document(&doc.data, &doc.filename, &docs_dir) {
                        doc_entries.push(entry);
                    }
                }
            }
            let _ = tx.send(Event::UserTextMessage { session_id, text, images: image_paths, documents: doc_entries });
        }
```

- [ ] **Step 4: Expand `/local-file` endpoint to serve documents with path validation**

Replace the `local_file_handler` function (lines 72-100) with:

```rust
async fn local_file_handler(
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let path = match query.get("path") {
        Some(p) => std::path::PathBuf::from(p),
        None => return axum::http::StatusCode::BAD_REQUEST.into_response(),
    };

    // Canonicalize to prevent path traversal
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };

    let base_dir = app_config::base_dir().canonicalize().unwrap_or_default();
    if !canonical.starts_with(&base_dir) {
        return (axum::http::StatusCode::FORBIDDEN, "Access denied").into_response();
    }

    let ext = canonical.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let allowed = matches!(ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" |
        "pdf" | "csv" | "txt" | "json" | "html" | "md"
    );
    if !allowed {
        return (axum::http::StatusCode::FORBIDDEN, "File type not allowed").into_response();
    }

    match tokio::fs::read(&canonical).await {
        Ok(data) => {
            let mime = mime_guess::from_path(&canonical).first_or_octet_stream();
            let content_type = (axum::http::header::CONTENT_TYPE, mime.as_ref().to_string());
            // Add Content-Disposition for document types
            if matches!(ext.as_str(), "pdf" | "csv" | "txt" | "json" | "html" | "md") {
                let filename = canonical.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("download");
                (
                    [
                        content_type,
                        (axum::http::header::CONTENT_DISPOSITION, format!("inline; filename=\"{}\"", filename)),
                    ],
                    data,
                ).into_response()
            } else {
                ([content_type, (axum::http::header::CONTENT_DISPOSITION, String::new())], data).into_response()
            }
        }
        Err(_) => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}
```

- [ ] **Step 5: Compile check**

Run: `cargo check 2>&1 | head -30`
Expected: Errors in `llm.rs` about missing `documents` field in pattern match. That's expected — fixed in next task.

- [ ] **Step 6: Commit**

```bash
git add src/server.rs
git commit -m "feat: add document upload handling to server"
```

---

### Task 4: LLM — Send documents as `input_file` parts

**Files:**
- Modify: `src/llm.rs:18-26` (send_to_llm signature)
- Modify: `src/llm.rs:27-94` (user turn creation + input_file building)
- Modify: `src/llm.rs:265-298` (event handlers)

- [ ] **Step 1: Update `send_to_llm` signature**

Change line 18-26 from:

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

to:

```rust
async fn send_to_llm(
    tx: &EventSender,
    pool: &SqlitePool,
    client: &Client,
    session_id: &str,
    user_message: &str,
    voice_enabled: bool,
    images: &[String],
    documents: &[(String, String)],
) {
```

- [ ] **Step 2: Update user turn creation to store documents**

Replace lines 28-37 (the images_json + create_turn_with_images block) with:

```rust
    // Create user turn in DB
    let images_json = if images.is_empty() { None } else {
        Some(serde_json::to_string(images).unwrap_or_default())
    };
    let documents_json = if documents.is_empty() { None } else {
        let doc_objects: Vec<serde_json::Value> = documents.iter().map(|(path, filename)| {
            serde_json::json!({"path": path, "filename": filename})
        }).collect();
        Some(serde_json::to_string(&doc_objects).unwrap_or_default())
    };
    let _user_turn_id = match db::create_turn_with_attachments(pool, session_id, "user", user_message, "complete", images_json.as_deref(), documents_json.as_deref()).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Failed to create user turn: {}", e);
            return;
        }
    };
```

- [ ] **Step 3: Add `input_file` parts for documents**

After the image parts block (after line 94, the closing `}` of `if !images.is_empty()`), add:

```rust
    // Add document parts
    if !documents.is_empty() {
        use base64::Engine;
        for (doc_path, filename) in documents {
            if let Ok(bytes) = tokio::fs::read(doc_path).await {
                let ext = std::path::Path::new(doc_path)
                    .extension().and_then(|e| e.to_str()).unwrap_or("txt");
                let media_type = match ext {
                    "pdf" => "application/pdf",
                    "csv" => "text/csv",
                    "json" => "application/json",
                    "html" => "text/html",
                    "md" => "text/markdown",
                    _ => "text/plain",
                };
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                user_parts.push(serde_json::json!({
                    "type": "file",
                    "source": {
                        "type": "base64",
                        "media_type": media_type,
                        "data": b64
                    },
                    "filename": filename
                }));
            }
        }
    }

    // Also include documents from prior turns in this session
    if let Ok(prior_doc_jsons) = db::get_session_documents(pool, session_id).await {
        use base64::Engine;
        let mut total_size: usize = 0;
        for doc_json in &prior_doc_jsons {
            if let Ok(docs) = serde_json::from_str::<Vec<serde_json::Value>>(doc_json) {
                for doc in &docs {
                    let path = doc["path"].as_str().unwrap_or("");
                    let filename = doc["filename"].as_str().unwrap_or("document");
                    // Skip documents from the current message (already added above)
                    if documents.iter().any(|(p, _)| p == path) { continue; }
                    if let Ok(bytes) = tokio::fs::read(path).await {
                        total_size += bytes.len();
                        if total_size > 20 * 1024 * 1024 {
                            tracing::warn!("Session document context exceeds 20MB, skipping remaining");
                            break;
                        }
                        let ext = std::path::Path::new(path)
                            .extension().and_then(|e| e.to_str()).unwrap_or("txt");
                        let media_type = match ext {
                            "pdf" => "application/pdf",
                            "csv" => "text/csv",
                            "json" => "application/json",
                            "html" => "text/html",
                            "md" => "text/markdown",
                            _ => "text/plain",
                        };
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        user_parts.push(serde_json::json!({
                            "type": "file",
                            "source": {
                                "type": "base64",
                                "media_type": media_type,
                                "data": b64
                            },
                            "filename": filename
                        }));
                    }
                }
            }
            if total_size > 20 * 1024 * 1024 { break; }
        }
    }
```

- [ ] **Step 4: Update event handlers to pass documents**

In the `run()` function, update the `FinalTranscript` handler (around line 279):

Change:
```rust
                send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &[]).await;
```
to:
```rust
                send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &[], &[]).await;
```

Update the `UserTextMessage` handler (around line 283-296):

Change:
```rust
            Ok(Event::UserTextMessage { session_id, text, images }) => {
```
to:
```rust
            Ok(Event::UserTextMessage { session_id, text, images, documents }) => {
```

And change:
```rust
                send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &images).await;
```
to:
```rust
                send_to_llm(&tx, &pool, &client, &session_id, &text, voice_enabled, &images, &documents).await;
```

- [ ] **Step 5: Build the full project**

Run: `cargo build 2>&1 | tail -20`
Expected: Successful compilation.

- [ ] **Step 6: Commit**

```bash
git add src/llm.rs
git commit -m "feat: send documents to OpenClaw as input_file parts"
```

---

## Chunk 2: Frontend (TypeScript/React)

### Task 5: Types — Add documents to WsCommand and Turn

**Files:**
- Modify: `ui/app/lib/types.ts`

- [ ] **Step 1: Add `documents` to Turn interface**

In `ui/app/lib/types.ts`, add after line 16 (`images: string[] | null`):

```typescript
  documents: string | null  // JSON string of [{path, filename}] from DB
```

- [ ] **Step 2: Add `documents` to WsCommand send_message**

In `ui/app/lib/types.ts` line 41, change:

```typescript
  | { type: 'send_message'; session_id: string; text: string; images?: string[] }
```

to:

```typescript
  | { type: 'send_message'; session_id: string; text: string; images?: string[]; documents?: Array<{data: string; filename: string}> }
```

- [ ] **Step 3: Commit**

```bash
git add ui/app/lib/types.ts
git commit -m "feat: add documents to Turn and WsCommand types"
```

---

### Task 6: TextInput — Accept and send document files

**Files:**
- Modify: `ui/app/components/TextInput.tsx`

- [ ] **Step 1: Add document state and types**

After the `PendingImage` interface (line 4-7), add:

```typescript
interface PendingDocument {
  file: File
  filename: string
}

const DOCUMENT_TYPES = [
  'application/pdf',
  'text/csv',
  'text/plain',
  'application/json',
  'text/html',
  'text/markdown',
]

const DOCUMENT_EXTENSIONS = ['.pdf', '.csv', '.txt', '.json', '.html', '.md']

const MAX_DOC_SIZE = 5 * 1024 * 1024 // 5MB

function isDocumentFile(file: File): boolean {
  if (DOCUMENT_TYPES.includes(file.type)) return true
  return DOCUMENT_EXTENSIONS.some(ext => file.name.toLowerCase().endsWith(ext))
}

function fileToBase64DataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = reject
    reader.readAsDataURL(file)
  })
}
```

- [ ] **Step 2: Add document state to component**

In the `TextInput` component, after `const [images, setImages] = useState<PendingImage[]>([])` (line 40), add:

```typescript
  const [documents, setDocuments] = useState<PendingDocument[]>([])
```

Update `hasContent` (line 48) to:

```typescript
  const hasContent = text.trim().length > 0 || images.length > 0 || documents.length > 0
```

- [ ] **Step 3: Add document handling functions**

After `removeImage` (line 68), add:

```typescript
  const addDocuments = useCallback((files: File[]) => {
    const docFiles = files.filter(f => isDocumentFile(f) && f.size <= MAX_DOC_SIZE)
    const rejected = files.filter(f => isDocumentFile(f) && f.size > MAX_DOC_SIZE)
    if (rejected.length > 0) {
      console.warn(`${rejected.length} file(s) exceeded 5MB limit`)
    }
    setDocuments(prev => [...prev, ...docFiles.map(f => ({ file: f, filename: f.name }))])
  }, [])

  const removeDocument = useCallback((index: number) => {
    setDocuments(prev => prev.filter((_, i) => i !== index))
  }, [])
```

- [ ] **Step 4: Update `addImages` to also handle documents**

The existing `addImages` function (line 53-60) only takes image files. Update the `handleDrop` and `handlePaste` to also route document files. Change `addImages`:

```typescript
  const addFiles = useCallback((files: File[]) => {
    const imageFiles = files.filter(isImageFile)
    const docFiles = files.filter(f => isDocumentFile(f) && !isImageFile(f))
    if (imageFiles.length > 0) {
      const newImages = imageFiles.map(file => ({
        file,
        preview: URL.createObjectURL(file),
      }))
      setImages(prev => [...prev, ...newImages])
    }
    if (docFiles.length > 0) {
      addDocuments(docFiles)
    }
  }, [addDocuments])
```

Replace all references to `addImages` in the component with `addFiles`:
- `handlePaste` (line 91-96): change the guard to `if (files.some(f => isImageFile(f) || isDocumentFile(f)))` and change `addImages(files)` to `addFiles(files)`
- `handleDrop` (line 98-103): change `addImages(files)` to `addFiles(files)`
- File input `onChange` (line 167-169): change `addImages(Array.from(e.target.files))` to `addFiles(Array.from(e.target.files))`

- [ ] **Step 5: Update send function to include documents**

In the `send` function (line 70-88), after the imageData block and before `ws?.send(`, add document encoding:

```typescript
    let documentData: Array<{data: string; filename: string}> | undefined
    if (documents.length > 0) {
      documentData = await Promise.all(
        documents.map(async doc => ({
          data: await fileToBase64DataUrl(doc.file),
          filename: doc.filename,
        }))
      )
    }
```

Update the `ws?.send()` call to include documents:

```typescript
    ws?.send({
      type: 'send_message',
      session_id: activeSessionId,
      text: text.trim(),
      ...(imageData && { images: imageData }),
      ...(documentData && { documents: documentData }),
    })
```

After `setImages([])`, add:

```typescript
    setDocuments([])
```

Update the `send` dependency array (line 88) from `[text, images, activeSessionId, busy, ws, hasContent]` to `[text, images, documents, activeSessionId, busy, ws, hasContent]`.

- [ ] **Step 6: Update file input to accept documents**

Change the file input accept attribute (line 164):

From: `accept="image/*"`
To: `accept="image/*,.pdf,.csv,.txt,.json,.html,.md"`

Change the button title (line 177):

From: `title="Attach image"`
To: `title="Attach file"`

Update the SVG icon to a generic paperclip/attach icon:

```tsx
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/>
          </svg>
```

- [ ] **Step 7: Add document chips display in the JSX**

After the image previews section (after line 144, the closing `</div>` and `)}` of the images block), add:

```tsx
      {documents.length > 0 && (
        <div className="flex gap-2 mb-2 flex-wrap">
          {documents.map((doc, i) => (
            <div key={i} className="flex items-center gap-1.5 bg-surface border border-border rounded px-2 py-1 text-xs text-text group">
              <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/>
                <path d="M14 2v4a2 2 0 0 0 2 2h4"/>
              </svg>
              <span className="max-w-[150px] truncate">{doc.filename}</span>
              <button
                onClick={() => removeDocument(i)}
                className="w-4 h-4 text-text-dim hover:text-error transition-colors opacity-0 group-hover:opacity-100"
              >
                &times;
              </button>
            </div>
          ))}
        </div>
      )}
```

- [ ] **Step 8: Commit**

```bash
git add ui/app/components/TextInput.tsx
git commit -m "feat: accept document uploads in TextInput"
```

---

### Task 7: MessageBubble — Display document attachments

**Files:**
- Modify: `ui/app/components/MessageBubble.tsx`

- [ ] **Step 1: Parse and display documents**

In `MessageBubble.tsx`, after the `images` parsing (line 10-12), add:

```typescript
  const documents: Array<{path: string; filename: string}> = turn.documents
    ? (typeof turn.documents === 'string' ? JSON.parse(turn.documents) : turn.documents)
    : []
```

After the images display section (after line 33, the closing `</div>` and `)}` of the images block), add:

```tsx
        {documents.length > 0 && (
          <div className="flex gap-2 flex-wrap mb-2">
            {documents.map((doc, i) => (
              <a
                key={i}
                href={localFileUrl(doc.path)}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1.5 bg-surface border border-border rounded px-2 py-1 text-xs text-accent hover:text-accent/80 hover:border-accent/30 transition-colors"
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/>
                  <path d="M14 2v4a2 2 0 0 0 2 2h4"/>
                </svg>
                <span className="max-w-[200px] truncate">{doc.filename}</span>
              </a>
            ))}
          </div>
        )}
```

- [ ] **Step 2: Commit**

```bash
git add ui/app/components/MessageBubble.tsx
git commit -m "feat: display document attachments in message bubbles"
```

---

### Task 8: Build, test, and verify

- [ ] **Step 1: Build frontend**

Run: `cd ui && npm run build && cd ..`
Expected: Successful build with no TypeScript errors.

- [ ] **Step 2: Build full project**

Run: `cargo build --release 2>&1 | tail -10`
Expected: Successful compilation.

- [ ] **Step 3: Manual smoke test**

1. Run `./start.sh`
2. Open `http://127.0.0.1:8765` in browser
3. Verify the attach button now shows a paperclip icon
4. Click attach → file picker should show PDF, CSV, TXT, etc.
5. Attach a PDF → should show as a filename chip
6. Send with a message like "summarize this document"
7. Verify the PDF appears as a clickable link in the message bubble
8. Verify the LLM responds with document context
9. Send a follow-up message referencing the document to verify session context works
10. Check logs: `tail -50 ~/.stomp-claw/stomp-claw.log` for any errors

- [ ] **Step 4: Verify `input_file` format**

If OpenClaw rejects `"type": "file"`, try changing to `"type": "input_file"` in `src/llm.rs`. Check the error response body in logs for format hints.

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "feat: document upload support (PDF, CSV, TXT, JSON, HTML, MD)"
```
