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

## Current status

The repository implements the first vertical slice of the architecture:

- workspace and crate structure
- capture abstraction plus Ubuntu monitor-source capture
- audio chunking and silence gate
- transcript state and rolling context helpers
- SQLite persistence and audit/session tracking
- OpenAI Responses API adapter with a local fallback
- Tauri desktop UI for live status and manual actions
- terminal UI retained as a debug client

Tray support, hotkeys, settings/history/privacy UX, and packaging are still
separate follow-up milestones.
