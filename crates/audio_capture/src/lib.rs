use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{info, warn};

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

pub struct ParecMonitorAudioSource {
    config: CaptureConfig,
}

impl ParecMonitorAudioSource {
    pub fn new(config: CaptureConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AudioSource for ParecMonitorAudioSource {
    async fn run(&mut self, sender: mpsc::Sender<CaptureEvent>) -> Result<()> {
        let frame_len = samples_per_frame(&self.config);
        let bytes_per_frame = frame_len * 2;
        let start = Instant::now();
        let mut current_device = resolve_default_monitor().await?;

        sender.send(CaptureEvent::DeviceChanged(current_device.clone())).await.ok();

        loop {
            let mut child = spawn_parec(&self.config, &current_device.monitor_source)?;
            let mut stdout = child.stdout.take().context("parec stdout was not piped")?;
            let mut buf = vec![0_u8; bytes_per_frame];
            let mut poll = tokio::time::interval(Duration::from_secs(2));

            loop {
                tokio::select! {
                    read_result = stdout.read_exact(&mut buf) => {
                        if let Err(error) = read_result {
                            warn!(?error, "parec stream ended; restarting capture");
                            sender.send(CaptureEvent::Error(error.to_string())).await.ok();
                            break;
                        }

                        let samples = buf
                            .chunks_exact(2)
                            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                            .collect::<Vec<_>>();

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
                            let _ = child.kill().await;
                            return Ok(());
                        }
                    }
                    _ = poll.tick() => {
                        let latest = resolve_default_monitor().await?;
                        if latest.monitor_source != current_device.monitor_source {
                            info!(
                                old = current_device.monitor_source,
                                new = latest.monitor_source,
                                "default sink changed; rebinding monitor capture"
                            );
                            current_device = latest.clone();
                            sender.send(CaptureEvent::DeviceChanged(latest)).await.ok();
                            let _ = child.kill().await;
                            break;
                        }
                    }
                }
            }
        }
    }
}

fn samples_per_frame(config: &CaptureConfig) -> usize {
    ((config.sample_rate_hz as u64 * config.frame_ms / 1_000) * u64::from(config.channels)) as usize
}

fn spawn_parec(config: &CaptureConfig, monitor_source: &str) -> Result<tokio::process::Child> {
    let child = Command::new("parec")
        .arg("--device")
        .arg(monitor_source)
        .arg("--raw")
        .arg("--format=s16le")
        .arg("--rate")
        .arg(config.sample_rate_hz.to_string())
        .arg("--channels")
        .arg(config.channels.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn parec")?;

    Ok(child)
}

async fn resolve_default_monitor() -> Result<CaptureDevice> {
    let sink = command_stdout("pactl", &["info"]).await?;
    let default_sink = sink
        .lines()
        .find_map(|line| line.strip_prefix("Default Sink: "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("default sink was not reported by pactl info")?;

    let sources = command_stdout("pactl", &["list", "short", "sources"]).await?;
    let monitor_source = sources
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _index = fields.next()?;
            let name = fields.next()?;
            name.ends_with(".monitor").then_some(name)
        })
        .find(|source| source.starts_with(default_sink))
        .context("failed to find monitor source for default sink")?;

    Ok(CaptureDevice {
        sink_name: default_sink.to_string(),
        monitor_source: monitor_source.to_string(),
    })
}

async fn command_stdout(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to run {program}"))?;

    if !output.status.success() {
        bail!("{program} exited with status {}", output.status);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sample_count_matches_config() {
        let config = CaptureConfig { frame_ms: 20, sample_rate_hz: 16_000, channels: 1 };

        assert_eq!(samples_per_frame(&config), 320);
    }
}
