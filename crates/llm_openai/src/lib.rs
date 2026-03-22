use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use transcript_core::TranscriptSegment;

#[derive(Debug, Clone, Copy)]
pub enum ResponseMode {
    AnswerQuestion,
    Commentary,
    SummariseRecent,
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
}

#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    pub api_key: Option<String>,
    pub model: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub mode: String,
    pub should_respond: bool,
    pub answer: String,
    pub confidence: f32,
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("OpenAI is not configured")]
    NotConfigured,
    #[error("OpenAI returned an unexpected payload")]
    InvalidPayload,
}

pub struct OpenAiReasoner {
    client: Client,
    config: OpenAiConfig,
}

impl OpenAiReasoner {
    pub fn new(config: OpenAiConfig) -> Self {
        Self { client: Client::new(), config }
    }

    pub async fn respond(
        &self,
        mode: ResponseMode,
        transcript: &[TranscriptSegment],
        context: &AssistantContextInput,
    ) -> Result<LlmResponse> {
        if !self.config.enabled {
            return Ok(fallback_response(mode, transcript));
        }

        let api_key = self.config.api_key.as_ref().context(LlmError::NotConfigured)?;
        let prompt = build_prompt(mode, transcript, context);
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string" },
                "should_respond": { "type": "boolean" },
                "answer": { "type": "string" },
                "confidence": { "type": "number" }
            },
            "required": ["mode", "should_respond", "answer", "confidence"],
            "additionalProperties": false
        });

        let body = json!({
            "model": self.config.model,
            "input": prompt,
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "soundmind_response",
                    "strict": true,
                    "schema": schema
                }
            }
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .context("failed to reach OpenAI Responses API")?
            .error_for_status()
            .context("OpenAI Responses API returned an error")?;

        let payload: Value = response.json().await.context("failed to decode OpenAI response")?;
        let output_text = payload
            .get("output_text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| extract_output_text(&payload))
            .ok_or_else(|| anyhow!(LlmError::InvalidPayload))?;

        serde_json::from_str(&output_text).context("failed to parse structured OpenAI output")
    }
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
            "Answer the last detected interview question in 3 to 5 short bullet points. Start with the strongest direct answer, then add supporting points, examples, or follow-up angles only if they help."
        }
        ResponseMode::Commentary => {
            "Provide 1 to 3 brief bullet points with the most useful live interview guidance right now. Focus on what the user should emphasize, clarify, or avoid."
        }
        ResponseMode::SummariseRecent => {
            "Summarise the recent interview exchange in 3 to 5 short bullet points. Highlight the interviewer intent, key themes, and any likely next question."
        }
    };

    let priming = render_priming_documents(&context.priming_documents);

    format!(
        "Primary assistant instruction:\n{}\n\nPriming documents:\n{}\n\nTask:\n{instruction}\nReturn strict JSON with mode, should_respond, answer, and confidence.\nFormat answer as plain text bullets when useful, using '-' prefixes and short lines. Optimize for fast reading during a live interview. Ground your response in the transcript and uploaded documents. Do not invent credentials, experience, or facts not supported by the provided context.\nTranscript:\n{rendered}",
        context.instruction,
        priming,
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

fn extract_output_text(payload: &Value) -> Option<String> {
    payload.get("output").and_then(Value::as_array).and_then(|items| {
        items.iter().find_map(|item| {
            item.get("content").and_then(Value::as_array).and_then(|content| {
                content.iter().find_map(|entry| {
                    entry.get("text").and_then(Value::as_str).map(ToOwned::to_owned)
                })
            })
        })
    })
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
                    "- OpenAI is disabled.\n- Latest question: {}\n- Use your CV and job description uploads to answer this directly.",
                    segment.text
                )
            })
            .unwrap_or_else(|| {
                "- OpenAI is disabled.\n- No recent question was found yet.".to_string()
            }),
        ResponseMode::Commentary => {
            format!(
                "- OpenAI is disabled.\n- Recent transcript topic: {}",
                clip(&transcript_text)
            )
        }
        ResponseMode::SummariseRecent => {
            format!(
                "- OpenAI is disabled.\n- Recent transcript summary: {}",
                clip(&transcript_text)
            )
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
