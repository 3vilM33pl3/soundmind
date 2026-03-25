use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::core::{
    AudioFormat, AudioFrame, AudioSource, CaptureConfig, CaptureDevice, CaptureEvent, SampleFormat,
    samples_per_frame,
};

pub struct MockAudioSource {
    config: CaptureConfig,
}

impl MockAudioSource {
    pub fn new(config: CaptureConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AudioSource for MockAudioSource {
    async fn run(&mut self, sender: mpsc::Sender<CaptureEvent>) -> Result<()> {
        let device = CaptureDevice {
            sink_name: "mock-output".to_string(),
            monitor_source: "mock-monitor".to_string(),
        };
        sender.send(CaptureEvent::DeviceChanged(device)).await.ok();

        let frame_len = samples_per_frame(&self.config);
        let start = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_millis(self.config.frame_ms));
        let mut phase = 0.0_f32;

        loop {
            tick.tick().await;
            let mut samples = Vec::with_capacity(frame_len);
            let burst = ((start.elapsed().as_secs() / 4) % 2) == 0;
            for _ in 0..frame_len {
                let value = if burst { (phase.sin() * 12_000.0) as i16 } else { 0 };
                phase += 0.08;
                samples.push(value);
            }

            let event = CaptureEvent::Frames(AudioFrame {
                timestamp_ms: start.elapsed().as_millis() as u64,
                samples,
                format: AudioFormat {
                    sample_rate_hz: self.config.sample_rate_hz,
                    channels: self.config.channels,
                    sample_format: SampleFormat::Signed16,
                },
            });

            if sender.send(event).await.is_err() {
                return Ok(());
            }
        }
    }
}
