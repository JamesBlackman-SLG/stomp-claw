import { useAppState, useWs } from '../lib/state'

export function StatusBar() {
  const { recording, partialTranscript, thinking, voiceEnabled } = useAppState()
  const ws = useWs()

  if (!recording && !thinking && !partialTranscript) {
    return (
      <div className="flex items-center justify-between px-4 py-1.5 border-b border-border text-xs text-text-dim">
        <span>Ready</span>
        <button
          onClick={() => ws?.send({ type: 'toggle_voice' })}
          className="hover:text-accent transition-colors"
        >
          voice: {voiceEnabled ? 'on' : 'off'}
        </button>
      </div>
    )
  }

  return (
    <div className="flex items-center gap-3 px-4 py-1.5 border-b border-border text-xs">
      {recording && (
        <>
          <span className="recording-pulse text-recording font-bold">REC</span>
          {partialTranscript && (
            <span className="text-text-dim truncate">{partialTranscript}</span>
          )}
        </>
      )}
      {thinking && (
        <span className="text-accent">thinking...</span>
      )}
    </div>
  )
}
