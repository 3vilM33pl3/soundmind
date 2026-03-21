use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use audio_capture::{
    AudioSource, CaptureConfig, CaptureEvent, MockAudioSource, ParecMonitorAudioSource,
};
use audio_pipeline::{AudioPipeline, AudioPipelineConfig};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use context_engine::{last_question_candidate, recent_transcript_window};
use dotenvy::from_filename_override;
use ipc_schema::{
    AppMode, AppSettingsDto, AssistantKind, AssistantOutput, BackendStatusSnapshot, CaptureState,
    CloudState, SessionDetailDto, SessionSummaryDto, TranscriptSegmentDto, TranscriptSnapshot,
    UserAction,
};
use llm_openai::{OpenAiConfig, OpenAiReasoner, ResponseMode, assistant_timestamp};
use policy_engine::PolicyState;
use serde::Deserialize;
use storage_sqlite::Storage;
use stt_scribe::{MockTranscriber, ScribeRealtimeConfig, ScribeRealtimeTranscriber, Transcriber};
use tokio::sync::{RwLock, mpsc};
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use transcript_core::{TranscriptSegment, TranscriptState};
use uuid::Uuid;

const MAX_HEALTH_SEGMENTS: usize = 48;

#[derive(Debug, Clone, Deserialize)]
struct RootConfig {
    app: AppSection,
    capture: CaptureSection,
    storage: StorageSection,
    providers: ProviderSection,
}

