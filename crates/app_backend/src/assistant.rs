use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use context_engine::last_question_candidate;
use ipc_schema::{
    AppSettingsDto, AssistantKind, AssistantOutput, BackendStatusSnapshot, CloudState,
    TranscriptSegmentDto, TranscriptSelectionPayload, TranscriptSnapshot, default_assistant_instruction,
    default_openai_model,
};
use llm_core::{AssistantContextInput, LlmProvider, PrimingDocumentInput, ResponseMode};
use llm_openai::assistant_timestamp;
use policy_engine::PolicyState;
use storage_sqlite::Storage;
use tokio::sync::RwLock;
use transcript_core::{TranscriptSegment, TranscriptState, is_question_candidate};
use uuid::Uuid;

use crate::{MAX_HEALTH_SEGMENTS, UploadGate, effective_cloud_state, sync_upload_state};

#[derive(Debug, Clone)]
pub(crate) struct AssistantRequestIdentity {
    pub(crate) request_kind: String,
    pub(crate) request_key: String,
    pub(crate) request_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssistantSurface {
    Manual,
    Automatic,
}

pub(crate) async fn handle_action(
    action: ipc_schema::UserAction,
    session_id: Uuid,
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    storage: &Storage,
    settings: &Arc<RwLock<AppSettingsDto>>,
    llm_provider: &(dyn LlmProvider + Send + Sync),
    transcript: &mut TranscriptState,
    policy: &mut PolicyState,
    upload_gate: &mut UploadGate,
) -> Result<()> {
    let audit_event = format!("user_action:{action:?}");
    match action {
        ipc_schema::UserAction::Start => {
            let mut locked = snapshot.write().await;
            locked.privacy_pause = false;
            locked.capture_state = ipc_schema::CaptureState::Capturing;
        }
        ipc_schema::UserAction::Stop => {
            upload_gate.mark_privacy_paused();
            transcript.clear_partial();
            let mut locked = snapshot.write().await;
            locked.privacy_pause = true;
            locked.capture_state = ipc_schema::CaptureState::Paused;
        }
        ipc_schema::UserAction::PauseCloud => {
            policy.cloud_paused = true;
            upload_gate.mark_manual_cloud_paused();
            let mut locked = snapshot.write().await;
            locked.cloud_pause = true;
        }
        ipc_schema::UserAction::ResumeCloud => {
            policy.cloud_paused = false;
            let mut locked = snapshot.write().await;
            locked.cloud_pause = false;
        }
        ipc_schema::UserAction::SetMode(mode) => {
            policy.mode = mode;
            snapshot.write().await.mode = mode;
            let mut runtime_settings = settings.write().await;
            runtime_settings.default_mode = mode;
            storage.save_settings(&runtime_settings).await?;
        }
        ipc_schema::UserAction::AnswerLastQuestion => {
            let last_question = last_question_candidate(transcript)
                .map(|segment| vec![segment])
                .unwrap_or_else(|| context_engine::recent_transcript_window(transcript, 60));
            let generated = maybe_generate_response(
                AssistantSurface::Manual,
                ResponseMode::AnswerQuestion,
                "answer",
                last_question,
                None,
                session_id,
                snapshot,
                storage,
                settings,
                llm_provider,
                policy,
                true,
            )
            .await?;
            if generated {
                policy.mark_response_sent(Utc::now());
            }
        }
        ipc_schema::UserAction::AnswerQuestionBySegment { segment_id } => {
            let generated = maybe_generate_response(
                AssistantSurface::Manual,
                ResponseMode::AnswerQuestion,
                "answer",
                transcript_window_for_segment_ids(transcript, &[segment_id]),
                selection_focus_excerpt(transcript, &[segment_id], None),
                session_id,
                snapshot,
                storage,
                settings,
                llm_provider,
                policy,
                true,
            )
            .await?;
            if generated {
                policy.mark_response_sent(Utc::now());
            }
        }
        ipc_schema::UserAction::AnswerSelection(selection) => {
            let generated = maybe_generate_response(
                AssistantSurface::Manual,
                ResponseMode::AnswerQuestion,
                "answer",
                transcript_window_for_selection(transcript, &selection),
                selection_focus_excerpt(
                    transcript,
                    &selection.segment_ids,
                    Some(&selection.selected_text),
                ),
                session_id,
                snapshot,
                storage,
                settings,
                llm_provider,
                policy,
                true,
            )
            .await?;
            if generated {
                policy.mark_response_sent(Utc::now());
            }
        }
        ipc_schema::UserAction::SummariseLastMinute => {
            let generated = maybe_generate_response(
                AssistantSurface::Manual,
                ResponseMode::SummariseRecent,
                "summary",
                context_engine::recent_transcript_window(transcript, 60),
                None,
                session_id,
                snapshot,
                storage,
                settings,
                llm_provider,
                policy,
                true,
            )
            .await?;
            if generated {
                policy.mark_response_sent(Utc::now());
            }
        }
        ipc_schema::UserAction::SummariseSelection(selection) => {
            let generated = maybe_generate_response(
                AssistantSurface::Manual,
                ResponseMode::SummariseRecent,
                "summary",
                transcript_window_for_selection(transcript, &selection),
                selection_focus_excerpt(
                    transcript,
                    &selection.segment_ids,
                    Some(&selection.selected_text),
                ),
                session_id,
                snapshot,
                storage,
                settings,
                llm_provider,
                policy,
                true,
            )
            .await?;
            if generated {
                policy.mark_response_sent(Utc::now());
            }
        }
        ipc_schema::UserAction::CommentCurrentTopic => {
            let generated = maybe_generate_response(
                AssistantSurface::Manual,
                ResponseMode::Commentary,
                "commentary",
                transcript.last_n_segments(6),
                None,
                session_id,
                snapshot,
                storage,
                settings,
                llm_provider,
                policy,
                true,
            )
            .await?;
            if generated {
                policy.mark_response_sent(Utc::now());
            }
        }
        ipc_schema::UserAction::CommentSelection(selection) => {
            let generated = maybe_generate_response(
                AssistantSurface::Manual,
                ResponseMode::Commentary,
                "commentary",
                transcript_window_for_selection(transcript, &selection),
                selection_focus_excerpt(
                    transcript,
                    &selection.segment_ids,
                    Some(&selection.selected_text),
                ),
                session_id,
                snapshot,
                storage,
                settings,
                llm_provider,
                policy,
                true,
            )
            .await?;
            if generated {
                policy.mark_response_sent(Utc::now());
            }
        }
        ipc_schema::UserAction::ClearCurrentView => {
            *transcript = TranscriptState::default();
            let mut locked = snapshot.write().await;
            locked.transcript = TranscriptSnapshot { partial_text: None, segments: Vec::new() };
            locked.detected_question = None;
            locked.manual_assistant = None;
            locked.automatic_assistant = None;
            locked.recent_errors.clear();
        }
    }

    sync_upload_state(snapshot, upload_gate).await;
    storage.append_audit_event(Some(session_id), &audit_event).await?;
    Ok(())
}

pub(crate) async fn maybe_auto_generate_assistant(
    committed_segment: &TranscriptSegment,
    session_id: Uuid,
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    storage: &Storage,
    settings: &Arc<RwLock<AppSettingsDto>>,
    llm_provider: &(dyn LlmProvider + Send + Sync),
    transcript: &mut TranscriptState,
    policy: &mut PolicyState,
) -> Result<()> {
    let now = Utc::now();

    if is_question_candidate(&committed_segment.text)
        && policy.should_auto_answer_question(committed_segment.id, now)
    {
        let generated = maybe_generate_response(
            AssistantSurface::Automatic,
            ResponseMode::AnswerQuestion,
            "answer",
            transcript_window_for_segment_ids(transcript, &[committed_segment.id]),
            Some(committed_segment.text.clone()),
            session_id,
            snapshot,
            storage,
            settings,
            llm_provider,
            policy,
            false,
        )
        .await?;

        if generated {
            policy.mark_auto_question_answered(committed_segment.id, now);
        }
    }

    Ok(())
}

pub(crate) async fn refresh_snapshot(
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    transcript: &TranscriptState,
) {
    let mut locked = snapshot.write().await;
    locked.transcript = TranscriptSnapshot {
        partial_text: transcript.partial_text().map(ToOwned::to_owned),
        segments: transcript.last_n_segments(MAX_HEALTH_SEGMENTS).into_iter().map(to_dto).collect(),
    };
    locked.detected_question = transcript.last_question_candidate().map(to_dto);
}

pub(crate) fn selection_focus_excerpt(
    transcript: &TranscriptState,
    segment_ids: &[Uuid],
    selected_text: Option<&str>,
) -> Option<String> {
    let explicit = selected_text.map(str::trim).filter(|text| !text.is_empty());
    if let Some(explicit) = explicit {
        return Some(explicit.to_string());
    }

    let mut segments = transcript_window_for_segment_ids(transcript, segment_ids);
    if segments.len() == 1 {
        return segments.pop().map(|segment| segment.text);
    }

    None
}

pub(crate) fn transcript_window_for_segment_ids(
    transcript: &TranscriptState,
    segment_ids: &[Uuid],
) -> Vec<TranscriptSegment> {
    let committed = transcript.segments();
    if committed.is_empty() || segment_ids.is_empty() {
        return Vec::new();
    }

    let mut included_indices = BTreeSet::new();
    for (index, segment) in committed.iter().enumerate() {
        if segment_ids.contains(&segment.id) {
            let start = index.saturating_sub(1);
            let end = (index + 1).min(committed.len().saturating_sub(1));
            for context_index in start..=end {
                included_indices.insert(context_index);
            }
        }
    }

    included_indices.into_iter().filter_map(|index| committed.get(index).cloned()).collect()
}

fn transcript_window_for_selection(
    transcript: &TranscriptState,
    selection: &TranscriptSelectionPayload,
) -> Vec<TranscriptSegment> {
    transcript_window_for_segment_ids(transcript, &selection.segment_ids)
}

fn to_dto(segment: TranscriptSegment) -> TranscriptSegmentDto {
    let is_question = is_question_candidate(&segment.text);
    TranscriptSegmentDto {
        id: segment.id,
        session_id: segment.session_id,
        start_ms: segment.start_ms,
        end_ms: segment.end_ms,
        text: segment.text,
        source: segment.source,
        created_at: segment.created_at,
        is_question_candidate: is_question,
    }
}

async fn maybe_generate_response(
    surface: AssistantSurface,
    mode: ResponseMode,
    kind: &str,
    window: Vec<TranscriptSegment>,
    focus_excerpt: Option<String>,
    session_id: Uuid,
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    storage: &Storage,
    settings: &Arc<RwLock<AppSettingsDto>>,
    llm_provider: &(dyn LlmProvider + Send + Sync),
    policy: &mut PolicyState,
    manual_trigger: bool,
) -> Result<bool> {
    let question_text = assistant_question_text(kind, focus_excerpt.as_deref(), &window);

    if window.is_empty() {
        if manual_trigger {
            let notice = AssistantOutput {
                kind: AssistantKind::Notice,
                question_text,
                content: "No recent transcript is available yet.".to_string(),
                confidence: Some(1.0),
                created_at: assistant_timestamp(),
                reused_from_history: false,
                source_model: None,
            };
            set_assistant_output(snapshot, surface, Some(notice)).await;
        }
        return Ok(false);
    }

    if manual_trigger {
        if !policy.can_generate_manual_response() {
            let notice = AssistantOutput {
                kind: AssistantKind::Notice,
                question_text,
                content: "Cloud processing is paused. Resume cloud processing to generate assistant output.".to_string(),
                confidence: Some(1.0),
                created_at: assistant_timestamp(),
                reused_from_history: false,
                source_model: None,
            };
            set_assistant_output(snapshot, surface, Some(notice)).await;
            return Ok(false);
        }
    } else if !policy.can_generate_automatic_response(Utc::now()) {
        return Ok(false);
    }

    snapshot.write().await.cloud_state = CloudState::LlmActive;
    let request_identity = build_request_identity(kind, &window, focus_excerpt.as_deref());
    let assistant_context = build_assistant_context(settings, storage, focus_excerpt).await?;
    let model = {
        let settings = settings.read().await;
        let model = settings.openai_model.trim();
        if model.is_empty() { default_openai_model() } else { model.to_string() }
    };

    if let Some(identity) = &request_identity {
        if let Some(reused) = storage
            .find_reusable_assistant_event(&model, &identity.request_kind, &identity.request_key)
            .await?
        {
            let assistant = AssistantOutput {
                kind: assistant_kind_from_str(kind),
                question_text: question_text.clone(),
                content: reused.content.clone(),
                confidence: Some(reused.confidence),
                created_at: assistant_timestamp(),
                reused_from_history: true,
                source_model: Some(model.clone()),
            };

            {
                let mut locked = snapshot.write().await;
                set_assistant_output_locked(&mut locked, surface, Some(assistant));
                locked.cloud_state = effective_cloud_state(&locked);
            }

            storage
                .insert_assistant_event(
                    session_id,
                    assistant_event_kind(kind, surface),
                    &reused.content,
                    reused.confidence,
                    &model,
                    &identity.request_kind,
                    &identity.request_key,
                    &identity.request_text,
                    Some(reused.id),
                    true,
                )
                .await?;

            return Ok(true);
        }
    }

    let response = llm_provider.respond(&model, mode, &window, &assistant_context).await?;

    if !response.should_respond {
        if manual_trigger {
            let notice = AssistantOutput {
                kind: AssistantKind::Notice,
                question_text,
                content: if response.answer.trim().is_empty() {
                    "No useful assistant response is available yet.".to_string()
                } else {
                    response.answer.clone()
                },
                confidence: Some(response.confidence),
                created_at: assistant_timestamp(),
                reused_from_history: false,
                source_model: Some(model.clone()),
            };
            let mut locked = snapshot.write().await;
            set_assistant_output_locked(&mut locked, surface, Some(notice));
            locked.cloud_state = effective_cloud_state(&locked);
        }
        return Ok(false);
    }

    let assistant = AssistantOutput {
        kind: assistant_kind_from_str(kind),
        question_text,
        content: response.answer.clone(),
        confidence: Some(response.confidence),
        created_at: assistant_timestamp(),
        reused_from_history: false,
        source_model: Some(model.clone()),
    };

    {
        let mut locked = snapshot.write().await;
        set_assistant_output_locked(&mut locked, surface, Some(assistant.clone()));
        locked.cloud_state = effective_cloud_state(&locked);
    }

    if let Some(identity) = &request_identity {
        storage
            .insert_assistant_event(
                session_id,
                assistant_event_kind(kind, surface),
                &response.answer,
                response.confidence,
                &model,
                &identity.request_kind,
                &identity.request_key,
                &identity.request_text,
                None,
                false,
            )
            .await?;
    }

    Ok(true)
}

fn assistant_kind_from_str(kind: &str) -> AssistantKind {
    match kind {
        "answer" => AssistantKind::Answer,
        "summary" => AssistantKind::Summary,
        "commentary" => AssistantKind::Commentary,
        _ => AssistantKind::Answer,
    }
}

fn assistant_event_kind(kind: &str, surface: AssistantSurface) -> &'static str {
    match (surface, kind) {
        (AssistantSurface::Manual, "answer") => "manual_answer",
        (AssistantSurface::Manual, "summary") => "manual_summary",
        (AssistantSurface::Manual, "commentary") => "manual_commentary",
        (AssistantSurface::Automatic, "answer") => "automatic_answer",
        (AssistantSurface::Automatic, "summary") => "automatic_summary",
        (AssistantSurface::Automatic, "commentary") => "automatic_commentary",
        (AssistantSurface::Manual, _) => "manual_answer",
        (AssistantSurface::Automatic, _) => "automatic_answer",
    }
}

