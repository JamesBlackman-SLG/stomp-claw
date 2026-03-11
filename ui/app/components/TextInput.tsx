import { useState, useCallback, useRef, useEffect } from 'react'
import { useAppState, useWs } from '../lib/state'

interface PendingImage {
  file: File
  preview: string
}

interface PendingDocument {
  file: File
  filename: string
}

const DOCUMENT_TYPES = [
  'application/pdf',
  'text/csv',
  'text/plain',
  'application/json',
  'text/html',
  'text/markdown',
]

const DOCUMENT_EXTENSIONS = ['.pdf', '.csv', '.txt', '.json', '.html', '.md']

const MAX_DOC_SIZE = 5 * 1024 * 1024 // 5MB

function isDocumentFile(file: File): boolean {
  if (DOCUMENT_TYPES.includes(file.type)) return true
  return DOCUMENT_EXTENSIONS.some(ext => file.name.toLowerCase().endsWith(ext))
}

function fileToBase64DataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = reject
    reader.readAsDataURL(file)
  })
}

const MAX_IMAGE_DIM = 1024

function resizeAndEncode(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      let { width, height } = img
      if (width > MAX_IMAGE_DIM || height > MAX_IMAGE_DIM) {
        const scale = MAX_IMAGE_DIM / Math.max(width, height)
        width = Math.round(width * scale)
        height = Math.round(height * scale)
      }
      const canvas = document.createElement('canvas')
      canvas.width = width
      canvas.height = height
      const ctx = canvas.getContext('2d')!
      ctx.drawImage(img, 0, 0, width, height)
      resolve(canvas.toDataURL('image/jpeg', 0.85))
      URL.revokeObjectURL(img.src)
    }
    img.onerror = reject
    img.src = URL.createObjectURL(file)
  })
}

function isImageFile(file: File): boolean {
  return file.type.startsWith('image/')
}

