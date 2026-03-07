import { MarkdownRenderer } from './MarkdownRenderer'
import type { Turn } from '../lib/types'

export function MessageBubble({ turn }: { turn: Turn }) {
  const isUser = turn.role === 'user'

  return (
    <div className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}>
      <div className={`max-w-[80%] px-4 py-2.5 rounded-lg text-sm leading-relaxed ${
        isUser
          ? 'bg-user-bg border border-border text-text'
          : 'bg-surface border border-border text-text'
      } ${turn.status === 'error' ? 'border-error/50' : ''} break-words overflow-hidden`}>
        {isUser ? (
          <p className="whitespace-pre-wrap">{turn.content}</p>
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