fn assistant_question_text(
    kind: &str,
    focus_excerpt: Option<&str>,
    window: &[TranscriptSegment],
) -> Option<String> {
    if kind != "answer" {
        return focus_excerpt
            .map(normalize_display_text)
            .filter(|text| !text.is_empty());
    }

    focus_excerpt
        .map(normalize_display_text)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            window
                .iter()
                .rev()
                .find(|segment| is_question_candidate(&segment.text))
                .map(|segment| normalize_display_text(&segment.text))
        })
        .or_else(|| window.last().map(|segment| normalize_display_text(&segment.text)))
        .filter(|text| !text.is_empty())
}

async fn set_assistant_output(
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    surface: AssistantSurface,
    output: Option<AssistantOutput>,
) {
    let mut locked = snapshot.write().await;
    set_assistant_output_locked(&mut locked, surface, output);
}

fn set_assistant_output_locked(
    snapshot: &mut BackendStatusSnapshot,
    surface: AssistantSurface,
    output: Option<AssistantOutput>,
) {
    match surface {
        AssistantSurface::Manual => snapshot.manual_assistant = output,
        AssistantSurface::Automatic => snapshot.automatic_assistant = output,
    }
}

pub(crate) fn build_request_identity(
    kind: &str,
    window: &[TranscriptSegment],
    focus_excerpt: Option<&str>,
) -> Option<AssistantRequestIdentity> {
    if window.is_empty() {
        return None;
    }

    let request_text = match kind {
        "answer" => focus_excerpt
            .map(normalize_display_text)
            .filter(|text| !text.is_empty())
            .or_else(|| {
                window
                    .iter()
                    .rev()
                    .find(|segment| segment.text.contains('?'))
                    .map(|segment| normalize_display_text(&segment.text))
            })
            .unwrap_or_else(|| normalize_display_text(&window[window.len() - 1].text)),
        "summary" | "commentary" => focus_excerpt
            .map(normalize_display_text)
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| {
                normalize_display_text(
                    &window.iter().map(|segment| segment.text.as_str()).collect::<Vec<_>>().join(" "),
                )
            }),
        _ => normalize_display_text(
            &window.iter().map(|segment| segment.text.as_str()).collect::<Vec<_>>().join(" "),
        ),
    };

    let request_key = normalize_request_key(&request_text);
    if request_key.is_empty() {
        return None;
    }

    Some(AssistantRequestIdentity {
        request_kind: kind.to_string(),
        request_key,
        request_text,
    })
}

fn normalize_display_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_string()
}

fn normalize_request_key(text: &str) -> String {
    normalize_display_text(text)
        .chars()
        .map(|ch| match ch {
            '’' | '‘' => '\'',
            '“' | '”' => '"',
            '–' | '—' => '-',
            _ => ch,
        })
        .collect::<String>()
        .to_lowercase()
}

async fn build_assistant_context(
    settings: &Arc<RwLock<AppSettingsDto>>,
    storage: &Storage,
    focus_excerpt: Option<String>,
) -> Result<AssistantContextInput> {
    let instruction = {
        let settings = settings.read().await;
        if settings.assistant_instruction.trim().is_empty() {
            default_assistant_instruction()
        } else {
            settings.assistant_instruction.clone()
        }
    };

    let priming_documents = storage
        .list_priming_document_records()
        .await?
        .into_iter()
        .map(|document| PrimingDocumentInput {
            file_name: document.file_name,
            text: document.extracted_text,
        })
        .collect();

    Ok(AssistantContextInput { instruction, priming_documents, focus_excerpt })
}
