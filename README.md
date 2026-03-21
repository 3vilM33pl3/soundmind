# Soundmind

Soundmind is an Ubuntu system-audio assistant written in Rust. It captures the
current output sink monitor, builds rolling transcript context, stores sessions
locally in SQLite, and exposes manual assistant actions such as answering the
last question or summarizing recent audio.

This repository currently includes:

- a Rust workspace with focused crates for capture, pipeline, transcript,
  policy, OpenAI integration, storage, backend orchestration, and a terminal UI
- a `parec`/`pactl` capture backend for Ubuntu PulseAudio or PipeWire
- a real ElevenLabs Scribe realtime transcriber with automatic fallback to the
  mock adapter when live startup fails
- a backend HTTP control plane, a Tauri desktop shell, and a terminal dashboard
  debug client
- persistent settings, local session history, transcript export, privacy state,
  tray controls, and global shortcuts in the desktop shell

## Quick start

1. Copy `config.example.toml` to `config.toml`.
2. Put provider keys in `keys.env` or export them in your shell environment.
3. Run the backend:

```bash
cargo run -p app_backend
```

4. In another terminal, run the dashboard:

```bash
cargo run -p app_ui
```

5. If you want the old debug client instead of the desktop shell:

```bash
cargo run -p app_ui --bin terminal_ui
```

## Desktop shell

The Tauri UI now includes:

- live transcript and assistant panels
- persistent settings for retention, transcript storage, auto-start cloud, and
  default mode
- local session history with detail view, JSON export, Markdown export, and
  delete/purge actions
- always-visible privacy state for capture, cloud, and local storage
- tray controls plus global shortcuts for showing the window and triggering
  answer, summary, and commentary actions

Default global shortcuts:

- `Ctrl+Alt+Shift+M`: show or hide the main window
- `Ctrl+Alt+Shift+A`: answer the last question
- `Ctrl+Alt+Shift+S`: summarise the last minute
- `Ctrl+Alt+Shift+C`: comment on the current topic

## User service install

For a local user install with a persistent backend service:

```bash
./scripts/install-user-service.sh
```

This script:

- builds release binaries
- installs wrappers under `~/.local/bin`
- installs config under `~/.config/soundmind`
- installs a desktop entry and icon
- enables `soundmind-backend.service` with `systemctl --user`

To build a redistributable tarball without installing it:

```bash
./scripts/package-release.sh
```

## Current status

The repository implements the first vertical slice of the architecture:

- workspace and crate structure
- capture abstraction plus Ubuntu monitor-source capture
- audio chunking and silence gate
- transcript state and rolling context helpers
- SQLite persistence and audit/session tracking
- OpenAI Responses API adapter with a local fallback
- Tauri desktop UI for live status, settings, history, export, tray, and
  privacy controls
- terminal UI retained as a debug client
- user-service and packaging assets for Linux installs
