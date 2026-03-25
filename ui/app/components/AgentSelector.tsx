import { memo, useState, useRef, useEffect } from 'react'
import { useAppState, useWs } from '../lib/state'

export const AgentSelector = memo(function AgentSelector() {
  const { agents, activeAgentId } = useAppState()
  const ws = useWs()
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [])

  const activeAgent = agents.find(a => a.id === activeAgentId)

  if (agents.length < 2) return null

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1.5 text-xs text-text-dim hover:text-accent border border-border rounded-full px-2.5 py-0.5 hover:border-accent transition-colors"
      >
        {activeAgent?.name ?? 'Agent'}
        <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 bg-surface border border-border rounded-lg py-1 min-w-[120px] z-50">
          {agents.map(agent => (
            <button
              key={agent.id}
              onClick={() => {
                if (agent.id !== activeAgentId) {
                  ws?.send({ type: 'switch_agent', agent_id: agent.id })
                }
                setOpen(false)
              }}
              className={`block w-full text-left px-3 py-1.5 text-xs transition-colors ${
                agent.id === activeAgentId
                  ? 'text-accent'
                  : 'text-text-dim hover:text-text hover:bg-surface-hover'
              }`}
            >
              {agent.name}
            </button>
          ))}
        </div>
      )}
    </div>
  )
})
