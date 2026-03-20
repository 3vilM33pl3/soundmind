# Soundmind

Soundmind is an Ubuntu system-audio assistant written in Rust. It captures the
current output sink monitor, builds rolling transcript context, stores sessions
locally in SQLite, and exposes manual assistant actions such as answering the
last question or summarizing recent audio.

This repository currently includes:

- a Rust workspace with focused crates for capture, pipeline, transcript,
  policy, OpenAI integration, storage, backend orchestration, and a terminal UI
- a `parec`/`pactl` capture backend for Ubuntu PulseAudio or PipeWire
- a mock STT adapter that keeps the end-to-end flow runnable without live
  ElevenLabs credentials
- a backend HTTP control plane and a terminal dashboard client

## Quick start

1. Copy `config.example.toml` to `config.toml`.
2. Optionally copy `.env.example` to `.env` and set provider keys.
3. Run the backend:

```bash
cargo run -p app_backend
```

4. In another terminal, run the dashboard:

```bash
cargo run -p app_ui
```

## Current status

The repository implements the first vertical slice of the architecture:

- workspace and crate structure
- capture abstraction plus Ubuntu monitor-source capture
- audio chunking and silence gate
- transcript state and rolling context helpers
- SQLite persistence and audit/session tracking
- OpenAI Responses API adapter with a local fallback
- terminal UI for live status and manual actions

The desktop shell, live ElevenLabs Scribe integration, tray support, hotkeys,
and packaging are still separate follow-up milestones.
