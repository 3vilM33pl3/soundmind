use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    Signed16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureDevice {
    pub sink_name: String,
    pub monitor_source: String,
}

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub timestamp_ms: u64,
    pub samples: Vec<i16>,
    pub format: AudioFormat,
}

#[derive(Debug, Clone)]
pub enum CaptureEvent {
    DeviceChanged(CaptureDevice),
    Frames(AudioFrame),
    Recovering(String),
    Error(String),
    Ended,
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub frame_ms: u64,
    pub sample_rate_hz: u32,
    pub channels: u16,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self { frame_ms: 20, sample_rate_hz: 16_000, channels: 1 }
    }
}

#[async_trait]
pub trait AudioSource: Send {
    async fn run(&mut self, sender: mpsc::Sender<CaptureEvent>) -> Result<()>;
}

pub(crate) fn samples_per_frame(config: &CaptureConfig) -> usize {
    ((config.sample_rate_hz as u64 * config.frame_ms / 1_000) * u64::from(config.channels)) as usize
}
