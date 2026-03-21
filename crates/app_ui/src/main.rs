use std::io::{Write, stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::queue;
use crossterm::style::Print;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ipc_schema::{AppMode, BackendStatusSnapshot, UserAction};

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:8765";

#[derive(Debug, Clone, Default)]
struct BackendConnectionState {
    connected: bool,
    last_error: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let backend_url =
        std::env::var("SOUNDMIND_BACKEND_URL").unwrap_or_else(|_| DEFAULT_BACKEND_URL.to_string());
    let client = reqwest::Client::new();
    let mut terminal = TerminalGuard::enter()?;
    let mut snapshot = BackendStatusSnapshot::default();
    let mut connection = BackendConnectionState::default();
    let mut last_refresh = Instant::now() - Duration::from_secs(10);

    loop {
        if last_refresh.elapsed() >= Duration::from_millis(800) {
            match client.get(format!("{backend_url}/health")).send().await {
                Ok(response) => match response.error_for_status() {
                    Ok(response) => match response.json::<BackendStatusSnapshot>().await {
                        Ok(fetched) => {
                            snapshot = fetched;
                            connection.connected = true;
                            connection.last_error = None;
                        }
                        Err(error) => {
                            connection.connected = false;
                            connection.last_error =
                                Some(format!("Failed to decode backend response: {error}"));
                        }
                    },
                    Err(error) => {
                        connection.connected = false;
                        connection.last_error = Some(format!("Backend returned an error: {error}"));
                    }
                },
                Err(error) => {
                    connection.connected = false;
                    connection.last_error = Some(format!(
                        "Cannot reach backend at {backend_url}. Start it with: cargo run -p app_backend ({error})"
                    ));
                }
            }
            render(&snapshot, &connection, &backend_url)?;
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

fn render(
    snapshot: &BackendStatusSnapshot,
    connection: &BackendConnectionState,
    backend_url: &str,
) -> Result<()> {
    let mut out = stdout();
    let mut lines = vec![
        "Soundmind Terminal UI".to_string(),
        "q quit  space start/stop  p pause-cloud  a answer  s summary  c comment  m cycle-mode"
            .to_string(),
        format!("Backend URL: {backend_url}"),
        format!(
            "Backend status: {}",
            if connection.connected { "connected" } else { "disconnected" }
        ),
    ];

    if let Some(error) = &connection.last_error {
        lines.push(format!("Backend note: {error}"));
    }

    lines.extend([
        String::new(),
        format!("Mode: {:?}", snapshot.mode),
        format!("Capture: {:?}  Cloud: {:?}", snapshot.capture_state, snapshot.cloud_state),
        format!(
            "Current sink: {}",
            snapshot.current_sink.clone().unwrap_or_else(|| "unknown".to_string())
        ),
        format!("Privacy pause: {}  Cloud pause: {}", snapshot.privacy_pause, snapshot.cloud_pause),
        format!(
            "Session: {}",
            snapshot
                .session_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "not started".to_string())
        ),
        String::new(),
        "Partial transcript:".to_string(),
        snapshot.transcript.partial_text.clone().unwrap_or_else(|| "-".to_string()),
        String::new(),
        "Committed transcript:".to_string(),
    ]);

    if snapshot.transcript.segments.is_empty() {
        lines.push("-".to_string());
    } else {
        lines.extend(snapshot.transcript.segments.iter().rev().take(8).rev().map(|segment| {
            format!("[{}-{}ms] {}", segment.start_ms, segment.end_ms, segment.text)
        }));
    }

    lines.extend([String::new(), "Latest assistant output:".to_string()]);

    if let Some(assistant) = &snapshot.latest_assistant {
        lines.push(format!("{:?} @ {}:", assistant.kind, assistant.created_at.to_rfc3339()));
        lines.push(assistant.content.clone());
    } else {
        lines.push("-".to_string());
    }

    lines.extend([String::new(), "Recent errors:".to_string()]);
    if snapshot.recent_errors.is_empty() {
        lines.push("-".to_string());
    } else {
        lines.extend(snapshot.recent_errors.iter().map(|error| format!("- {error}")));
    }

    execute!(out, MoveTo(0, 0), Clear(ClearType::All))?;
    for (row, line) in lines.iter().enumerate() {
        queue!(out, MoveTo(0, row as u16), Print(line))?;
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
