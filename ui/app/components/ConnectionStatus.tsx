import { memo } from 'react'
import { useAppState } from '../lib/state'

export const ConnectionStatus = memo(function ConnectionStatus() {
  const { connected } = useAppState()
  return (
    <div className="flex items-center gap-2 text-xs text-text-dim">
      <div className={`w-2 h-2 rounded-full ${connected ? 'bg-accent' : 'bg-error'}`} />
      {connected ? 'connected' : 'disconnected'}
    </div>
  )
})
