import { memo } from 'react'
import { MarkdownRenderer } from './MarkdownRenderer'
import type { Turn } from '../lib/types'

function localFileUrl(path: string): string {
  return `/local-file?path=${encodeURIComponent(path)}`
}

export const MessageBubble = memo(function MessageBubble({ turn }: { turn: Turn }) {
  const isUser = turn.role === 'user'
  const images = turn.images
    ? (typeof turn.images === 'string' ? JSON.parse(turn.images) : turn.images) as string[]
    : []
  const documents: Array<{path: string; filename: string}> = turn.documents
    ? (typeof turn.documents === 'string' ? JSON.parse(turn.documents) : turn.documents)
    : []

  return (
    <div className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}>
      <div className={`max-w-[95%] sm:max-w-[80%] px-3 sm:px-4 py-2 sm:py-2.5 rounded-lg text-sm leading-relaxed ${
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
})
