mod assistant;
mod http_api;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use app_core::{
    AppCoreState, CaptureSection, RootConfig, load_config, load_keys_env, sqlite_url,
};
use audio_capture::{
    AudioSource, CaptureConfig, CaptureEvent, MockAudioSource, ParecMonitorAudioSource,
};
use audio_pipeline::{AudioChunk, AudioPipeline, AudioPipelineConfig};
use ipc_schema::{
    AppSettingsDto, BackendStatusSnapshot, CaptureState, CloudState, UserAction,
};
use llm_openai::{OpenAiConfig, OpenAiReasoner};
use policy_engine::PolicyState;
use storage_sqlite::Storage;
use stt_core::Transcriber;
use stt_scribe::{MockTranscriber, ScribeRealtimeConfig, ScribeRealtimeTranscriber};
use tokio::sync::{RwLock, mpsc};
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use transcript_core::{TranscriptSegment, TranscriptState};
use uuid::Uuid;

use crate::assistant::{handle_action, maybe_auto_generate_assistant, refresh_snapshot};
#[cfg(test)]
use crate::assistant::{
    build_request_identity, selection_focus_excerpt, transcript_window_for_segment_ids,
};

const MAX_HEALTH_SEGMENTS: usize = 48;

#[derive(Debug, Clone)]
struct UploadGate {
    speech_hold_ms: u64,
    idle_timeout_ms: u64,
    first_chunk_start_ms: Option<u64>,
    last_speech_end_ms: Option<u64>,
    auto_paused: bool,
    upload_active: bool,
}

impl UploadGate {
    fn new(config: &CaptureSection) -> Self {
        Self {
            speech_hold_ms: config.speech_hold_ms,
            idle_timeout_ms: config.idle_timeout_ms,
            first_chunk_start_ms: None,
            last_speech_end_ms: None,
            auto_paused: false,
            upload_active: false,
        }
    }

    fn mark_privacy_paused(&mut self) {
        self.upload_active = false;
    }

    fn mark_manual_cloud_paused(&mut self) {
        self.upload_active = false;
        self.auto_paused = false;
    }

    fn evaluate(&mut self, chunk: &AudioChunk, manual_cloud_paused: bool) -> bool {
        self.first_chunk_start_ms.get_or_insert(chunk.start_ms);

        if manual_cloud_paused {
            self.mark_manual_cloud_paused();
            return false;
        }

        if chunk.speech_likely {
            self.last_speech_end_ms = Some(chunk.end_ms);
            self.auto_paused = false;
            self.upload_active = true;
            return true;
        }

        let within_speech_hold = self
            .last_speech_end_ms
            .map(|last_speech_end_ms| {
                chunk.start_ms.saturating_sub(last_speech_end_ms) <= self.speech_hold_ms
            })
            .unwrap_or(false);

        if within_speech_hold {
            self.upload_active = true;
            return true;
        }

        let idle_reference_ms =
            self.last_speech_end_ms.or(self.first_chunk_start_ms).unwrap_or(chunk.start_ms);
        self.auto_paused = chunk.end_ms.saturating_sub(idle_reference_ms) >= self.idle_timeout_ms;
        self.upload_active = false;
        false
    }
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

    let (action_tx, action_rx) = mpsc::channel(64);
    let app_state = AppCoreState::initialize(&config, Arc::clone(&storage), action_tx.clone()).await?;

