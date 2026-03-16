import { useState, memo } from 'react'
import { useAppState, useWs, useDispatch } from '../lib/state'

export const SessionSidebar = memo(function SessionSidebar() {
  const { sessions, activeSessionId, sidebarOpen } = useAppState()
  const ws = useWs()
  const dispatch = useDispatch()
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editName, setEditName] = useState('')

  const startRename = (id: string, name: string) => {
    setEditingId(id)
    setEditName(name)
  }

  const submitRename = () => {
    if (editingId && editName.trim()) {
      ws?.send({ type: 'rename_session', session_id: editingId, name: editName.trim() })
    }
    setEditingId(null)
  }

  const closeSidebar = () => dispatch({ type: 'set_sidebar_open', open: false })

  const sidebarContent = (
    <aside className={`w-56 border-r border-border flex flex-col bg-surface h-full`}>
      <div className="flex items-center justify-between px-3 py-2 border-b border-border">
        <span className="text-xs text-text-dim uppercase tracking-wider">Sessions</span>
        <div className="flex items-center gap-2">
          <button
            onClick={() => ws?.send({ type: 'create_session' })}
            className="text-accent hover:text-accent-dim text-sm font-bold"
            title="New session"
          >
            +
          </button>
          <button
            onClick={closeSidebar}
            className="text-text-dim hover:text-text text-sm md:hidden"
            title="Close sidebar"
          >
            ✕
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto">
        {sessions.map(session => (
          <div
            key={session.id}
            className={`group flex items-center gap-2 px-3 py-2 cursor-pointer select-none text-sm border-l-2 ${
              session.id === activeSessionId
                ? 'border-accent bg-surface-hover text-text'
                : 'border-transparent hover:bg-surface-hover text-text-dim'
            }`}
            style={{ WebkitTapHighlightColor: 'transparent' }}
            onClick={() => ws?.send({ type: 'switch_session', session_id: session.id })}
          >
            {session.id === activeSessionId && (
              <span className="w-1.5 h-1.5 rounded-full bg-accent flex-shrink-0" />
            )}
            {editingId === session.id ? (
              <input
                className="flex-1 bg-bg border border-border px-1 py-0.5 text-xs text-text outline-none focus:border-accent"
                value={editName}
                onChange={e => setEditName(e.target.value)}
                onBlur={submitRename}
                onKeyDown={e => {
                  if (e.key === 'Enter') submitRename()
                  if (e.key === 'Escape') setEditingId(null)
                }}
                autoFocus
                onClick={e => e.stopPropagation()}
              />
            ) : (
              <span className="flex-1 truncate text-xs select-none">{session.name}</span>
            )}
            <div className="hidden group-hover:flex items-center gap-1">
              <button
                onClick={e => { e.stopPropagation(); startRename(session.id, session.name) }}
                className="text-text-dim hover:text-text text-xs"
                title="Rename"
              >
                r
              </button>
              <button
                onClick={e => { e.stopPropagation(); ws?.send({ type: 'delete_session', session_id: session.id }) }}
                className="text-text-dim hover:text-error text-xs"
                title="Delete"
              >
                x
              </button>
            </div>
          </div>
        ))}
      </div>
    </aside>
  )

  return (
    <>
      {/* Desktop: always visible */}
      <div className="hidden md:flex">
        {sidebarContent}
      </div>

      {/* Mobile: overlay when open */}
      {sidebarOpen && (
        <div className="fixed inset-0 z-40 flex md:hidden">
          <div className="flex-shrink-0">
            {sidebarContent}
          </div>
          <div className="flex-1 bg-black/60" onClick={closeSidebar} />
        </div>
      )}
    </>
  )
})
