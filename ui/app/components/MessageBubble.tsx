import { MarkdownRenderer } from './MarkdownRenderer'
import type { Turn } from '../lib/types'

function localFileUrl(path: string): string {
  return `/local-file?path=${encodeURIComponent(path)}`
}

export function MessageBubble({ turn }: { turn: Turn }) {
  const isUser = turn.role === 'user'
  const images = turn.images
    ? (typeof turn.images === 'string' ? JSON.parse(turn.images) : turn.images) as string[]
    : []

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
        {turn.status === 'error' ? (
          <div className="text-xs text-error">{turn.content || 'Error'}</div>
        ) : isUser ? (
          turn.content && <p className="whitespace-pre-wrap">{turn.content}</p>
        ) : (
          <MarkdownRenderer content={turn.content} />
        )}
      </div>
    </div>
  )
}
