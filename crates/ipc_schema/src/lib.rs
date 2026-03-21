use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AppMode {
    CaptionsOnly,
    ManualQa,
    Assisted,
    Summary,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CaptureState {
    Idle,
    Capturing,
    Paused,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CloudState {
    Off,
    SttActive,
    LlmActive,
    Paused,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssistantKind {
    Answer,
    Commentary,
    Summary,
    Notice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptSegmentDto {
    pub id: Uuid,
    pub session_id: Uuid,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptSnapshot {
    pub partial_text: Option<String>,
    pub segments: Vec<TranscriptSegmentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantOutput {
    pub kind: AssistantKind,
    pub content: String,
    pub confidence: Option<f32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackendStatusSnapshot {
    pub mode: AppMode,
    pub capture_state: CaptureState,
    pub cloud_state: CloudState,
    pub current_sink: Option<String>,
    pub current_monitor_source: Option<String>,
    pub stt_provider: Option<String>,
    pub stt_status: Option<String>,
    pub session_id: Option<Uuid>,
    pub transcript: TranscriptSnapshot,
    pub latest_assistant: Option<AssistantOutput>,
    pub recent_errors: Vec<String>,
    pub privacy_pause: bool,
    pub cloud_pause: bool,
}

impl Default for BackendStatusSnapshot {
    fn default() -> Self {
        Self {
            mode: AppMode::ManualQa,
            capture_state: CaptureState::Idle,
            cloud_state: CloudState::Off,
            current_sink: None,
            current_monitor_source: None,
            stt_provider: None,
            stt_status: None,
            session_id: None,
            transcript: TranscriptSnapshot { partial_text: None, segments: Vec::new() },
            latest_assistant: None,
            recent_errors: Vec::new(),
            privacy_pause: false,
            cloud_pause: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UserAction {
    Start,
    Stop,
    PauseCloud,
    ResumeCloud,
    AnswerLastQuestion,
    SummariseLastMinute,
    CommentCurrentTopic,
    SetMode(AppMode),
}
