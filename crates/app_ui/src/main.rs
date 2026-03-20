use std::io::{Write, stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ipc_schema::{AppMode, BackendStatusSnapshot, UserAction};

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:8765";

#[tokio::main]
async fn main() -> Result<()> {
    let backend_url =
        std::env::var("SOUNDMIND_BACKEND_URL").unwrap_or_else(|_| DEFAULT_BACKEND_URL.to_string());
    let client = reqwest::Client::new();
    let mut terminal = TerminalGuard::enter()?;
    let mut snapshot = BackendStatusSnapshot::default();
    let mut last_refresh = Instant::now() - Duration::from_secs(10);

    loop {
        if last_refresh.elapsed() >= Duration::from_millis(800) {
            if let Ok(response) = client.get(format!("{backend_url}/health")).send().await {
                if let Ok(fetched) = response.json::<BackendStatusSnapshot>().await {
                    snapshot = fetched;
                }
            }
            render(&snapshot)?;
            last_refresh = Instant::now();
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let action = match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('a') => Some(UserAction::AnswerLastQuestion),
                    KeyCode::Char('s') => Some(UserAction::SummariseLastMinute),
                    KeyCode::Char('c') => Some(UserAction::CommentCurrentTopic),
                    KeyCode::Char('p') => Some(if snapshot.cloud_pause {
                        UserAction::ResumeCloud
                    } else {
                        UserAction::PauseCloud
                    }),
                    KeyCode::Char(' ') => Some(if snapshot.privacy_pause {
                        UserAction::Start
                    } else {
                        UserAction::Stop
                    }),
                    KeyCode::Char('m') => Some(UserAction::SetMode(next_mode(snapshot.mode))),
                    _ => None,
                };

                if let Some(action) = action {
                    let _ =
                        client.post(format!("{backend_url}/actions")).json(&action).send().await;
                    last_refresh = Instant::now() - Duration::from_secs(10);
                }
            }
        }
    }

    terminal.exit()?;
    Ok(())
}

fn render(snapshot: &BackendStatusSnapshot) -> Result<()> {
    let mut out = stdout();
    execute!(out, MoveTo(0, 0), Clear(ClearType::All))?;
    writeln!(out, "Soundmind Terminal UI")?;
    writeln!(
        out,
        "q quit  space start/stop  p pause-cloud  a answer  s summary  c comment  m cycle-mode"
    )?;
    writeln!(out)?;
    writeln!(out, "Mode: {:?}", snapshot.mode)?;
    writeln!(out, "Capture: {:?}  Cloud: {:?}", snapshot.capture_state, snapshot.cloud_state)?;
    writeln!(
        out,
        "Current sink: {}",
        snapshot.current_sink.clone().unwrap_or_else(|| "unknown".to_string())
    )?;
    writeln!(
        out,
        "Privacy pause: {}  Cloud pause: {}",
        snapshot.privacy_pause, snapshot.cloud_pause
    )?;
    writeln!(
        out,
        "Session: {}",
        snapshot.session_id.map(|id| id.to_string()).unwrap_or_else(|| "not started".to_string())
    )?;
    writeln!(out)?;
    writeln!(out, "Partial transcript:")?;
    writeln!(
        out,
        "{}",
        snapshot.transcript.partial_text.clone().unwrap_or_else(|| "-".to_string())
    )?;
    writeln!(out)?;
    writeln!(out, "Committed transcript:")?;
    if snapshot.transcript.segments.is_empty() {
        writeln!(out, "-")?;
    } else {
        for segment in snapshot.transcript.segments.iter().rev().take(8).rev() {
            writeln!(out, "[{}-{}ms] {}", segment.start_ms, segment.end_ms, segment.text)?;
        }
    }
    writeln!(out)?;
    writeln!(out, "Latest assistant output:")?;
    if let Some(assistant) = &snapshot.latest_assistant {
        writeln!(out, "{:?} @ {}:", assistant.kind, assistant.created_at.to_rfc3339())?;
        writeln!(out, "{}", assistant.content)?;
    } else {
        writeln!(out, "-")?;
    }
    writeln!(out)?;
    writeln!(out, "Recent errors:")?;
    if snapshot.recent_errors.is_empty() {
        writeln!(out, "-")?;
    } else {
        for error in &snapshot.recent_errors {
            writeln!(out, "- {}", error)?;
        }
    }
    out.flush()?;
    Ok(())
}

fn next_mode(mode: AppMode) -> AppMode {
    match mode {
        AppMode::CaptionsOnly => AppMode::ManualQa,
        AppMode::ManualQa => AppMode::Assisted,
        AppMode::Assisted => AppMode::Summary,
        AppMode::Summary => AppMode::CaptionsOnly,
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }

    fn exit(&mut self) -> Result<()> {
        disable_raw_mode()?;
        execute!(stdout(), LeaveAlternateScreen)?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.exit();
    }
}
