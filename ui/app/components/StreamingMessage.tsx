import Markdown from 'react-markdown'

export function StreamingMessage({ content }: { content: string }) {
  if (!content) return null

  return (
    <div className="flex justify-start">
      <div className="max-w-[80%] px-4 py-2.5 rounded-lg text-sm leading-relaxed bg-surface border border-accent/30 text-text">
        <div className="prose prose-invert prose-sm max-w-none [&_p]:my-1 [&_pre]:bg-bg [&_pre]:p-3 [&_pre]:rounded [&_code]:text-accent">
          <Markdown>{content}</Markdown>
        </div>
        <span className="streaming-cursor" />
      </div>
    </div>
  )
}
