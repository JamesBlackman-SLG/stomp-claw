use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_used: String,
}

#[derive(Debug, Clone)]
pub enum Command {
    NewSession,
    SwitchSession(String),
    ListSessions,
    RenameSession(String),
    DeleteSession,
    ConfirmDelete,
    CancelDelete,
    VoiceOn,
    VoiceOff,
    Help,
}

#[derive(Debug, Clone)]
pub enum Event {
    // MIDI
    PedalDown,
    PedalUp,

    // Recording & Transcription
    RecordingStarted { session_id: String },
    PartialTranscript { session_id: String, text: String },
    RecordingCancelled { session_id: String },
    RecordingComplete { session_id: String, samples: Vec<f32>, duration_ms: u64 },

    // Transcription
    FinalTranscript { session_id: String, text: String },

    // Commands
    VoiceCommand { command: Command },

    // LLM
    LlmThinking { session_id: String, turn_id: i64 },
    LlmToken { session_id: String, turn_id: i64, token: String, accumulated: String },
    LlmDone { session_id: String, turn_id: i64, full_response: String, input_tokens: Option<u32>, output_tokens: Option<u32>, total_tokens: Option<u32> },
    LlmError { session_id: String, turn_id: i64, error: String },

    // Session
    SessionSwitched { session_id: String },
    SessionCreated { session: SessionInfo },
    SessionRenamed { session_id: String, name: String },
    SessionDeleted { session_id: String },

    // Config
    VoiceToggled { enabled: bool },

    // UI
    CancelRecording,
    ShowHelp,

    // UI-originated
    UserTextMessage { session_id: String, text: String, images: Vec<String>, documents: Vec<(String, String)> },
}

pub type EventSender = broadcast::Sender<Event>;
pub type EventReceiver = broadcast::Receiver<Event>;

pub fn create_event_bus(capacity: usize) -> (EventSender, EventReceiver) {
    let (tx, rx) = broadcast::channel(capacity);
    (tx, rx)
}
