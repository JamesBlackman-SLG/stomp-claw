import { memo } from 'react'
import { useAppState } from '../lib/state'

function formatTokens(n: number): string {
  if (n >= 1000) return (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k'
  return String(n)
}

export const ContextBar = memo(function ContextBar() {
  const { totalTokens, contextWindow } = useAppState()

  if (totalTokens == null || contextWindow == null) return null

  const pct = Math.min((totalTokens / contextWindow) * 100, 100)
  const color = pct >= 80 ? 'text-error' : pct >= 50 ? 'text-yellow-400' : 'text-text-dim'

  return (
    <div className="flex items-center gap-3 px-4 py-1 border-t border-border text-[10px] text-text-dim font-mono">
      <div className="flex items-center gap-2 flex-1">
        <div className="w-24 h-1.5 bg-surface rounded-full overflow-hidden">
          <div
            className={`h-full rounded-full transition-all duration-500 ${
              pct >= 80 ? 'bg-error' : pct >= 50 ? 'bg-yellow-400' : 'bg-accent-dim'
            }`}
            style={{ width: `${pct}%` }}
          />
        </div>
        <span className={color}>
          {formatTokens(totalTokens)} / {formatTokens(contextWindow)} context
        </span>
      </div>
      <span className="text-text-dim">{pct.toFixed(0)}%</span>
    </div>
  )
})
