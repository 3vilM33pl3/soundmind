#[cfg(target_os = "linux")]
mod platform {
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, bail};
    use async_trait::async_trait;
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;
    use tokio::sync::mpsc;
    use tracing::{info, warn};

    use crate::core::{
        AudioFormat, AudioFrame, AudioSource, CaptureConfig, CaptureDevice, CaptureEvent,
        SampleFormat, samples_per_frame,
    };

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
            let mut current_device = loop {
                match resolve_default_monitor().await {
                    Ok(device) => break device,
                    Err(error) => {
                        sender
                            .send(CaptureEvent::Recovering(format!(
                                "waiting for a default monitor source: {error}"
                            )))
                            .await
                            .ok();
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            };
            let mut restart_delay = Duration::from_millis(250);

            sender.send(CaptureEvent::DeviceChanged(current_device.clone())).await.ok();

            loop {
                let mut child = match spawn_parec(&self.config, &current_device.monitor_source) {
                    Ok(child) => child,
                    Err(error) => {
                        sender
                            .send(CaptureEvent::Recovering(format!(
                                "failed to bind monitor capture for {}: {error}",
                                current_device.monitor_source
                            )))
                            .await
                            .ok();
                        tokio::time::sleep(restart_delay).await;
                        restart_delay = next_backoff(restart_delay);
                        current_device = wait_for_default_monitor(&sender).await?;
                        sender.send(CaptureEvent::DeviceChanged(current_device.clone())).await.ok();
                        continue;
                    }
                };
                let mut stdout = child.stdout.take().context("parec stdout was not piped")?;
                let mut buf = vec![0_u8; bytes_per_frame];
                let mut poll = tokio::time::interval(Duration::from_secs(2));

                let requested_restart = loop {
                    tokio::select! {
                        read_result = stdout.read_exact(&mut buf) => {
                            if let Err(error) = read_result {
                                warn!(?error, "parec stream ended; restarting capture");
                                sender.send(CaptureEvent::Recovering(format!(
                                    "parec stream ended; restarting capture: {error}"
                                ))).await.ok();
                                break true;
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
                                let _ = child.wait().await;
                                return Ok(());
                            }
                        }
                        _ = poll.tick() => {
                            match resolve_default_monitor().await {
                                Ok(latest) => {
                                    if latest.monitor_source != current_device.monitor_source {
                                        info!(
                                            old = current_device.monitor_source,
                                            new = latest.monitor_source,
                                            "default sink changed; rebinding monitor capture"
                                        );
                                        current_device = latest.clone();
                                        sender.send(CaptureEvent::DeviceChanged(latest)).await.ok();
                                        let _ = child.kill().await;
                                        let _ = child.wait().await;
                                        restart_delay = Duration::from_millis(250);
                                        break true;
                                    }
                                }
                                Err(error) => {
                                    sender
                                        .send(CaptureEvent::Recovering(format!(
                                            "failed to refresh the default monitor source: {error}"
                                        )))
                                        .await
                                        .ok();
                                }
                            }
                        }
                    }
                };

                let _ = child.kill().await;
                let _ = child.wait().await;

                if requested_restart {
                    tokio::time::sleep(restart_delay).await;
                    restart_delay = next_backoff(restart_delay);
                    continue;
                }
            }
        }
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
        let sink_name = sink
            .lines()
            .find_map(|line| line.strip_prefix("Default Sink:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context("default sink was not reported by pactl info")?;

        let sources = command_stdout("pactl", &["list", "short", "sources"]).await?;
        let monitor_source = sources
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1))
            .find(|name| *name == format!("{sink_name}.monitor"))
            .map(ToOwned::to_owned)
            .with_context(|| format!("no monitor source found for sink {sink_name}"))?;

        Ok(CaptureDevice { sink_name: sink_name.to_string(), monitor_source })
    }

    async fn wait_for_default_monitor(sender: &mpsc::Sender<CaptureEvent>) -> Result<CaptureDevice> {
        let mut delay = Duration::from_millis(500);
        loop {
            match resolve_default_monitor().await {
                Ok(device) => return Ok(device),
                Err(error) => {
                    sender
                        .send(CaptureEvent::Recovering(format!(
                            "waiting for a default monitor source: {error}"
                        )))
                        .await
                        .ok();
                    tokio::time::sleep(delay).await;
                    delay = next_backoff(delay);
                }
            }
        }
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

        String::from_utf8(output.stdout).with_context(|| format!("{program} did not return utf-8"))
    }

    fn next_backoff(current: Duration) -> Duration {
        (current.saturating_mul(2)).min(Duration::from_secs(5))
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use anyhow::{Result, bail};
    use async_trait::async_trait;
    use tokio::sync::mpsc;

    use crate::core::{AudioSource, CaptureConfig, CaptureEvent};

    pub struct ParecMonitorAudioSource {
        _config: CaptureConfig,
    }

    impl ParecMonitorAudioSource {
        pub fn new(config: CaptureConfig) -> Self {
            Self { _config: config }
        }
    }

    #[async_trait]
    impl AudioSource for ParecMonitorAudioSource {
        async fn run(&mut self, _sender: mpsc::Sender<CaptureEvent>) -> Result<()> {
            bail!("Pulse/PipeWire monitor capture is only available on Linux")
        }
    }
}

pub use platform::ParecMonitorAudioSource;
