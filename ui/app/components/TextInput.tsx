import { useState, useCallback } from 'react'
import { useAppState, useWs } from '../lib/state'

export function TextInput() {
  const [text, setText] = useState('')
  const { activeSessionId, thinking, streamingTurnId } = useAppState()
  const ws = useWs()

  const busy = thinking || streamingTurnId !== null

  const send = useCallback(() => {
    const trimmed = text.trim()
    if (!trimmed || !activeSessionId || busy) return
    ws?.send({ type: 'send_message', session_id: activeSessionId, text: trimmed })
    setText('')
  }, [text, activeSessionId, busy, ws])

  return (
    <div className="border-t border-border px-4 py-3 flex gap-2">
      <input
        className="flex-1 bg-surface border border-border rounded px-3 py-2 text-sm text-text outline-none focus:border-accent placeholder:text-text-dim"
        placeholder={busy ? 'Waiting for response...' : 'Type a message...'}
        value={text}
        onChange={e => setText(e.target.value)}
        onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send() } }}
        disabled={busy}
      />
      <button
        onClick={send}
        disabled={busy || !text.trim()}
        className="px-4 py-2 bg-accent/20 text-accent border border-accent/30 rounded text-sm font-medium hover:bg-accent/30 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
      >
        Send
      </button>
    </div>
  )
}
