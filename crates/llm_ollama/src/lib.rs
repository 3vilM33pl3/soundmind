use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use llm_core::{
    AssistantContextInput, LlmProvider, LlmResponse, ModelCapability, ModelDescriptor,
    ModelLocality, PrimingDocumentInput, ResponseMode,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use transcript_core::TranscriptSegment;

#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub base_url: String,
    pub enabled: bool,
    pub default_model: String,
}

pub struct OllamaReasoner {
    client: Client,
    config: OllamaConfig,
}

impl OllamaReasoner {
    pub fn new(config: OllamaConfig) -> Self {
        Self { client: Client::new(), config }
    }

    async fn respond_inner(
        &self,
        model: &str,
        mode: ResponseMode,
        transcript: &[TranscriptSegment],
        context: &AssistantContextInput,
    ) -> Result<LlmResponse> {
        if !self.config.enabled {
            return Ok(fallback_response(mode, transcript));
        }

        let prompt = build_prompt(mode, transcript, context);
        let response = self
            .client
            .post(format!("{}/api/generate", self.config.base_url.trim_end_matches('/')))
            .json(&json!({
                "model": if model.trim().is_empty() { self.config.default_model.as_str() } else { model },
                "prompt": prompt,
                "stream": false,
                "format": {
                    "type": "object",
                    "properties": {
                        "mode": { "type": "string" },
                        "should_respond": { "type": "boolean" },
                        "answer": { "type": "string" },
                        "confidence": { "type": "number" }
                    },
                    "required": ["mode", "should_respond", "answer", "confidence"],
                    "additionalProperties": false
                }
            }))
            .send()
            .await
            .context("failed to reach Ollama API")?
            .error_for_status()
            .context("Ollama API returned an error")?;

        let payload: OllamaGenerateResponse =
            response.json().await.context("failed to decode Ollama response")?;
        serde_json::from_str(&payload.response).context("failed to parse structured Ollama output")
    }
}

#[async_trait]
impl LlmProvider for OllamaReasoner {
    fn provider_id(&self) -> &'static str {
        "ollama"
    }

    fn models(&self) -> Vec<ModelDescriptor> {
        vec![
            self.config.default_model.clone(),
            "llama3.2:3b-instruct-q4_K_M".to_string(),
            "qwen2.5:7b-instruct".to_string(),
            "mistral:7b-instruct".to_string(),
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|model_id| ModelDescriptor {
            provider_id: self.provider_id().to_string(),
            model_id,
            locality: ModelLocality::Local,
            capabilities: vec![
                ModelCapability::StructuredOutput,
                ModelCapability::TranscriptReasoning,
                ModelCapability::PrimingDocuments,
            ],
        })
        .collect()
    }

    async fn respond(
        &self,
        model: &str,
        mode: ResponseMode,
        transcript: &[TranscriptSegment],
        context: &AssistantContextInput,
    ) -> Result<LlmResponse> {
        self.respond_inner(model, mode, transcript, context).await
    }
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

fn build_prompt(
    mode: ResponseMode,
    transcript: &[TranscriptSegment],
    context: &AssistantContextInput,
) -> String {
    let rendered = transcript
        .iter()
        .map(|segment| format!("[{}-{}ms] {}", segment.start_ms, segment.end_ms, segment.text))
        .collect::<Vec<_>>()
        .join("\n");

    let instruction = match mode {
        ResponseMode::AnswerQuestion => {
            "Answer the last detected interview question as 3 to 5 short bullet points only. Start with the strongest direct answer, then add supporting points only if they help."
        }
        ResponseMode::Commentary => {
            "Provide 1 to 3 brief bullet points only with the most useful live interview guidance right now."
        }
        ResponseMode::SummariseRecent => {
            "Summarise the recent interview exchange as 3 to 5 short bullet points only."
        }
    };

    let priming = render_priming_documents(&context.priming_documents);
    let focus_excerpt = context
        .focus_excerpt
        .as_ref()
        .map(|focus| format!("\n\nSelected focus excerpt:\n{focus}"))
        .unwrap_or_default();

    format!(
        "Primary assistant instruction:\n{}\n\nPriming documents:\n{}{}\n\nTask:\n{instruction}\nReturn strict JSON with mode, should_respond, answer, and confidence.\nFormat answer as plain text bullet lines only, using '-' prefixes and short lines. Optimize for fast reading during a live interview. If a selected focus excerpt is provided, prioritize it while still using nearby transcript context. Ground your response in the transcript and uploaded documents. Do not invent credentials, experience, or facts not supported by the provided context.\nTranscript:\n{rendered}",
        context.instruction, priming, focus_excerpt,
    )
}

fn render_priming_documents(documents: &[PrimingDocumentInput]) -> String {
    if documents.is_empty() {
        return "No priming documents uploaded.".to_string();
    }

    let mut remaining_chars = 12_000usize;
    let mut sections = Vec::new();

    for document in documents {
        if remaining_chars < 200 {
            break;
        }

        let clipped = clip_to_chars(&document.text, remaining_chars.saturating_sub(80));
        remaining_chars = remaining_chars.saturating_sub(clipped.chars().count() + 80);
        sections.push(format!("Document: {}\n{}", document.file_name, clipped));
    }

    sections.join("\n\n")
}

fn fallback_response(mode: ResponseMode, transcript: &[TranscriptSegment]) -> LlmResponse {
    let transcript_text =
        transcript.iter().map(|segment| segment.text.clone()).collect::<Vec<_>>().join(" ");

    let answer = match mode {
        ResponseMode::AnswerQuestion => transcript
            .iter()
            .rev()
            .find(|segment| segment.text.contains('?'))
            .map(|segment| {
                format!(
                    "- Ollama is disabled.\n- Latest question: {}\n- Use your CV and job description uploads to answer this directly.",
                    segment.text
                )
            })
            .unwrap_or_else(|| {
                "- Ollama is disabled.\n- No recent question was found yet.".to_string()
            }),
        ResponseMode::Commentary => format!("- Ollama is disabled.\n- Recent transcript topic: {}", clip(&transcript_text)),
        ResponseMode::SummariseRecent => {
            format!("- Ollama is disabled.\n- Recent transcript summary: {}", clip(&transcript_text))
        }
    };

    LlmResponse { mode: format!("{mode:?}"), should_respond: true, answer, confidence: 0.2 }
}

fn clip(text: &str) -> String {
    if text.is_empty() {
        return "no recent transcript available".to_string();
    }

    let mut clipped = text.chars().take(180).collect::<String>();
    if text.chars().count() > 180 {
        clipped.push_str("...");
    }
    clipped
}

fn clip_to_chars(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut clipped = normalized.chars().take(max_chars).collect::<String>();
    if normalized.chars().count() > max_chars {
        clipped.push_str("...");
    }
    clipped
}

pub fn assistant_timestamp() -> chrono::DateTime<Utc> {
    Utc::now()
}
