use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use audio_pipeline::AudioChunk;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::Request},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialTranscript {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalTranscript {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriberHealth {
    pub healthy: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TranscriberEvent {
    PartialTranscript(PartialTranscript),
    FinalTranscript(FinalTranscript),
    Error(String),
    Health(TranscriberHealth),
}

#[async_trait]
pub trait Transcriber: Send {
    async fn start(&mut self) -> Result<()>;
    async fn push_audio(&mut self, chunk: AudioChunk) -> Result<()>;
    fn try_recv_event(&mut self) -> Option<TranscriberEvent>;
    async fn stop(&mut self) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct ScribeRealtimeConfig {
    pub api_key: String,
    pub model_id: String,
    pub sample_rate_hz: u32,
    pub language_code: Option<String>,
    pub include_timestamps: bool,
    pub enable_logging: bool,
}

impl Default for ScribeRealtimeConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model_id: "scribe_v2_realtime".to_string(),
            sample_rate_hz: 16_000,
            language_code: Some("en".to_string()),
            include_timestamps: true,
            enable_logging: true,
        }
    }
}

pub struct ScribeRealtimeTranscriber {
    config: ScribeRealtimeConfig,
    event_tx: mpsc::Sender<TranscriberEvent>,
    event_rx: mpsc::Receiver<TranscriberEvent>,
    writer: Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
    reader_task: Option<tokio::task::JoinHandle<()>>,
    last_window: Arc<Mutex<Option<(u64, u64)>>>,
    connected: Arc<AtomicBool>,
    reconnect_delay: Duration,
}

impl ScribeRealtimeTranscriber {
    pub fn new(config: ScribeRealtimeConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(128);
        Self {
            config,
            event_tx,
            event_rx,
            writer: None,
            reader_task: None,
            last_window: Arc::new(Mutex::new(None)),
            connected: Arc::new(AtomicBool::new(false)),
            reconnect_delay: Duration::from_millis(500),
        }
    }

    async fn connect(&mut self) -> Result<()> {
        let mut request = build_request(&self.config)?;
        request.headers_mut().insert(
            "xi-api-key",
            self.config.api_key.parse().context("invalid xi-api-key header")?,
        );

        let (stream, _response) =
            connect_async(request).await.context("failed to connect to ElevenLabs realtime STT")?;
        let (writer, mut reader) = stream.split();
        self.writer = Some(writer);
        self.connected.store(true, Ordering::SeqCst);

        let event_tx = self.event_tx.clone();
        let last_window = Arc::clone(&self.last_window);
        let connected = Arc::clone(&self.connected);
        self.reader_task = Some(tokio::spawn(async move {
            while let Some(message) = reader.next().await {
                match message {
                    Ok(Message::Text(payload)) => {
                        if let Some(event) = parse_realtime_event(&payload, &last_window).await {
                            event_tx.send(event).await.ok();
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        connected.store(false, Ordering::SeqCst);
                        let reason = frame
                            .map(|frame| frame.reason.to_string())
                            .unwrap_or_else(|| "websocket closed".to_string());
                        event_tx.send(TranscriberEvent::Error(reason)).await.ok();
                        break;
                    }
                    Ok(Message::Binary(_))
                    | Ok(Message::Ping(_))
                    | Ok(Message::Pong(_))
                    | Ok(Message::Frame(_)) => {}
                    Err(error) => {
                        connected.store(false, Ordering::SeqCst);
                        event_tx
                            .send(TranscriberEvent::Error(format!(
                                "ElevenLabs websocket read failed: {error}"
                            )))
                            .await
                            .ok();
                        break;
                    }
                }
            }
            connected.store(false, Ordering::SeqCst);
        }));

        self.event_tx
            .send(TranscriberEvent::Health(TranscriberHealth {
                healthy: true,
                message: "connected to ElevenLabs Scribe realtime".to_string(),
            }))
            .await
            .ok();
        Ok(())
    }

    async fn reconnect(&mut self) -> Result<()> {
        self.stop().await.ok();

        let mut delay = self.reconnect_delay;
        let mut last_error = None;
        for attempt in 1..=3 {
            match self.connect().await {
                Ok(()) => {
                    self.reconnect_delay = Duration::from_millis(500);
                    if attempt > 1 {
                        self.event_tx
                            .send(TranscriberEvent::Health(TranscriberHealth {
                                healthy: true,
                                message: "reconnected to ElevenLabs Scribe realtime".to_string(),
                            }))
                            .await
                            .ok();
                    }
                    return Ok(());
                }
                Err(error) => {
                    let message =
                        format!("ElevenLabs reconnect attempt {attempt}/3 failed: {error}");
                    self.event_tx.send(TranscriberEvent::Error(message)).await.ok();
                    last_error = Some(error);
                    tokio::time::sleep(delay).await;
                    delay = next_backoff(delay);
                }
            }
        }

        self.reconnect_delay = delay;
        Err(last_error.unwrap_or_else(|| anyhow!("failed to reconnect to ElevenLabs")))
    }

    async fn send_chunk(&mut self, chunk: &AudioChunk) -> Result<()> {
        let writer = self.writer.as_mut().context("ElevenLabs realtime writer is not connected")?;
        *self.last_window.lock().await = Some((chunk.start_ms, chunk.end_ms));

        let pcm_bytes = chunk
            .samples
            .iter()
            .flat_map(|sample| {
                let clamped = sample.clamp(-1.0, 1.0);
                ((clamped * i16::MAX as f32) as i16).to_le_bytes()
            })
            .collect::<Vec<_>>();

        let payload = json!({
            "message_type": "input_audio_chunk",
            "audio_base_64": BASE64_STANDARD.encode(pcm_bytes),
            "sample_rate": self.config.sample_rate_hz,
        });

        writer
            .send(Message::Text(payload.to_string().into()))
            .await
            .context("failed to send audio chunk to ElevenLabs")?;
        Ok(())
    }
}

#[async_trait]
impl Transcriber for ScribeRealtimeTranscriber {
    async fn start(&mut self) -> Result<()> {
        self.reconnect().await
    }

    async fn push_audio(&mut self, chunk: AudioChunk) -> Result<()> {
        if !self.connected.load(Ordering::SeqCst) || self.writer.is_none() {
            self.reconnect().await?;
        }

        if let Err(error) = self.send_chunk(&chunk).await {
            self.connected.store(false, Ordering::SeqCst);
            self.event_tx
                .send(TranscriberEvent::Error(format!(
                    "ElevenLabs audio send failed; attempting reconnect: {error}"
                )))
                .await
                .ok();
            self.reconnect().await?;
            self.send_chunk(&chunk).await?;
        }

        Ok(())
    }

    fn try_recv_event(&mut self) -> Option<TranscriberEvent> {
        self.event_rx.try_recv().ok()
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.send(Message::Close(None)).await;
        }
        if let Some(reader_task) = self.reader_task.take() {
            reader_task.abort();
        }
        self.writer = None;
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }
}

fn next_backoff(current: Duration) -> Duration {
    (current.saturating_mul(2)).min(Duration::from_secs(5))
}

fn build_request(config: &ScribeRealtimeConfig) -> Result<Request<()>> {
    let mut url = format!(
        "wss://api.elevenlabs.io/v1/speech-to-text/realtime?model_id={}&include_timestamps={}&audio_format=pcm_16000&commit_strategy=vad&enable_logging={}",
        config.model_id, config.include_timestamps, config.enable_logging
    );

    if let Some(language_code) = &config.language_code {
        url.push_str("&language_code=");
        url.push_str(language_code);
    }

    url.into_client_request().map_err(|error| anyhow!(error))
}

async fn parse_realtime_event(
    payload: &str,
    last_window: &Arc<Mutex<Option<(u64, u64)>>>,
) -> Option<TranscriberEvent> {
    let value: Value = match serde_json::from_str(payload) {
        Ok(value) => value,
        Err(error) => {
            return Some(TranscriberEvent::Error(format!(
                "failed to decode ElevenLabs event payload: {error}"
            )));
        }
    };

    let message_type = value.get("message_type").and_then(Value::as_str).unwrap_or_default();
    let text =
        value.get("text").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();

    match message_type {
        "session_started" => Some(TranscriberEvent::Health(TranscriberHealth {
            healthy: true,
            message: value
                .get("session_id")
                .and_then(Value::as_str)
                .map(|session_id| format!("ElevenLabs session started: {session_id}"))
                .unwrap_or_else(|| "ElevenLabs session started".to_string()),
        })),
        "partial_transcript" if !text.is_empty() => {
            let window = *last_window.lock().await;
            let (start_ms, end_ms) = window.unwrap_or((0, 0));
            Some(TranscriberEvent::PartialTranscript(PartialTranscript {
                start_ms,
                end_ms,
                text,
                source: "elevenlabs_scribe_realtime".to_string(),
            }))
        }
        "committed_transcript" | "committed_transcript_with_timestamps" if !text.is_empty() => {
            let window = *last_window.lock().await;
            let (start_ms, end_ms) = window.unwrap_or((0, 0));
            Some(TranscriberEvent::FinalTranscript(FinalTranscript {
                start_ms,
                end_ms,
                text,
                source: "elevenlabs_scribe_realtime".to_string(),
            }))
        }
        other if other.contains("error") => {
            Some(TranscriberEvent::Error(extract_error_message(&value)))
        }
        _ => None,
    }
}

fn extract_error_message(value: &Value) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| value.get("detail").and_then(Value::as_str))
        .or_else(|| value.get("error").and_then(Value::as_str))
        .unwrap_or("ElevenLabs reported an unknown error")
        .to_string()
}

