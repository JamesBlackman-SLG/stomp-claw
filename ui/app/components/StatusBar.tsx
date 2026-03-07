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
    <div className="flex items-start gap-3 px-4 py-1.5 border-b border-border text-xs flex-wrap">
      {recording && (
        <>
          <span className="recording-pulse text-recording font-bold shrink-0">REC</span>
          {partialTranscript && (
            <span className="text-text-dim break-words flex-1 min-w-0">{partialTranscript}</span>
          )}
          <button
            onClick={() => ws?.send({ type: 'cancel_recording' })}
            className="shrink-0 ml-auto text-text-dim hover:text-error transition-colors"
            title="Cancel and reset transcription"
          >
            &times;
          </button>
        </>
      )}
      {thinking && (
        <span className="text-accent">thinking...</span>
      )}
    </div>
  )
}