    let config_for_runtime = config.clone();
    let runtime_snapshot = app_state.snapshot_handle();
    let runtime_storage = app_state.storage_handle();
    let runtime_settings = app_state.settings_handle();
    tokio::spawn(async move {
        if let Err(error) = run_runtime(
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

    let app = http_api::router(app_state).layer(CorsLayer::permissive());

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
        enabled: config.providers.openai.enabled,
    });

    let mut policy = PolicyState {
        mode: runtime_settings.default_mode,
        cloud_paused: !runtime_settings.auto_start_cloud,
        ..PolicyState::default()
    };
    let mut transcript = TranscriptState::default();
    let mut upload_gate = UploadGate::new(&config.capture);
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
        locked.cloud_pause = !runtime_settings.auto_start_cloud;
    }
    sync_upload_state(&snapshot, &upload_gate).await;
    let _ = drain_transcriber_events(
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
                    &mut upload_gate,
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
                        let (privacy_pause, cloud_pause) = {
                            let locked = snapshot.read().await;
                            (locked.privacy_pause, locked.cloud_pause)
                        };

                        if privacy_pause {
                            upload_gate.mark_privacy_paused();
                            sync_upload_state(&snapshot, &upload_gate).await;
                            continue;
                        }

                        if let Some(chunk) = pipeline.push_frame(frame) {
                            let was_auto_paused = upload_gate.auto_paused;
                            let should_send = upload_gate.evaluate(&chunk, cloud_pause);
                            sync_upload_state(&snapshot, &upload_gate).await;

                            if upload_gate.auto_paused != was_auto_paused {
                                let event = if upload_gate.auto_paused {
                                    "cloud_auto_paused:idle_timeout"
                                } else {
                                    "cloud_auto_resumed:speech_detected"
                                };
                                storage.append_audit_event(Some(session_id), event).await?;
                            }

                            if should_send {
                                transcriber.push_audio(chunk).await?;
                                let committed_segment = drain_transcriber_events(
                                    &snapshot,
                                    &storage,
                                    &settings,
                                    session_id,
                                    &mut transcript,
                                    transcriber.as_mut(),
                                )
                                .await?;

                                if let Some(committed_segment) = committed_segment {
                                    maybe_auto_generate_assistant(
                                        &committed_segment,
                                        session_id,
                                        &snapshot,
                                        &storage,
                                        &settings,
                                        &openai,
                                        &mut transcript,
                                        &mut policy,
                                    )
                                    .await?;
                                }
                            }
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
) -> Result<Option<TranscriptSegment>> {
    let mut latest_committed_segment = None;

    while let Some(event) = transcriber.try_recv_event() {
        match &event {
            stt_scribe::TranscriberEvent::Health(_) => {
                let mut locked = snapshot.write().await;
                locked.cloud_state = effective_cloud_state(&locked);
                locked.stt_status = Some(effective_stt_status(&locked));
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
            latest_committed_segment = Some(segment);
        } else {
            refresh_snapshot(snapshot, transcript).await;
        }
    }

    Ok(latest_committed_segment)
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
                        warn!(
                            ?error,
                            "failed to start ElevenLabs realtime transcriber; falling back to mock"
                        );
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
            append_recent_error(&mut locked.recent_errors, error.to_string());
        }
    });
}

async fn push_error(snapshot: &Arc<RwLock<BackendStatusSnapshot>>, message: String) {
    let mut locked = snapshot.write().await;
    locked.capture_state = CaptureState::Error;
    append_recent_error(&mut locked.recent_errors, message);
}

async fn push_stt_error(snapshot: &Arc<RwLock<BackendStatusSnapshot>>, message: String) {
    let mut locked = snapshot.write().await;
    locked.cloud_state = CloudState::Error;
    locked.audio_upload_active = false;
    locked.cloud_auto_pause = false;
    let trimmed = message.trim();
    if !trimmed.is_empty() {
        locked.stt_status = Some(trimmed.to_string());
    }
    append_recent_error(&mut locked.recent_errors, message);
}

async fn push_recovering(snapshot: &Arc<RwLock<BackendStatusSnapshot>>, message: String) {
    let mut locked = snapshot.write().await;
    if locked.capture_state == CaptureState::Error {
        locked.capture_state = CaptureState::Capturing;
    }
    append_recent_error(&mut locked.recent_errors, message);
}

fn append_recent_error(recent_errors: &mut Vec<String>, message: String) {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return;
    }

    recent_errors.push(trimmed.to_string());
    if recent_errors.len() > 5 {
        let excess = recent_errors.len() - 5;
        recent_errors.drain(0..excess);
    }
}

async fn sync_upload_state(
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    upload_gate: &UploadGate,
) {
    let mut locked = snapshot.write().await;
    if locked.cloud_state == CloudState::Error {
        locked.audio_upload_active = false;
        locked.cloud_auto_pause = false;
        return;
    }

    locked.audio_upload_active =
        !locked.privacy_pause && !locked.cloud_pause && upload_gate.upload_active;
    locked.cloud_auto_pause =
        !locked.privacy_pause && !locked.cloud_pause && upload_gate.auto_paused;
    locked.cloud_state = effective_cloud_state(&locked);
    locked.stt_status = Some(effective_stt_status(&locked));
}

fn effective_cloud_state(snapshot: &BackendStatusSnapshot) -> CloudState {
    if snapshot.stt_provider.is_none() {
        return CloudState::Off;
    }

    if snapshot.cloud_pause || snapshot.cloud_auto_pause {
        return CloudState::Paused;
    }

    CloudState::SttActive
}