export function TextInput() {
  const [text, setText] = useState('')
  const [images, setImages] = useState<PendingImage[]>([])
  const [documents, setDocuments] = useState<PendingDocument[]>([])
  const [dragOver, setDragOver] = useState(false)
  const { activeSessionId, thinking, streamingTurnId, recording, partialTranscript } = useAppState()
  const ws = useWs()
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const busy = thinking || streamingTurnId !== null
  const hasContent = text.trim().length > 0 || images.length > 0 || documents.length > 0

  // When recording, show partial transcript in the textarea area
  const displayText = recording ? (partialTranscript || '') : text

  const addDocuments = useCallback((files: File[]) => {
    const docFiles = files.filter(f => isDocumentFile(f) && f.size <= MAX_DOC_SIZE)
    const rejected = files.filter(f => isDocumentFile(f) && f.size > MAX_DOC_SIZE)
    if (rejected.length > 0) {
      console.warn(`${rejected.length} file(s) exceeded 5MB limit`)
    }
    setDocuments(prev => [...prev, ...docFiles.map(f => ({ file: f, filename: f.name }))])
  }, [])

  const removeDocument = useCallback((index: number) => {
    setDocuments(prev => prev.filter((_, i) => i !== index))
  }, [])

  const addFiles = useCallback((files: File[]) => {
    const imageFiles = files.filter(isImageFile)
    const docFiles = files.filter(f => isDocumentFile(f) && !isImageFile(f))
    if (imageFiles.length > 0) {
      const newImages = imageFiles.map(file => ({
        file,
        preview: URL.createObjectURL(file),
      }))
      setImages(prev => [...prev, ...newImages])
    }
    if (docFiles.length > 0) {
      addDocuments(docFiles)
    }
  }, [addDocuments])

  const removeImage = useCallback((index: number) => {
    setImages(prev => {
      const removed = prev[index]
      if (removed) URL.revokeObjectURL(removed.preview)
      return prev.filter((_, i) => i !== index)
    })
  }, [])

  const send = useCallback(async () => {
    if (!hasContent || !activeSessionId || busy) return

    let imageData: string[] | undefined
    if (images.length > 0) {
      imageData = await Promise.all(images.map(img => resizeAndEncode(img.file)))
    }

    let documentData: Array<{data: string; filename: string}> | undefined
    if (documents.length > 0) {
      documentData = await Promise.all(
        documents.map(async doc => ({
          data: await fileToBase64DataUrl(doc.file),
          filename: doc.filename,
        }))
      )
    }

    ws?.send({
      type: 'send_message',
      session_id: activeSessionId,
      text: text.trim(),
      ...(imageData && { images: imageData }),
      ...(documentData && { documents: documentData }),
    })

    images.forEach(img => URL.revokeObjectURL(img.preview))
    setImages([])
    setDocuments([])
    setText('')
  }, [text, images, documents, activeSessionId, busy, ws, hasContent])

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    const files = Array.from(e.clipboardData.files)
    if (files.some(f => isImageFile(f) || isDocumentFile(f))) {
      e.preventDefault()
      addFiles(files)
    }
  }, [addFiles])

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(false)
    const files = Array.from(e.dataTransfer.files)
    addFiles(files)
  }, [addFiles])

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(true)
  }, [])

  const handleDragLeave = useCallback(() => setDragOver(false), [])

  useEffect(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = Math.min(el.scrollHeight, 200) + 'px'
  }, [displayText])

  return (
    <div
      className={`border-t border-border px-2 sm:px-4 py-2 sm:py-3 ${dragOver ? 'bg-accent/10' : ''}`}
      onDrop={handleDrop}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
    >
      {images.length > 0 && (
        <div className="flex gap-2 mb-2 flex-wrap">
          {images.map((img, i) => (
            <div key={i} className="relative group">
              <img
                src={img.preview}
                alt=""
                className="w-16 h-16 object-cover rounded border border-border"
              />
              <button
                onClick={() => removeImage(i)}
                className="absolute -top-1.5 -right-1.5 w-5 h-5 bg-error text-white rounded-full text-xs flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
              >
                x
              </button>
            </div>
          ))}
        </div>
      )}
      {documents.length > 0 && (
        <div className="flex gap-2 mb-2 flex-wrap">
          {documents.map((doc, i) => (
            <div key={i} className="flex items-center gap-1.5 bg-surface border border-border rounded px-2 py-1 text-xs text-text group">
              <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/>
                <path d="M14 2v4a2 2 0 0 0 2 2h4"/>
              </svg>
              <span className="max-w-[150px] truncate">{doc.filename}</span>
              <button
                onClick={() => removeDocument(i)}
                className="w-4 h-4 text-text-dim hover:text-error transition-colors opacity-0 group-hover:opacity-100"
              >
                &times;
              </button>
            </div>
          ))}
        </div>
      )}
      {recording && (
        <div className="flex items-center gap-2 mb-2 text-xs">
          <span className="recording-pulse text-recording font-bold">REC</span>
          <button
            onClick={() => {
              setText('')
              ws?.send({ type: 'cancel_recording' })
            }}
            className="text-text-dim hover:text-error transition-colors ml-auto"
            title="Cancel and reset transcription"
          >
            &times; cancel
          </button>
        </div>
      )}
      <div className="flex gap-2">
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*,.pdf,.csv,.txt,.json,.html,.md"
          multiple
          className="hidden"
          onChange={e => {
            if (e.target.files) addFiles(Array.from(e.target.files))
            e.target.value = ''
          }}
        />
        <button
          onClick={() => fileInputRef.current?.click()}
          disabled={busy || recording}
          className="px-2 py-2 text-text-dim hover:text-text transition-colors disabled:opacity-30"
          title="Attach file"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/>
          </svg>
        </button>
        <textarea
          ref={textareaRef}
          className={`flex-1 bg-surface border rounded px-3 py-2 text-sm text-text outline-none placeholder:text-text-dim resize-none ${recording ? 'border-recording/50' : 'border-border focus:border-accent'}`}
          placeholder={recording ? 'Listening...' : busy ? 'Waiting for response...' : 'Type a message...'}
          value={displayText}
          rows={1}
          onChange={e => { if (!recording) setText(e.target.value) }}
          onKeyDown={e => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
          onPaste={handlePaste}
          disabled={busy}
          readOnly={recording}
        />
        {recording ? (
          <button
            onClick={() => {
              setText(partialTranscript || '')
              ws?.send({ type: 'cancel_recording' })
            }}
            className="px-3 sm:px-4 py-2 bg-recording/20 text-recording border border-recording/30 rounded text-sm font-medium hover:bg-recording/30 transition-colors self-end shrink-0"
          >
            Edit
          </button>
        ) : (
          <button
            onClick={send}
            disabled={busy || !hasContent}
            className="px-3 sm:px-4 py-2 bg-accent/20 text-accent border border-accent/30 rounded text-sm font-medium hover:bg-accent/30 disabled:opacity-30 disabled:cursor-not-allowed transition-colors self-end shrink-0"
          >
            Send
          </button>
        )}
      </div>
    </div>
  )
}
