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
pub struct AppSettingsDto {
    pub retention_days: u32,
    pub transcript_storage_enabled: bool,
    pub auto_start_cloud: bool,
    pub default_mode: AppMode,
    pub assistant_instruction: String,
}

impl Default for AppSettingsDto {
    fn default() -> Self {
        Self {
            retention_days: 0,
            transcript_storage_enabled: true,
            auto_start_cloud: false,
            default_mode: AppMode::ManualQa,
            assistant_instruction: default_assistant_instruction(),
        }
    }
}

pub fn default_assistant_instruction() -> String {
    "You are assisting the user during a live job interview. Use the live transcript plus any uploaded priming documents such as the user's CV, the job description, company notes, or project history. Summarize what the interviewer is asking, suggest concise high-quality answers tailored to the user's background, point out likely follow-up questions, and avoid inventing experience or qualifications that are not supported by the transcript or uploaded documents. Prefer short bullet points when they improve speed of reading. Keep advice compact, practical, and easy to scan while the user is speaking with an interviewer.".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrimingDocumentDto {
    pub id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    pub char_count: u32,
    pub preview_text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantEventDto {
    pub id: Uuid,
    pub session_id: Uuid,
    pub kind: String,
    pub content: String,
    pub confidence: f32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionSummaryDto {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub capture_device: String,
    pub mode: String,
    pub transcript_segment_count: u32,
    pub assistant_event_count: u32,
    pub latest_transcript_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionDetailDto {
    pub session: SessionSummaryDto,
    pub transcript_segments: Vec<TranscriptSegmentDto>,
    pub assistant_events: Vec<AssistantEventDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackendStatusSnapshot {
    pub mode: AppMode,
    pub capture_state: CaptureState,
    pub cloud_state: CloudState,
    pub audio_upload_active: bool,
    pub current_sink: Option<String>,
    pub current_monitor_source: Option<String>,
    pub stt_provider: Option<String>,
    pub stt_status: Option<String>,
    pub session_id: Option<Uuid>,
    pub transcript: TranscriptSnapshot,
    pub detected_question: Option<TranscriptSegmentDto>,
    pub latest_assistant: Option<AssistantOutput>,
    pub recent_errors: Vec<String>,
    pub privacy_pause: bool,
    pub cloud_pause: bool,
    pub cloud_auto_pause: bool,
}

impl Default for BackendStatusSnapshot {
    fn default() -> Self {
        Self {
            mode: AppMode::ManualQa,
            capture_state: CaptureState::Idle,
            cloud_state: CloudState::Off,
            audio_upload_active: false,
            current_sink: None,
            current_monitor_source: None,
            stt_provider: None,
            stt_status: None,
            session_id: None,
            transcript: TranscriptSnapshot { partial_text: None, segments: Vec::new() },
            detected_question: None,
            latest_assistant: None,
            recent_errors: Vec::new(),
            privacy_pause: false,
            cloud_pause: false,
            cloud_auto_pause: false,
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
