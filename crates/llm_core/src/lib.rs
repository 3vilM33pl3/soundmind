use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use transcript_core::TranscriptSegment;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResponseMode {
    AnswerQuestion,
    Commentary,
    SummariseRecent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelLocality {
    Remote,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelCapability {
    StructuredOutput,
    TranscriptReasoning,
    PrimingDocuments,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub provider_id: String,
    pub model_id: String,
    pub locality: ModelLocality,
    pub capabilities: Vec<ModelCapability>,
}

#[derive(Debug, Clone)]
pub struct PrimingDocumentInput {
    pub file_name: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct AssistantContextInput {
    pub instruction: String,
    pub priming_documents: Vec<PrimingDocumentInput>,
    pub focus_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub mode: String,
    pub should_respond: bool,
    pub answer: String,
    pub confidence: f32,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn models(&self) -> Vec<ModelDescriptor>;
    async fn respond(
        &self,
        model: &str,
        mode: ResponseMode,
        transcript: &[TranscriptSegment],
        context: &AssistantContextInput,
    ) -> Result<LlmResponse>;
}
