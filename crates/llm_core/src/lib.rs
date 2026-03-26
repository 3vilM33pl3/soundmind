use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
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

#[derive(Clone, Default)]
pub struct LlmProviderRegistry {
    providers: HashMap<String, Arc<dyn LlmProvider + Send + Sync>>,
}

impl LlmProviderRegistry {
    pub fn new(providers: Vec<Arc<dyn LlmProvider + Send + Sync>>) -> Self {
        let providers = providers
            .into_iter()
            .map(|provider| (provider.provider_id().to_string(), provider))
            .collect();
        Self { providers }
    }

    pub fn provider(&self, provider_id: &str) -> Option<Arc<dyn LlmProvider + Send + Sync>> {
        self.providers.get(provider_id).cloned()
    }

    pub fn models(&self) -> Vec<ModelDescriptor> {
        let mut models = self
            .providers
            .values()
            .flat_map(|provider| provider.models())
            .collect::<Vec<_>>();
        models.sort_by(|left, right| {
            left.provider_id
                .cmp(&right.provider_id)
                .then_with(|| left.model_id.cmp(&right.model_id))
        });
        models
    }

    pub fn default_selection(&self) -> Option<(String, String)> {
        self.models()
            .into_iter()
            .next()
            .map(|descriptor| (descriptor.provider_id, descriptor.model_id))
    }
}
