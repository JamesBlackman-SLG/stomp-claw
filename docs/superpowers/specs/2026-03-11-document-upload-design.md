# Document Upload Support

## Summary

Add file upload support for documents (PDF, CSV, TXT, JSON, HTML, Markdown) by leveraging OpenClaw's native `input_file` content type. No local text extraction needed — OpenClaw handles PDF parsing, OCR fallback, and all supported formats natively.

## Supported Formats

| Format | MIME Type | Max Size |
|--------|-----------|----------|
| PDF | application/pdf | 5MB |
| CSV | text/csv | 5MB |
| TXT | text/plain | 5MB |
| JSON | application/json | 5MB |
| HTML | text/html | 5MB |
| Markdown | text/markdown | 5MB |

## Architecture

### Data Flow

```
User attaches document in UI
  → base64 data URL sent over WebSocket
  → server saves to ~/.stomp-claw/documents/{uuid}.{ext}
  → stored in DB (turns.documents column, JSON array)
  → sent to OpenClaw as input_file part (base64 + mime type + filename)
  → OpenClaw extracts text / rasterizes PDF pages as needed
  → LLM responds with document context
```

### Session Context

The server already sends an `x-openclaw-session-key` header, which may mean OpenClaw persists file context server-side across turns. Implementation should first test whether OpenClaw retains document context within a session key. If it does, no re-sending is needed. If not, fall back to re-sending all prior session documents as `input_file` parts (capped at ~20MB total).

## Changes by Layer

### 1. UI (TextInput.tsx)

- Expand file picker accept list: `.pdf, .csv, .txt, .json, .html, .md`
- Distinguish between image and document attachments by MIME type
- Show documents as filename chips (not image previews) with remove button
- Send documents in a new `documents` field: `Array<{data: string, filename: string}>` where `data` is the base64 data URL
- Enforce 5MB per-file limit client-side with user feedback on rejection

### 2. Types (types.ts)

- Add `documents?: Array<{data: string, filename: string}>` to `WsCommand` send_message
- Add `documents?: Array<{path: string, filename: string}> | null` to `Turn` interface
- Note: documents use `{path, filename}` objects (not flat strings like `images: string[]`) because we need to display the original filename

### 3. Server (server.rs)

- New `save_document()` function: decode base64, determine extension from MIME type, write to `~/.stomp-claw/documents/{uuid}.{ext}`, return path + original filename. Create `~/.stomp-claw/documents/` directory on demand via `create_dir_all`.
- Update `WsIncoming::SendMessage` to include optional `documents` field
- Update `handle_ws_message()` to process document attachments alongside images
- Expand `/local-file` endpoint allowlist to include document MIME types, **with path prefix validation**: only serve files whose paths start with `~/.stomp-claw/` to prevent path traversal

### 4. Events (events.rs)

- Add `documents: Vec<(String, String)>` field (path, filename) to `Event::UserTextMessage`

### 5. LLM (llm.rs)

- Update `send_to_llm` signature to accept `documents: &[(String, String)]` parameter
- Build `input_file` parts for documents:
  ```json
  {
    "type": "file",
    "source": {
      "type": "base64",
      "media_type": "application/pdf",
      "data": "<base64>"
    },
    "filename": "report.pdf"
  }
  ```
  Note: The exact field names (`"type": "file"` vs `"input_file"`) need verification against the OpenClaw endpoint at implementation time. Start with `"file"` per the Responses API spec; fall back to `"input_file"` if rejected.
- The `FinalTranscript` handler (voice input) passes empty documents `&[]` — voice input does not support document attachment.

### 6. Database (db.rs)

- Add `documents` TEXT column to `turns` table via ALTER TABLE (same error-swallow migration pattern as existing columns)
- JSON array of objects: `[{"path": "/home/jb/.stomp-claw/documents/uuid.pdf", "filename": "report.pdf"}]`
- Extend `create_turn_with_images` → `create_turn_with_attachments(session_id, role, content, status, images: Option<&str>, documents: Option<&str>)`
- New query: `get_session_documents(session_id)` — returns all document paths/filenames from a session's user turns
- Frontend parsing: `documents` column arrives as a JSON string from DB, parsed at the component level (same pattern as existing `images` handling in MessageBubble.tsx)

### 7. UI Display (MessageBubble.tsx)

- Parse `turn.documents` (JSON string → array of `{path, filename}`)
- Render document attachments as clickable filename links with a file icon
- Link href points to `/local-file?path=<encoded_path>` for download
- Visual distinction from images (no inline preview, just icon + filename text)

## Not In Scope

- DOCX / XLSX support (future: add local extraction via Rust crates)
- Document previews / inline rendering in UI
- Cross-session document access
- Search across documents
- RAG / chunking (OpenClaw handles extraction, full content sent inline)
- Voice input document attachment
- Orphaned file cleanup on session delete (existing tech debt for images too)

## System Dependencies

None added. OpenClaw handles all document processing. Tesseract/OCR not needed.

## Risk Notes

- Large PDFs may be slow if OpenClaw needs to rasterize pages for OCR
- 20MB session context cap may need tuning based on OpenClaw's actual limits
- `input_file` / `file` API format needs verification against actual OpenClaw endpoint at implementation time — the type name and envelope structure should be confirmed with a test request
- Session key persistence of file context is unverified — implementation should test this early and adapt accordingly