#[derive(Debug, Clone, Deserialize)]
struct AppSection {
    mode: AppMode,
    auto_start: bool,
    http_bind: String,
    simulate_transcriber: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureSection {
    source: String,
    frame_ms: u64,
    sample_rate_hz: u32,
    channels: u16,
    silence_threshold: f32,
    chunk_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct StorageSection {
    database_path: String,
    retention_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderSection {
    openai: ProviderConfig,
    elevenlabs: ProviderConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderConfig {
    enabled: bool,
    model: String,
}

#[derive(Clone)]
struct AppState {
    snapshot: Arc<RwLock<BackendStatusSnapshot>>,
    action_tx: mpsc::Sender<UserAction>,
    storage: Arc<Storage>,
    settings: Arc<RwLock<AppSettingsDto>>,
}

#[derive(Debug, Clone, Deserialize)]
struct SessionExportQuery {
    format: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,app_backend=debug".to_string()),
        )
        .init();
    load_keys_env();

    let config = load_config()?;
    let database_url = sqlite_url(&config.storage.database_path);
    let storage = Arc::new(Storage::connect(&database_url).await?);
    let settings = Arc::new(RwLock::new(load_or_initialize_settings(&storage, &config).await?));
    let initial_settings = settings.read().await.clone();
    let snapshot = Arc::new(RwLock::new(BackendStatusSnapshot {
        mode: initial_settings.default_mode,
        capture_state: if config.app.auto_start {
            CaptureState::Capturing
        } else {
            CaptureState::Paused
        },
        cloud_state: CloudState::Off,
        privacy_pause: !config.app.auto_start,
        ..BackendStatusSnapshot::default()
    }));

    let (action_tx, action_rx) = mpsc::channel(64);
    let app_state = AppState {
        snapshot: Arc::clone(&snapshot),
        action_tx: action_tx.clone(),
        storage: Arc::clone(&storage),
        settings: Arc::clone(&settings),
    };

    let config_for_runtime = config.clone();
    let runtime_snapshot = Arc::clone(&snapshot);
    let runtime_storage = Arc::clone(&storage);
    let runtime_settings = Arc::clone(&settings);
    tokio::spawn(async move {
        if let Err(error) =
            run_runtime(
                config_for_runtime,
                runtime_snapshot,
                runtime_storage,
                runtime_settings,
                action_rx,
            )
            .await
        {
            error!(?error, "runtime exited with an error");
        }
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/actions", post(post_action))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/sessions", get(get_sessions))
        .route("/sessions/purge", post(post_purge_sessions))
        .route("/sessions/{session_id}", get(get_session_detail).delete(delete_session))
        .route("/sessions/{session_id}/export", get(get_session_export))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let address: SocketAddr = config.app.http_bind.parse().context("invalid http_bind")?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(address = %address, "backend API listening");

    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;

    Ok(())
}

async fn run_runtime(
    config: RootConfig,
    snapshot: Arc<RwLock<BackendStatusSnapshot>>,
    storage: Arc<Storage>,
    settings: Arc<RwLock<AppSettingsDto>>,
    mut action_rx: mpsc::Receiver<UserAction>,
) -> Result<()> {
    let session_id = Uuid::new_v4();
    let runtime_settings = settings.read().await.clone();
    let openai = OpenAiReasoner::new(OpenAiConfig {
        api_key: std::env::var("OPENAI_API_KEY").ok(),
        model: std::env::var("OPENAI_MODEL").unwrap_or(config.providers.openai.model.clone()),
        enabled: config.providers.openai.enabled,
    });

    let mut policy = PolicyState {
        mode: runtime_settings.default_mode,
        cloud_paused: !runtime_settings.auto_start_cloud,
        ..PolicyState::default()
    };
    let mut transcript = TranscriptState::default();
    let (mut transcriber, stt_provider) = build_transcriber(&config, &snapshot).await?;

    let capture_config = CaptureConfig {
        frame_ms: config.capture.frame_ms,
        sample_rate_hz: config.capture.sample_rate_hz,
        channels: config.capture.channels,
    };
    let mut pipeline = AudioPipeline::new(
        AudioPipelineConfig {
            silence_threshold: config.capture.silence_threshold,
            chunk_ms: config.capture.chunk_ms,
        },
        config.capture.frame_ms,
    );

    let (capture_tx, mut capture_rx) = mpsc::channel(256);
    spawn_capture_source(
        config.capture.source.as_str(),
        capture_config,
        capture_tx,
        Arc::clone(&snapshot),
    );

    storage
        .start_session(
            session_id,
            "pending-device",
            &format!("{:?}", runtime_settings.default_mode),
        )
        .await?;
    storage.append_audit_event(Some(session_id), "session_started").await?;

    {
        let mut locked = snapshot.write().await;
        locked.session_id = Some(session_id);
        locked.stt_provider = Some(stt_provider);
        locked.cloud_state = if config.providers.openai.enabled || config.providers.elevenlabs.enabled
        {
            CloudState::SttActive
        } else {
            CloudState::Off
        };
    }
    drain_transcriber_events(
        &snapshot,
        &storage,
        &settings,
        session_id,
        &mut transcript,
        transcriber.as_mut(),
    )
    .await?;

    loop {
        tokio::select! {
            maybe_action = action_rx.recv() => {
                let Some(action) = maybe_action else {
                    break;
                };
                handle_action(
                    action,
                    session_id,
                    &snapshot,
                    &storage,
                    &settings,
                    &openai,
                    &mut transcript,
                    &mut policy,
                ).await?;
            }
            maybe_event = capture_rx.recv() => {
                let Some(event) = maybe_event else {
                    break;
                };

                match event {
                    CaptureEvent::DeviceChanged(device) => {
                        {
                            let mut locked = snapshot.write().await;
                            locked.current_sink = Some(device.sink_name.clone());
                            locked.current_monitor_source = Some(device.monitor_source.clone());
                            locked.capture_state = CaptureState::Capturing;
                        }
                        storage
                            .append_audit_event(
                                Some(session_id),
                                &format!("capture_device_changed:{}", device.monitor_source),
                            )
                            .await?;
                    }
                    CaptureEvent::Frames(frame) => {
                        if snapshot.read().await.privacy_pause {
                            continue;
                        }

                        if let Some(chunk) = pipeline.push_frame(frame) {
                            transcriber.push_audio(chunk).await?;
                            drain_transcriber_events(
                                &snapshot,
                                &storage,
                                &settings,
                                session_id,
                                &mut transcript,
                                transcriber.as_mut(),
                            )
                            .await?;
                        }
                    }
                    CaptureEvent::Error(message) => {
                        push_error(&snapshot, message).await;
                    }
                    CaptureEvent::Recovering(message) => {
                        push_recovering(&snapshot, message.clone()).await;
                        storage
                            .append_audit_event(Some(session_id), &format!("capture_recovering:{message}"))
                            .await?;
                    }
                    CaptureEvent::Ended => {
                        break;
                    }
                }
            }
        }
    }

    transcriber.stop().await.ok();
    storage.end_session(session_id).await?;
    storage.append_audit_event(Some(session_id), "session_stopped").await?;
    Ok(())
}

async fn drain_transcriber_events(
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    storage: &Storage,
    settings: &Arc<RwLock<AppSettingsDto>>,
    session_id: Uuid,
    transcript: &mut TranscriptState,
    transcriber: &mut dyn Transcriber,
) -> Result<()> {
    while let Some(event) = transcriber.try_recv_event() {
        match &event {
            stt_scribe::TranscriberEvent::Health(health) => {
                let mut locked = snapshot.write().await;
                locked.cloud_state = CloudState::SttActive;
                locked.stt_status = Some(health.message.clone());
            }
            stt_scribe::TranscriberEvent::Error(message) => {
                push_stt_error(snapshot, message.clone()).await;
            }
            stt_scribe::TranscriberEvent::PartialTranscript(_)
            | stt_scribe::TranscriberEvent::FinalTranscript(_) => {}
        }

        if let Some(segment) = transcript.apply_event(session_id, event) {
            if settings.read().await.transcript_storage_enabled {
                storage.insert_transcript_segment(&segment).await?;
            }
            refresh_snapshot(snapshot, transcript).await;
        } else {
            refresh_snapshot(snapshot, transcript).await;
        }
    }

    Ok(())
}

async fn build_transcriber(
    config: &RootConfig,
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
) -> Result<(Box<dyn Transcriber>, String)> {
    if !config.app.simulate_transcriber && config.providers.elevenlabs.enabled {
        match std::env::var("ELEVENLABS_API_KEY") {
            Ok(api_key) if !api_key.trim().is_empty() => {
                let mut transcriber = ScribeRealtimeTranscriber::new(ScribeRealtimeConfig {
                    api_key,
                    model_id: config.providers.elevenlabs.model.clone(),
                    sample_rate_hz: config.capture.sample_rate_hz,
                    language_code: Some("en".to_string()),
                    include_timestamps: true,
                    enable_logging: true,
                });

                match transcriber.start().await {
                    Ok(()) => {
                        info!(model = %config.providers.elevenlabs.model, "using ElevenLabs realtime transcriber");
                        return Ok((
                            Box::new(transcriber),
                            "elevenlabs_scribe_realtime".to_string(),
                        ));
                    }
                    Err(error) => {
                        warn!(?error, "failed to start ElevenLabs realtime transcriber; falling back to mock");
                        push_stt_error(
                            snapshot,
                            format!("ElevenLabs startup failed; falling back to mock: {error}"),
                        )
                        .await;
                    }
                }
            }
            _ => {
                push_stt_error(
                    snapshot,
                    "ELEVENLABS_API_KEY is missing; falling back to mock transcriber".to_string(),
                )
                .await;
            }
        }
    }

    let mut transcriber = MockTranscriber::new();
    transcriber.start().await?;
    Ok((Box::new(transcriber), "mock_scribe".to_string()))
}

fn spawn_capture_source(
    source_name: &str,
    capture_config: CaptureConfig,
    capture_tx: mpsc::Sender<CaptureEvent>,
    snapshot: Arc<RwLock<BackendStatusSnapshot>>,
) {
    let source_name = source_name.to_string();
    tokio::spawn(async move {
        let result = match source_name.as_str() {
            "mock" => {
                let mut source = MockAudioSource::new(capture_config);
                source.run(capture_tx).await
            }
            _ => {
                let mut source = ParecMonitorAudioSource::new(capture_config);
                source.run(capture_tx).await
            }
        };

        if let Err(error) = result {
            warn!(?error, "capture source stopped");
            let mut locked = snapshot.write().await;
            locked.capture_state = CaptureState::Error;
            locked.recent_errors.push(error.to_string());
        }
    });
}

async fn handle_action(
    action: UserAction,
    session_id: Uuid,
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    storage: &Storage,
    settings: &Arc<RwLock<AppSettingsDto>>,
    openai: &OpenAiReasoner,
    transcript: &mut TranscriptState,
    policy: &mut PolicyState,
) -> Result<()> {
    match action {
        UserAction::Start => {
            let mut locked = snapshot.write().await;
            locked.privacy_pause = false;
            locked.capture_state = CaptureState::Capturing;
        }
        UserAction::Stop => {
            let mut locked = snapshot.write().await;
            locked.privacy_pause = true;
            locked.capture_state = CaptureState::Paused;
        }
        UserAction::PauseCloud => {
            policy.cloud_paused = true;
            let mut locked = snapshot.write().await;
            locked.cloud_pause = true;
            locked.cloud_state = CloudState::Paused;
        }
        UserAction::ResumeCloud => {
            policy.cloud_paused = false;
            let mut locked = snapshot.write().await;
            locked.cloud_pause = false;
            locked.cloud_state = CloudState::SttActive;
        }
        UserAction::SetMode(mode) => {
            policy.mode = mode;
            snapshot.write().await.mode = mode;
            let mut runtime_settings = settings.write().await;
            runtime_settings.default_mode = mode;
            storage.save_settings(&runtime_settings).await?;
        }
        UserAction::AnswerLastQuestion => {
            let last_question = last_question_candidate(transcript)
                .map(|segment| vec![segment])
                .unwrap_or_else(|| recent_transcript_window(transcript, 60));
            maybe_generate_response(
                ResponseMode::AnswerQuestion,
                "answer",
                last_question,
                session_id,
                snapshot,
                storage,
                openai,
                policy,
            )
            .await?;
        }
        UserAction::SummariseLastMinute => {
            maybe_generate_response(
                ResponseMode::SummariseRecent,
                "summary",
                recent_transcript_window(transcript, 60),
                session_id,
                snapshot,
                storage,
                openai,
                policy,
            )
            .await?;
        }
        UserAction::CommentCurrentTopic => {
            maybe_generate_response(
                ResponseMode::Commentary,
                "commentary",
                transcript.last_n_segments(6),
                session_id,
                snapshot,
                storage,
                openai,
                policy,
            )
            .await?;
        }
    }

    storage.append_audit_event(Some(session_id), &format!("user_action:{action:?}")).await?;
    Ok(())
}

async fn maybe_generate_response(
    mode: ResponseMode,
    kind: &str,
    window: Vec<TranscriptSegment>,
    session_id: Uuid,
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    storage: &Storage,
    openai: &OpenAiReasoner,
    policy: &mut PolicyState,
) -> Result<()> {
    if window.is_empty() {
        let notice = AssistantOutput {
            kind: AssistantKind::Notice,
            content: "No recent transcript is available yet.".to_string(),
            confidence: Some(1.0),
            created_at: assistant_timestamp(),
        };
        snapshot.write().await.latest_assistant = Some(notice);
        return Ok(());
    }

    if !policy.can_generate_response(Utc::now()) {
        let notice = AssistantOutput {
            kind: AssistantKind::Notice,
            content: "Response suppressed by policy cooldown or cloud pause.".to_string(),
            confidence: Some(1.0),
            created_at: assistant_timestamp(),
        };
        snapshot.write().await.latest_assistant = Some(notice);
        return Ok(());
    }

    snapshot.write().await.cloud_state = CloudState::LlmActive;
    let response = openai.respond(mode, &window).await?;
    policy.mark_response_sent(Utc::now());

    let assistant = AssistantOutput {
        kind: match kind {
            "answer" => AssistantKind::Answer,
            "summary" => AssistantKind::Summary,
            "commentary" => AssistantKind::Commentary,
            _ => AssistantKind::Notice,
        },
        content: response.answer.clone(),
        confidence: Some(response.confidence),
        created_at: assistant_timestamp(),
    };

    {
        let mut locked = snapshot.write().await;
        locked.latest_assistant = Some(assistant.clone());
        locked.cloud_state =
            if locked.cloud_pause { CloudState::Paused } else { CloudState::SttActive };
    }

    storage.insert_assistant_event(session_id, kind, &response.answer, response.confidence).await?;

    Ok(())
}

async fn refresh_snapshot(
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    transcript: &TranscriptState,
) {
    let mut locked = snapshot.write().await;
    locked.transcript = TranscriptSnapshot {
        partial_text: transcript.partial_text().map(ToOwned::to_owned),
        segments: transcript
            .last_n_segments(MAX_HEALTH_SEGMENTS)
            .into_iter()
            .map(to_dto)
            .collect(),
    };
}

fn to_dto(segment: TranscriptSegment) -> TranscriptSegmentDto {
    TranscriptSegmentDto {
        id: segment.id,
        session_id: segment.session_id,
        start_ms: segment.start_ms,
        end_ms: segment.end_ms,
        text: segment.text,
        source: segment.source,
        created_at: segment.created_at,
    }
}

async fn push_error(snapshot: &Arc<RwLock<BackendStatusSnapshot>>, message: String) {
    let mut locked = snapshot.write().await;
    locked.capture_state = CaptureState::Error;
    locked.recent_errors.push(message);
    if locked.recent_errors.len() > 5 {
        let excess = locked.recent_errors.len() - 5;
        locked.recent_errors.drain(0..excess);
    }
}

async fn push_stt_error(snapshot: &Arc<RwLock<BackendStatusSnapshot>>, message: String) {
    let mut locked = snapshot.write().await;
    locked.cloud_state = CloudState::Error;
    locked.stt_status = Some(message.clone());
    locked.recent_errors.push(message);
    if locked.recent_errors.len() > 5 {
        let excess = locked.recent_errors.len() - 5;
        locked.recent_errors.drain(0..excess);
    }
}

async fn push_recovering(snapshot: &Arc<RwLock<BackendStatusSnapshot>>, message: String) {
    let mut locked = snapshot.write().await;
    if locked.capture_state == CaptureState::Error {
        locked.capture_state = CaptureState::Capturing;
    }
    locked.recent_errors.push(message);
    if locked.recent_errors.len() > 5 {
        let excess = locked.recent_errors.len() - 5;
        locked.recent_errors.drain(0..excess);
    }
}

async fn health(State(state): State<AppState>) -> Json<BackendStatusSnapshot> {
    Json(state.snapshot.read().await.clone())
}

async fn post_action(
    State(state): State<AppState>,
    Json(action): Json<UserAction>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    state
        .action_tx
        .send(action)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn get_settings(State(state): State<AppState>) -> Json<AppSettingsDto> {
    Json(state.settings.read().await.clone())
}

async fn put_settings(
    State(state): State<AppState>,
    Json(settings): Json<AppSettingsDto>,
) -> Result<Json<AppSettingsDto>, StatusCode> {
    state
        .storage
        .save_settings(&settings)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    *state.settings.write().await = settings.clone();
    Ok(Json(settings))
}

async fn get_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionSummaryDto>>, StatusCode> {
    let sessions = state
        .storage
        .list_sessions(100)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(sessions))
}

async fn get_session_detail(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<Uuid>,
) -> Result<Json<SessionDetailDto>, StatusCode> {
    let session = state
        .storage
        .get_session_detail(session_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    session.map(Json).ok_or(StatusCode::NOT_FOUND)
}

async fn delete_session(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if state.snapshot.read().await.session_id == Some(session_id) {
        return Err(StatusCode::CONFLICT);
    }
    state
        .storage
        .delete_session(session_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "deleted": session_id })))
}

async fn post_purge_sessions(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let settings = state.settings.read().await.clone();
    let deleted = state
        .storage
        .purge_sessions_older_than_days(settings.retention_days)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

async fn get_session_export(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<Uuid>,
    Query(query): Query<SessionExportQuery>,
) -> Result<Response, StatusCode> {
    let Some(session) = state
        .storage
        .get_session_detail(session_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::NOT_FOUND);
    };

    let format = query.format.as_deref().unwrap_or("json");
    match format {
        "markdown" | "md" => {
            let body = render_session_markdown(&session);
            Ok((
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/markdown; charset=utf-8"),
                )],
                body,
            )
                .into_response())
        }
        _ => Ok((
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            serde_json::to_string_pretty(&session).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
            .into_response()),
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn load_config() -> Result<RootConfig> {
    let path = resolve_config_path()?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut config: RootConfig =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))?;

    if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        if !api_key.trim().is_empty() {
            config.providers.openai.enabled = true;
        }
    }

