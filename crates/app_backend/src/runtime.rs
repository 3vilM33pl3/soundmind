use std::sync::Arc;

use anyhow::Result;
use ipc_schema::{AppSettingsDto, BackendStatusSnapshot, CaptureState, CloudState, UserAction};
use llm_core::LlmProviderRegistry;
use llm_ollama::{OllamaConfig, OllamaReasoner};
use llm_openai::{OpenAiConfig, OpenAiReasoner};
use policy_engine::PolicyState;
use storage_sqlite::Storage;
use tokio::sync::{RwLock, mpsc};
use tracing::{info, warn};
use transcript_core::{TranscriptSegment, TranscriptState};
use uuid::Uuid;

use app_core::{CaptureSection, RootConfig};
use audio_capture::{
    AudioSource, CaptureConfig, CaptureEvent, MockAudioSource, ParecMonitorAudioSource,
};
use audio_pipeline::{AudioChunk, AudioPipeline, AudioPipelineConfig};
use stt_core::Transcriber;
use stt_scribe::{MockTranscriber, ScribeRealtimeConfig, ScribeRealtimeTranscriber};

use crate::assistant::{handle_action, maybe_auto_generate_assistant, refresh_snapshot};

#[derive(Debug, Clone)]
pub(crate) struct UploadGate {
    pub(crate) speech_hold_ms: u64,
    pub(crate) idle_timeout_ms: u64,
    pub(crate) first_chunk_start_ms: Option<u64>,
    pub(crate) last_speech_end_ms: Option<u64>,
    pub(crate) auto_paused: bool,
    pub(crate) upload_active: bool,
}

impl UploadGate {
    pub(crate) fn new(config: &CaptureSection) -> Self {
        Self {
            speech_hold_ms: config.speech_hold_ms,
            idle_timeout_ms: config.idle_timeout_ms,
            first_chunk_start_ms: None,
            last_speech_end_ms: None,
            auto_paused: false,
            upload_active: false,
        }
    }

    pub(crate) fn mark_privacy_paused(&mut self) {
        self.upload_active = false;
    }

    pub(crate) fn mark_manual_cloud_paused(&mut self) {
        self.upload_active = false;
        self.auto_paused = false;
    }

    pub(crate) fn evaluate(&mut self, chunk: &AudioChunk, manual_cloud_paused: bool) -> bool {
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

pub(crate) async fn run_runtime(
    config: RootConfig,
    snapshot: Arc<RwLock<BackendStatusSnapshot>>,
    storage: Arc<Storage>,
    settings: Arc<RwLock<AppSettingsDto>>,
    llm_registry: LlmProviderRegistry,
    mut action_rx: mpsc::Receiver<UserAction>,
) -> Result<()> {
    let session_id = Uuid::new_v4();
    let runtime_settings = settings.read().await.clone();

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
                    &llm_registry,
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
                                        &llm_registry,
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

pub(crate) fn build_llm_registry(config: &RootConfig) -> LlmProviderRegistry {
    let openai = Arc::new(OpenAiReasoner::new(OpenAiConfig {
        api_key: std::env::var("OPENAI_API_KEY").ok(),
        enabled: config.providers.openai.enabled,
    }));
    let ollama = Arc::new(OllamaReasoner::new(OllamaConfig {
        base_url: config.providers.ollama.base_url.clone(),
        enabled: config.providers.ollama.enabled,
        default_model: config.providers.ollama.model.clone(),
    }));

    let mut providers: Vec<Arc<dyn llm_core::LlmProvider + Send + Sync>> = Vec::new();
    providers.push(openai);
    providers.push(ollama);
    LlmProviderRegistry::new(providers)
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
                warn!("ELEVENLABS_API_KEY is missing; using mock transcriber");
                push_stt_error(
                    snapshot,
                    "ELEVENLABS_API_KEY is missing; using mock transcriber".to_string(),
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
    source: &str,
    config: CaptureConfig,
    tx: mpsc::Sender<CaptureEvent>,
    snapshot: Arc<RwLock<BackendStatusSnapshot>>,
) {
    let source_name = source.to_string();
    tokio::spawn(async move {
        let mut source: Box<dyn AudioSource + Send> = match source_name.as_str() {
            "parec" => Box::new(ParecMonitorAudioSource::new(config)),
            _ => Box::new(MockAudioSource::new(config)),
        };

        if let Err(error) = source.run(tx).await {
            push_error(&snapshot, format!("capture source failed: {error}")).await;
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
    locked.stt_status = Some(message.clone());
    append_recent_error(&mut locked.recent_errors, message);
}

async fn push_recovering(snapshot: &Arc<RwLock<BackendStatusSnapshot>>, message: String) {
    let mut locked = snapshot.write().await;
    locked.capture_state = CaptureState::Capturing;
    append_recent_error(&mut locked.recent_errors, format!("recovering: {message}"));
}

fn append_recent_error(recent_errors: &mut Vec<String>, message: String) {
    if recent_errors.last().map(|last| last == &message).unwrap_or(false) {
        return;
    }

    recent_errors.push(message);
    if recent_errors.len() > 8 {
        let overflow = recent_errors.len() - 8;
        recent_errors.drain(0..overflow);
    }
}

pub(crate) async fn sync_upload_state(
    snapshot: &Arc<RwLock<BackendStatusSnapshot>>,
    upload_gate: &UploadGate,
) {
    let mut locked = snapshot.write().await;
    locked.audio_upload_active = upload_gate.upload_active;
    locked.cloud_auto_pause = upload_gate.auto_paused;
    locked.cloud_state = effective_cloud_state(&locked);
    locked.stt_status = Some(effective_stt_status(&locked));
}

pub(crate) fn effective_cloud_state(snapshot: &BackendStatusSnapshot) -> CloudState {
    if snapshot.cloud_pause || snapshot.cloud_auto_pause {
        CloudState::Paused
    } else if snapshot.audio_upload_active {
        CloudState::SttActive
    } else if snapshot.stt_provider.is_some() {
        CloudState::SttActive
    } else {
        CloudState::Off
    }
}

pub(crate) fn effective_stt_status(snapshot: &BackendStatusSnapshot) -> String {
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
