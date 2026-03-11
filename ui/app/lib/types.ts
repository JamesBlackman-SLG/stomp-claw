export interface Session {
  id: string
  name: string
  created_at: string
  last_used: string
}

export interface Turn {
  id: number
  session_id: string
  role: 'user' | 'assistant'
  content: string
  status: 'pending' | 'streaming' | 'complete' | 'error'
  created_at: string
  completed_at: string | null
  images: string[] | null
  documents: string | null
}

// Server -> Client messages
export type WsMessage =
  | { type: 'session_list'; sessions: Session[] }
  | { type: 'session_switched'; session_id: string }
  | { type: 'session_created'; session: Session }
  | { type: 'session_renamed'; session_id: string; name: string }
  | { type: 'session_deleted'; session_id: string }
  | { type: 'turn_list'; session_id: string; turns: Turn[] }
  | { type: 'turn_created'; turn: Turn }
  | { type: 'recording_started'; session_id: string }
  | { type: 'recording_cancelled'; session_id: string }
  | { type: 'partial_transcript'; session_id: string; text: string }
  | { type: 'llm_thinking'; session_id: string; turn_id: number }
  | { type: 'llm_token'; session_id: string; turn_id: number; token: string; accumulated: string }
  | { type: 'llm_done'; session_id: string; turn_id: number; content: string }
  | { type: 'llm_error'; session_id: string; turn_id: number; error: string }
  | { type: 'voice_toggled'; enabled: boolean }
  | { type: 'show_help' }
  | { type: 'config'; voice_enabled: boolean; active_session_id: string }

// Client -> Server messages
export type WsCommand =
  | { type: 'send_message'; session_id: string; text: string; images?: string[]; documents?: Array<{data: string; filename: string}> }
  | { type: 'switch_session'; session_id: string }
  | { type: 'create_session' }
  | { type: 'rename_session'; session_id: string; name: string }
  | { type: 'delete_session'; session_id: string }
  | { type: 'cancel_recording' }
  | { type: 'toggle_voice' }
