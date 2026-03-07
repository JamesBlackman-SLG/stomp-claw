import { MarkdownRenderer } from './MarkdownRenderer'

export function StreamingMessage({ content }: { content: string }) {
  if (!content) return null

  return (
    <div className="flex justify-start">
      <div className="max-w-[80%] px-4 py-2.5 rounded-lg text-sm leading-relaxed bg-surface border border-accent/30 text-text break-words overflow-hidden">
        <MarkdownRenderer content={content} />
        <span className="streaming-cursor" />
      </div>
    </div>
  )
}
