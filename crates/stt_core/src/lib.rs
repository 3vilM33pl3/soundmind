use anyhow::Result;
use async_trait::async_trait;
use audio_pipeline::AudioChunk;
use serde::{Deserialize, Serialize};

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