pub struct MockTranscriber {
    phrases: Vec<&'static str>,
    next_index: usize,
    event_tx: mpsc::Sender<TranscriberEvent>,
    event_rx: mpsc::Receiver<TranscriberEvent>,
    active_chunks: usize,
}

impl MockTranscriber {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(64);
        Self {
            phrases: vec![
                "Can someone explain what BGP does?",
                "The rollout needs a summary before the meeting ends.",
                "What changed in the retention policy?",
                "We should pause cloud processing for sensitive content.",
            ],
            next_index: 0,
            event_tx,
            event_rx,
            active_chunks: 0,
        }
    }
}

impl Default for MockTranscriber {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transcriber for MockTranscriber {
    async fn start(&mut self) -> Result<()> {
        self.event_tx
            .send(TranscriberEvent::Health(TranscriberHealth {
                healthy: true,
                message: "mock transcriber ready".to_string(),
            }))
            .await
            .ok();
        Ok(())
    }

    async fn push_audio(&mut self, chunk: AudioChunk) -> Result<()> {
        if !chunk.speech_likely {
            self.active_chunks = 0;
            return Ok(());
        }

        self.active_chunks += 1;
        let phrase = self.phrases[self.next_index];
        self.event_tx
            .send(TranscriberEvent::PartialTranscript(PartialTranscript {
                start_ms: chunk.start_ms,
                end_ms: chunk.end_ms,
                text: phrase.to_string(),
                source: "mock_scribe".to_string(),
            }))
            .await
            .ok();

        if self.active_chunks >= 3 {
            self.event_tx
                .send(TranscriberEvent::FinalTranscript(FinalTranscript {
                    start_ms: chunk.start_ms,
                    end_ms: chunk.end_ms,
                    text: phrase.to_string(),
                    source: "mock_scribe".to_string(),
                }))
                .await
                .ok();
            self.active_chunks = 0;
            self.next_index = (self.next_index + 1) % self.phrases.len();
        }

        Ok(())
    }

