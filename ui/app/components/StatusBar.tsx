import { useAppState, useWs } from '../lib/state'

export function StatusBar() {
  const { thinking, voiceEnabled } = useAppState()
  const ws = useWs()

  return (
    <div className="flex items-center justify-between px-4 py-1.5 border-b border-border text-xs text-text-dim">
      <span>{thinking ? <span className="text-accent">thinking...</span> : 'Ready'}</span>
      <button
        onClick={() => ws?.send({ type: 'toggle_voice' })}
        className="hover:text-accent transition-colors"
      >
        voice: {voiceEnabled ? 'on' : 'off'}
      </button>
    </div>
  )
}
