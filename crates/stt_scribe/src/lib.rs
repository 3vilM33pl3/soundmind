use anyhow::Result;
use async_trait::async_trait;
use audio_pipeline::AudioChunk;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

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