    fn try_recv_event(&mut self) -> Option<TranscriberEvent> {
        self.event_rx.try_recv().ok()
    }

    async fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parses_partial_transcript_messages() {
        let last_window = Arc::new(Mutex::new(Some((1200, 1400))));
        let event = parse_realtime_event(
            r#"{"message_type":"partial_transcript","text":"hello world"}"#,
            &last_window,
        )
        .await;

        match event {
            Some(TranscriberEvent::PartialTranscript(transcript)) => {
                assert_eq!(transcript.start_ms, 1200);
                assert_eq!(transcript.end_ms, 1400);
                assert_eq!(transcript.text, "hello world");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn parses_error_messages() {
        let last_window = Arc::new(Mutex::new(None));
        let event = parse_realtime_event(
            r#"{"message_type":"scribe_rate_limited_error","message":"too many requests"}"#,
            &last_window,
        )
        .await;

        match event {
            Some(TranscriberEvent::Error(message)) => {
                assert!(message.contains("too many requests"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn build_request_uses_vad_commit_strategy() {
        let request = build_request(&ScribeRealtimeConfig::default()).expect("request");
        let uri = request.uri().to_string();

        assert!(uri.contains("commit_strategy=vad"));
        assert!(!uri.contains("commit_strategy=manual"));
    }
}