fn effective_stt_status(snapshot: &BackendStatusSnapshot) -> String {
    if snapshot.privacy_pause {
        "local capture paused before cloud upload".to_string()
    } else if snapshot.cloud_pause {
        "cloud processing paused manually".to_string()
    } else if snapshot.cloud_auto_pause {
        "idle auto-pause active; waiting for speech to resume uploads".to_string()
    } else if snapshot.audio_upload_active {
        "speech detected; uploading audio to the STT provider".to_string()
    } else {
        "speech gate active; waiting for speech before upload".to_string()
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use stt_scribe::{FinalTranscript, TranscriberEvent};

    fn capture_section() -> CaptureSection {
        CaptureSection {
            source: "mock".to_string(),
            frame_ms: 20,
            sample_rate_hz: 16_000,
            channels: 1,
            silence_threshold: 0.008,
            chunk_ms: 200,
            speech_hold_ms: 1_200,
            idle_timeout_ms: 5_000,
        }
    }

    fn chunk(start_ms: u64, end_ms: u64, speech_likely: bool) -> AudioChunk {
        AudioChunk {
            start_ms,
            end_ms,
            samples: vec![0.0; 3200],
            energy: if speech_likely { 0.05 } else { 0.0 },
            speech_likely,
        }
    }

    #[test]
    fn upload_gate_skips_initial_silence_and_auto_pauses_after_idle_timeout() {
        let mut gate =
            UploadGate::new(&CaptureSection { idle_timeout_ms: 1_000, ..capture_section() });

        assert!(!gate.evaluate(&chunk(0, 200, false), false));
        assert!(!gate.upload_active);
        assert!(!gate.auto_paused);

        assert!(!gate.evaluate(&chunk(800, 1_000, false), false));
        assert!(gate.auto_paused);
        assert!(!gate.upload_active);
    }

    #[test]
    fn upload_gate_keeps_short_silence_after_speech_but_stops_long_idle_uploads() {
        let mut gate = UploadGate::new(&CaptureSection {
            speech_hold_ms: 300,
            idle_timeout_ms: 1_000,
            ..capture_section()
        });

        assert!(gate.evaluate(&chunk(0, 200, true), false));
        assert!(gate.upload_active);
        assert!(!gate.auto_paused);

        assert!(gate.evaluate(&chunk(250, 450, false), false));
        assert!(gate.upload_active);

        assert!(!gate.evaluate(&chunk(1_300, 1_500, false), false));
        assert!(gate.auto_paused);
        assert!(!gate.upload_active);
    }

    #[test]
    fn upload_gate_resumes_when_speech_returns_after_idle_auto_pause() {
        let mut gate =
            UploadGate::new(&CaptureSection { idle_timeout_ms: 1_000, ..capture_section() });

        assert!(!gate.evaluate(&chunk(0, 1_000, false), false));
        assert!(gate.auto_paused);

        assert!(gate.evaluate(&chunk(1_000, 1_200, true), false));
        assert!(gate.upload_active);
        assert!(!gate.auto_paused);
    }

    #[test]
    fn effective_status_prefers_manual_and_idle_pause_messages() {
        let mut snapshot = BackendStatusSnapshot {
            stt_provider: Some("elevenlabs_scribe_realtime".to_string()),
            ..BackendStatusSnapshot::default()
        };

        snapshot.cloud_pause = true;
        assert_eq!(effective_stt_status(&snapshot), "cloud processing paused manually");

        snapshot.cloud_pause = false;
        snapshot.cloud_auto_pause = true;
        assert_eq!(
            effective_stt_status(&snapshot),
            "idle auto-pause active; waiting for speech to resume uploads"
        );
    }

    fn transcript_with_segments(texts: &[&str]) -> TranscriptState {
        let mut transcript = TranscriptState::default();
        let session_id = Uuid::new_v4();

        for (index, text) in texts.iter().enumerate() {
            transcript.apply_event(
                session_id,
                TranscriberEvent::FinalTranscript(FinalTranscript {
                    start_ms: (index as u64) * 1_000,
                    end_ms: ((index as u64) + 1) * 1_000,
                    text: (*text).to_string(),
                    source: "test".to_string(),
                }),
            );
        }

        transcript
    }

    #[test]
    fn segment_window_includes_one_segment_of_context_on_each_side() {
        let transcript = transcript_with_segments(&["one", "two", "three", "four"]);
        let target_segment_id = transcript.segments()[1].id;

        let window = transcript_window_for_segment_ids(&transcript, &[target_segment_id]);
        let texts = window.into_iter().map(|segment| segment.text).collect::<Vec<_>>();

        assert_eq!(texts, vec!["one", "two", "three"]);
    }

    #[test]
    fn selection_focus_uses_selected_text_when_present() {
        let transcript = transcript_with_segments(&["Who are you?", "Tell me more"]);
        let focus = selection_focus_excerpt(
            &transcript,
            &[transcript.segments()[0].id],
            Some("Who are you"),
        );

        assert_eq!(focus.as_deref(), Some("Who are you"));
    }

    #[test]
    fn request_identity_normalizes_question_text_for_exact_reuse() {
        let window = transcript_with_segments(&["How  would you improve this — process?"]).segments().to_vec();
        let identity = build_request_identity("answer", &window, Some("How would you improve this - process?"))
            .expect("expected request identity");

        assert_eq!(identity.request_kind, "answer");
        assert_eq!(identity.request_text, "How would you improve this - process?");
        assert_eq!(identity.request_key, "how would you improve this - process?");
    }
}
