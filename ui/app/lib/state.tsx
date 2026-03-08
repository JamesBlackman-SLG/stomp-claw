import { createContext, useContext, useReducer, useEffect, useRef, type ReactNode } from 'react'
import type { Session, Turn, WsMessage } from './types'
import { WebSocketManager } from './ws'

interface AppState {
  sessions: Session[]
  activeSessionId: string
  turns: Map<string, Turn[]>
  streamingTurnId: number | null
  streamingContent: string
  recording: boolean
  partialTranscript: string
  voiceEnabled: boolean
  connected: boolean
  thinking: boolean
  showHelp: boolean
}

type Action =
  | { type: 'ws_message'; msg: WsMessage }
  | { type: 'set_connected'; connected: boolean }
  | { type: 'set_show_help'; show: boolean }

const initialState: AppState = {
  sessions: [],
  activeSessionId: '',
  turns: new Map(),
  streamingTurnId: null,
  streamingContent: '',
  recording: false,
  partialTranscript: '',
  voiceEnabled: true,
  connected: false,
  thinking: false,
  showHelp: false,
}

function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case 'set_show_help':
      return { ...state, showHelp: action.show }

    case 'set_connected':
      if (action.connected) {
        // On reconnect, clear all stale state — server will send fresh data
        return { ...state, connected: true, turns: new Map(), streamingTurnId: null, streamingContent: '', thinking: false }
      }
      return { ...state, connected: false }

    case 'ws_message': {
      const msg = action.msg
      switch (msg.type) {
        case 'config':
          return { ...state, voiceEnabled: msg.voice_enabled, activeSessionId: msg.active_session_id }
        case 'session_list':
          return { ...state, sessions: msg.sessions }
        case 'session_switched':
          return { ...state, activeSessionId: msg.session_id, turns: new Map(), recording: false, partialTranscript: '', thinking: false, streamingTurnId: null, streamingContent: '' }
        case 'session_created':
          return { ...state, sessions: [...state.sessions, msg.session] }
        case 'session_renamed':
          return { ...state, sessions: state.sessions.map(s => s.id === msg.session_id ? { ...s, name: msg.name } : s) }
        case 'session_deleted':
          return { ...state, sessions: state.sessions.filter(s => s.id !== msg.session_id) }
        case 'turn_list': {
          const newTurns = new Map(state.turns)
          newTurns.set(msg.session_id, [...msg.turns])
          return { ...state, turns: newTurns }
        }
        case 'turn_created': {
          const newTurns = new Map(state.turns)
          const existing = newTurns.get(msg.turn.session_id) || []
          newTurns.set(msg.turn.session_id, [...existing, msg.turn])
          return { ...state, turns: newTurns }
        }
        case 'recording_started':
          return { ...state, recording: true, partialTranscript: '' }
        case 'recording_cancelled':
          return { ...state, recording: false, partialTranscript: '' }
        case 'partial_transcript':
          return { ...state, partialTranscript: msg.text }
        case 'llm_thinking':
          return { ...state, thinking: true, recording: false, streamingTurnId: msg.turn_id }
        case 'llm_token':
          return { ...state, thinking: false, streamingContent: msg.accumulated, streamingTurnId: msg.turn_id }
        case 'llm_done': {
          // Immutable update — don't mutate the existing array
          const newTurns = new Map(state.turns)
          const existing = [...(newTurns.get(msg.session_id) || [])]
          const completedTurn: Turn = {
            id: msg.turn_id, session_id: msg.session_id, role: 'assistant',
            content: msg.content, status: 'complete', created_at: '', completed_at: null, images: null,
          }
          const existingIdx = existing.findIndex(t => t.id === msg.turn_id)
          if (existingIdx >= 0) {
            existing[existingIdx] = completedTurn
          } else {
            existing.push(completedTurn)
          }
          newTurns.set(msg.session_id, existing)
          return { ...state, turns: newTurns, streamingTurnId: null, streamingContent: '', thinking: false }
        }
        case 'llm_error': {
          // Update the assistant turn to show error status
          const newTurns = new Map(state.turns)
          if (msg.turn_id && msg.session_id) {
            const existing = [...(newTurns.get(msg.session_id) || [])]
            const idx = existing.findIndex(t => t.id === msg.turn_id)
            if (idx >= 0) {
              existing[idx] = { ...existing[idx], status: 'error', content: msg.error }
              newTurns.set(msg.session_id, existing)
            }
          }
          return { ...state, turns: newTurns, streamingTurnId: null, streamingContent: '', thinking: false }
        }
        case 'voice_toggled':
          return { ...state, voiceEnabled: msg.enabled }
        case 'show_help':
          return { ...state, showHelp: true }
        default:
          return state
      }
    }
    default:
      return state
  }
}

const StateContext = createContext<AppState>(initialState)
const DispatchContext = createContext<React.Dispatch<Action>>(() => {})
const WsContext = createContext<WebSocketManager | null>(null)

export function AppProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState)
  const wsRef = useRef<WebSocketManager | null>(null)

  useEffect(() => {
    const ws = new WebSocketManager(
      (msg) => dispatch({ type: 'ws_message', msg }),
      (connected) => dispatch({ type: 'set_connected', connected }),
    )
    wsRef.current = ws
    ws.connect()
    return () => ws.disconnect()
  }, [])

  return (
    <WsContext.Provider value={wsRef.current}>
      <DispatchContext.Provider value={dispatch}>
        <StateContext.Provider value={state}>
          {children}
        </StateContext.Provider>
      </DispatchContext.Provider>
    </WsContext.Provider>
  )
}

export function useAppState() { return useContext(StateContext) }
export function useDispatch() { return useContext(DispatchContext) }
export function useWs() { return useContext(WsContext) }
