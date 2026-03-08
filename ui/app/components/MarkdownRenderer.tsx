import { useState, useCallback } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import rehypeHighlight from 'rehype-highlight'
import rehypeKatex from 'rehype-katex'
import type { Components } from 'react-markdown'

const remarkPlugins = [remarkGfm, remarkMath]
const rehypePlugins = [rehypeHighlight, rehypeKatex]

function extractTextContent(node: any): string {
  if (typeof node === 'string') return node
  if (typeof node === 'number') return String(node)
  if (!node) return ''
  if (Array.isArray(node)) return node.map(extractTextContent).join('')
  if (node.props?.children) return extractTextContent(node.props.children)
  return ''
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    })
  }, [text])

  return (
    <button
      onClick={handleCopy}
      className="absolute top-2 right-2 text-xs px-2 py-1 rounded bg-border/50 text-text-dim hover:text-text hover:bg-border transition-colors"
    >
      {copied ? 'copied' : 'copy'}
    </button>
  )
}

const components: Components = {
  pre({ children, ...props }) {
    const codeText = extractTextContent(children)

    return (
      <div className="relative group">
        <pre {...props} className="bg-bg border border-border rounded-lg p-4 overflow-x-auto text-sm leading-relaxed">
          {children}
        </pre>
        <div className="opacity-0 group-hover:opacity-100 transition-opacity">
          <CopyButton text={codeText} />
        </div>
      </div>
    )
  },

  code({ className, children, ...props }) {
    const isBlock = className?.startsWith('hljs') || className?.includes('language-')
    if (isBlock) {
      return <code className={className} {...props}>{children}</code>
    }
    return (
      <code className="bg-surface border border-border rounded px-1.5 py-0.5 text-accent text-[0.85em]" {...props}>
        {children}
      </code>
    )
  },

  a({ href, children, ...props }) {
    return (
      <a
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        className="text-accent hover:text-accent-dim underline underline-offset-2"
        {...props}
      >
        {children}
      </a>
    )
  },

  img({ src, alt, ...props }) {
    // Rewrite local filesystem paths to use the local-file endpoint
    let resolvedSrc = src || ''
    if (/^(~\/|\/home\/|\/tmp\/)/.test(resolvedSrc)) {
      const abs = resolvedSrc.replace(/^~\//, '/home/jb/')
      resolvedSrc = `/local-file?path=${encodeURIComponent(abs)}`
    }
    return (
      <a href={resolvedSrc} target="_blank" rel="noopener noreferrer">
        <img
          src={resolvedSrc}
          alt={alt || ''}
          className="max-w-full rounded-lg border border-border my-2 hover:opacity-90 transition-opacity cursor-pointer"
          loading="lazy"
          {...props}
        />
      </a>
    )
  },

  input({ checked, ...props }) {
    return (
      <input
        type="checkbox"
        checked={checked}
        readOnly
        className="mr-1.5 accent-accent"
        {...props}
      />
    )
  },
}

const IMAGE_EXTS = 'png|jpg|jpeg|gif|webp|svg|bmp'

function preprocessImagePaths(content: string): string {
  // First: convert [Image: source: /path/to/file.png] patterns
  content = content.replace(
    new RegExp(`\\[(?:Image|image)[^\\]]*?(?:source:\\s*)?((~\\/|/(?:home|tmp)/)[^\\]]+\\.(?:${IMAGE_EXTS}))\\]`, 'gi'),
    (_match, filePath) => {
      const abs = filePath.replace(/^~\//, '/home/jb/')
      return `![image](/local-file?path=${encodeURIComponent(abs)})`
    }
  )

  // Then: convert bare file paths (not already inside markdown image syntax)
  content = content.replace(
    new RegExp(`(!\\[[^\\]]*\\]\\([^)]*\\))|(?:(?<=\\s|^|\`)((~\\/|/(?:home|tmp)/)[^\\s\`"'<>]+\\.(?:${IMAGE_EXTS}))(?=\\s|$|\`|[.,;:!?)]|$))`, 'gi'),
    (match, existingImg, filePath) => {
      if (existingImg) return existingImg
      const abs = filePath.replace(/^~\//, '/home/jb/')
      return `![image](/local-file?path=${encodeURIComponent(abs)})`
    }
  )

  return content
}

export function MarkdownRenderer({ content }: { content: string }) {
  const processed = preprocessImagePaths(content)
  return (
    <div className="markdown-body prose prose-invert prose-sm max-w-none">
      <Markdown
        remarkPlugins={remarkPlugins}
        rehypePlugins={rehypePlugins}
        components={components}
      >
        {processed}
      </Markdown>
    </div>
  )
}