    if let Ok(api_key) = std::env::var("ELEVENLABS_API_KEY") {
        if !api_key.trim().is_empty() {
            config.providers.elevenlabs.enabled = true;
            if config.app.simulate_transcriber {
                info!(
                    "ELEVENLABS_API_KEY is present; overriding simulate_transcriber=true to use the live provider by default"
                );
                config.app.simulate_transcriber = false;
            }
        }
    }

    if let Ok(model) = std::env::var("ELEVENLABS_MODEL") {
        if !model.trim().is_empty() {
            config.providers.elevenlabs.model = model;
        }
    } else if config.providers.elevenlabs.model == "scribe_v1" {
        info!(
            "Overriding legacy realtime STT model scribe_v1 with scribe_v2_realtime for the ElevenLabs realtime endpoint"
        );
        config.providers.elevenlabs.model = "scribe_v2_realtime".to_string();
    }

    info!(
        retention_days = if config.storage.retention_days == 0 {
            "infinite".to_string()
        } else {
            config.storage.retention_days.to_string()
        },
        simulate_transcriber = config.app.simulate_transcriber,
        "loaded configuration"
    );
    Ok(config)
}

fn load_keys_env() {
    for path in candidate_keys_env_paths() {
        if path.exists() {
            if let Err(error) = from_filename_override(&path) {
                warn!(path = %path.display(), ?error, "failed to load keys.env");
            } else {
                info!(path = %path.display(), "loaded provider keys");
            }
            break;
        }
    }
}

