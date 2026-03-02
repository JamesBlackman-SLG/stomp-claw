# Stomp Claw Live Viewer Design

**Date:** 2026-03-02

## Overview

A browser-based viewer for `/tmp/stomp-claw-live.md` with live updates via Server-Sent Events (SSE).

## Architecture

- **Server**: Rust HTTP server on localhost:8080
- **Frontend**: Single HTML page with dark theme
- **Updates**: SSE endpoint streams file changes instantly

## Components

### HTTP Server (`src/viewer.rs`)

- GET `/` — serves the HTML viewer
- GET `/events` — SSE endpoint that watches `/tmp/stomp-claw-live.md` and pushes content on change
- Uses `notify` crate for file watching

### HTML Viewer

- Black background (#000), white text (#fff)
- Markdown rendered to HTML using client-side showdown.js
- Auto-updates via SSE connection
- Monospace font for terminal aesthetic

## Data Flow

1. Browser opens `localhost:8080`
2. Server sends HTML with dark theme
3. Browser connects to `/events` SSE
4. Server watches file; on change, pushes new content to SSE
5. Browser receives and re-renders markdown

## Error Handling

- If markdown file doesn't exist, show "Waiting for recording..."
- If SSE connection drops, auto-reconnect with exponential backoff
