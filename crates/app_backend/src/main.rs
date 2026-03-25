mod assistant;
mod http_api;
mod runtime;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use app_core::{AppCoreState, load_config, load_keys_env, sqlite_url};
#[cfg(test)]
use app_core::CaptureSection;
#[cfg(test)]
use audio_pipeline::AudioChunk;
use storage_sqlite::Storage;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

#[cfg(test)]
use crate::assistant::{
    build_request_identity, selection_focus_excerpt, transcript_window_for_segment_ids,
};
use crate::runtime::run_runtime;
#[cfg(test)]
use crate::runtime::{UploadGate, effective_stt_status};
#[cfg(test)]
use transcript_core::TranscriptState;
#[cfg(test)]
use uuid::Uuid;

const MAX_HEALTH_SEGMENTS: usize = 48;

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