fn resolve_config_path() -> Result<PathBuf> {
    for path in candidate_config_paths() {
        if path.exists() {
            return Ok(path);
        }
    }

    let searched = candidate_config_paths()
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("no config file found; searched {searched}")
}

fn candidate_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(explicit) = std::env::var("SOUNDMIND_CONFIG") {
        if !explicit.trim().is_empty() {
            paths.push(PathBuf::from(explicit));
        }
    }
    paths.push(PathBuf::from("config.toml"));
    if let Some(config_dir) = soundmind_config_dir() {
        paths.push(config_dir.join("config.toml"));
    }
    paths.push(PathBuf::from("config.example.toml"));
    paths
}

fn candidate_keys_env_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(explicit) = std::env::var("SOUNDMIND_KEYS_ENV") {
        if !explicit.trim().is_empty() {
            paths.push(PathBuf::from(explicit));
        }
    }
    paths.push(PathBuf::from("keys.env"));
    if let Some(config_dir) = soundmind_config_dir() {
        paths.push(config_dir.join("keys.env"));
    }
    paths
}

fn soundmind_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join("soundmind"))
}

fn sqlite_url(path: &str) -> String {
    if path.starts_with("sqlite:") { path.to_string() } else { format!("sqlite://{path}") }
}

async fn load_or_initialize_settings(storage: &Storage, config: &RootConfig) -> Result<AppSettingsDto> {
    if let Some(settings) = storage.load_settings().await? {
        return Ok(settings);
    }

    let settings = AppSettingsDto {
        retention_days: config.storage.retention_days,
        transcript_storage_enabled: true,
        auto_start_cloud: config.app.auto_start,
        default_mode: config.app.mode,
    };
    storage.save_settings(&settings).await?;
    Ok(settings)
}

fn render_session_markdown(session: &SessionDetailDto) -> String {
    let mut markdown = String::new();
    markdown.push_str("# Soundmind Session Export\n\n");
    markdown.push_str(&format!("Session ID: `{}`\n", session.session.id));
    markdown.push_str(&format!("Started: {}\n", session.session.started_at.to_rfc3339()));
    if let Some(ended_at) = session.session.ended_at {
        markdown.push_str(&format!("Ended: {}\n", ended_at.to_rfc3339()));
    }
    markdown.push_str(&format!("Capture Device: {}\n", session.session.capture_device));
    markdown.push_str(&format!("Mode: {}\n\n", session.session.mode));
    markdown.push_str("## Transcript\n\n");
    for segment in &session.transcript_segments {
        markdown.push_str(&format!(
            "- [{}-{} ms] {}\n",
            segment.start_ms, segment.end_ms, segment.text
        ));
    }
    markdown.push_str("\n## Assistant Events\n\n");
    for event in &session.assistant_events {
        markdown.push_str(&format!(
            "- {} ({:.2}): {}\n",
            event.kind, event.confidence, event.content
        ));
    }
    markdown
}
