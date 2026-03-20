use audio_capture::AudioFrame;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioChunk {
    pub start_ms: u64,
    pub end_ms: u64,
    pub samples: Vec<f32>,
    pub energy: f32,
    pub speech_likely: bool,
}

#[derive(Debug, Clone)]
pub struct AudioPipelineConfig {
    pub silence_threshold: f32,
    pub chunk_ms: u64,
}

impl Default for AudioPipelineConfig {
    fn default() -> Self {
        Self { silence_threshold: 0.008, chunk_ms: 200 }
    }
}

pub struct AudioPipeline {
    config: AudioPipelineConfig,
    buffered_samples: Vec<f32>,
    buffered_start_ms: Option<u64>,
    frame_duration_ms: u64,
}

impl AudioPipeline {
    pub fn new(config: AudioPipelineConfig, frame_duration_ms: u64) -> Self {
        Self { config, buffered_samples: Vec::new(), buffered_start_ms: None, frame_duration_ms }
    }

    pub fn push_frame(&mut self, frame: AudioFrame) -> Option<AudioChunk> {
        if self.buffered_start_ms.is_none() {
            self.buffered_start_ms = Some(frame.timestamp_ms);
        }

        let end_ms = frame.timestamp_ms + self.frame_duration_ms;
        let normalized = frame
            .samples
            .into_iter()
            .map(|sample| sample as f32 / i16::MAX as f32)
            .collect::<Vec<_>>();
        self.buffered_samples.extend(normalized);

        let start_ms = self.buffered_start_ms?;
        if end_ms.saturating_sub(start_ms) < self.config.chunk_ms {
            return None;
        }

        let samples = std::mem::take(&mut self.buffered_samples);
        self.buffered_start_ms = Some(end_ms);
        let energy = rms_energy(&samples);

        Some(AudioChunk {
            start_ms,
            end_ms,
            samples,
            energy,
            speech_likely: energy >= self.config.silence_threshold,
        })
    }
}

fn rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use audio_capture::{AudioFormat, SampleFormat};

    use super::*;

    #[test]
    fn emits_chunk_after_target_window() {
        let mut pipeline = AudioPipeline::new(AudioPipelineConfig::default(), 20);

        for frame_index in 0..9 {
            let chunk = pipeline.push_frame(AudioFrame {
                timestamp_ms: frame_index * 20,
                samples: vec![1000; 320],
                format: AudioFormat {
                    sample_rate_hz: 16_000,
                    channels: 1,
                    sample_format: SampleFormat::Signed16,
                },
            });
            assert!(chunk.is_none());
        }

        let chunk = pipeline.push_frame(AudioFrame {
            timestamp_ms: 180,
            samples: vec![1000; 320],
            format: AudioFormat {
                sample_rate_hz: 16_000,
                channels: 1,
                sample_format: SampleFormat::Signed16,
            },
        });

        assert!(chunk.is_some());
        assert_eq!(chunk.unwrap().start_ms, 0);
    }
}
