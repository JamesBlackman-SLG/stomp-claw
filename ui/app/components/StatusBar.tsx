import { useState, memo } from 'react'
import { useAppState, useWs } from '../lib/state'

export const StatusBar = memo(function StatusBar() {
  const { thinking, voiceEnabled, activeSessionId, sessions } = useAppState()
  const ws = useWs()
  const [confirmingDelete, setConfirmingDelete] = useState(false)
  const [editingName, setEditingName] = useState(false)
  const [nameValue, setNameValue] = useState('')

  const activeSession = sessions.find(s => s.id === activeSessionId)

  const startRename = () => {
    if (activeSession) {
      setNameValue(activeSession.name)
      setEditingName(true)
    }
  }

  const submitRename = () => {
    if (activeSessionId && nameValue.trim()) {
      ws?.send({ type: 'rename_session', session_id: activeSessionId, name: nameValue.trim() })
    }
    setEditingName(false)
  }

  const handleDelete = () => {
    if (activeSessionId) {
      ws?.send({ type: 'delete_session', session_id: activeSessionId })
    }
    setConfirmingDelete(false)
  }

  return (
    <>
      <div className="flex items-center justify-between px-4 py-1.5 border-b border-border text-xs text-text-dim">
        <span>{thinking ? <span className="text-accent">thinking...</span> : 'Ready'}</span>
        <div className="flex items-center gap-3">
          {editingName ? (
            <input
              className="bg-bg border border-border px-1.5 py-0.5 text-xs text-text outline-none focus:border-accent w-36"
              value={nameValue}
              onChange={e => setNameValue(e.target.value)}
              onBlur={submitRename}
              onKeyDown={e => {
                if (e.key === 'Enter') submitRename()
                if (e.key === 'Escape') setEditingName(false)
              }}
              autoFocus
            />
          ) : (
            <button onClick={startRename} className="hover:text-accent transition-colors">
              rename
            </button>
          )}
          <button
            onClick={() => setConfirmingDelete(true)}
            className="hover:text-error transition-colors"
          >
            delete
          </button>
          <button
            onClick={() => ws?.send({ type: 'toggle_voice' })}
            className="hover:text-accent transition-colors"
          >
            voice: {voiceEnabled ? 'on' : 'off'}
          </button>
        </div>
      </div>

      {confirmingDelete && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70" onClick={() => setConfirmingDelete(false)}>
          <div className="bg-bg border border-border rounded-lg p-6 max-w-sm w-full mx-4" onClick={e => e.stopPropagation()}>
            <p className="text-text text-sm mb-1">Delete session?</p>
            <p className="text-text-dim text-xs mb-4">
              "{activeSession?.name}" will be removed from the sidebar.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmingDelete(false)}
                className="px-3 py-1 text-xs text-text-dim hover:text-text border border-border rounded"
              >
                Cancel
              </button>
              <button
                onClick={handleDelete}
                className="px-3 py-1 text-xs text-white bg-red-600 hover:bg-red-700 rounded"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  )
})
