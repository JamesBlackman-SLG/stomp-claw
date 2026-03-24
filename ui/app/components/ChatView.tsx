import { useCallback, useEffect, useRef } from 'react'
import { useAppState, useWs } from '../lib/state'
import { MessageBubble } from './MessageBubble'
import { StreamingMessage } from './StreamingMessage'

export function ChatView() {
  const { activeSessionId, turns, streamingTurnId, streamingContent, thinking } = useAppState()
  const ws = useWs()
  const containerRef = useRef<HTMLDivElement>(null)

  const sessionTurns = turns.get(activeSessionId) || []

  useEffect(() => {
    const el = containerRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [sessionTurns.length, streamingContent, thinking])

  const handleDelete = useCallback((turnId: number) => {
    ws?.send({ type: 'delete_message', session_id: activeSessionId, turn_id: turnId })
  }, [ws, activeSessionId])

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto px-2 sm:px-4 py-3 sm:py-4 space-y-2 sm:space-y-3">
      {sessionTurns.length === 0 && !thinking && !streamingTurnId && (
        <div className="flex items-center justify-center h-full text-text-dim text-sm">
          Hold the pedal to speak, or type below.
        </div>
      )}
      {sessionTurns.map(turn => (
        <MessageBubble key={turn.id} turn={turn} onDelete={handleDelete} />
      ))}
      {thinking && !streamingContent && (
        <div className="flex justify-start">
          <div className="px-4 py-2.5 rounded-lg text-sm bg-surface border border-border text-accent">
            thinking...
          </div>
        </div>
      )}
      {streamingTurnId && streamingContent && (
        <StreamingMessage content={streamingContent} />
      )}
      <div />
    </div>
  )
}
