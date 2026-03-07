import { useEffect, useRef } from 'react'
import { useAppState } from '../lib/state'
import { MessageBubble } from './MessageBubble'
import { StreamingMessage } from './StreamingMessage'

export function ChatView() {
  const { activeSessionId, turns, streamingTurnId, streamingContent, thinking } = useAppState()
  const bottomRef = useRef<HTMLDivElement>(null)

  const sessionTurns = turns.get(activeSessionId) || []

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'instant' })
  }, [sessionTurns.length, streamingContent, thinking])

  return (
    <div className="flex-1 overflow-y-auto px-4 py-4 space-y-3">
      {sessionTurns.length === 0 && !thinking && !streamingTurnId && (
        <div className="flex items-center justify-center h-full text-text-dim text-sm">
          Hold the pedal to speak, or type below.
        </div>
      )}
      {sessionTurns.map(turn => (
        <MessageBubble key={turn.id} turn={turn} />
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
      <div ref={bottomRef} />
    </div>
  )
}
