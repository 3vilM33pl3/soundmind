# Development

## Workspace layout

The project is a Rust workspace with focused crates:

- `app_backend`: runtime orchestration and HTTP API
- `app_ui`: Tauri desktop shell and terminal debug UI
- `audio_capture`: monitor-source capture and device rebinding
- `audio_pipeline`: chunking and silence gating
- `stt_scribe`: ElevenLabs realtime STT adapter and mock adapter
- `transcript_core`: partial/final transcript state and question detection
- `context_engine`: recent transcript helpers
- `policy_engine`: response throttling and cloud gating
- `llm_openai`: OpenAI-backed answer, summary, and commentary generation
- `storage_sqlite`: SQLite storage and migrations
- `ipc_schema`: shared DTOs between backend and UI

## Local development loop

Backend:

```bash
cargo run -p app_backend
```

Desktop UI:

```bash
cargo run -p app_ui
```

Terminal debug UI:

```bash
cargo run -p app_ui --bin terminal_ui
```

## Tests and checks

```bash
cargo check
cargo test
```

## Runtime notes

- The backend serves HTTP on `127.0.0.1:8765` by default.
- The desktop shell polls `/health` for live state and uses `/actions`,
  `/settings`, and `/sessions` for interaction.
- `/health` only includes the most recent committed transcript window for live
  rendering; full historical data comes from the session APIs.
- Global shortcut registration is best-effort so the app still launches if
  another process already owns one of the configured shortcuts.

## Packaging

Install locally as a user service:

```bash
./scripts/install-user-service.sh
```

Create a release bundle:

```bash
./scripts/package-release.sh
```

Relevant assets:

- `packaging/systemd/soundmind-backend.service`
- `packaging/linux/soundmind.desktop.in`
