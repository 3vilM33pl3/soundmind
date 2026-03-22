# Soundmind

Soundmind is an Ubuntu desktop assistant for system audio. It captures the
current output monitor, transcribes speech in near real time, detects likely
questions, and lets you generate answers, summaries, and commentary while
keeping session history and agent setup locally in SQLite.

## What it does

- Captures desktop audio through PulseAudio or PipeWire's Pulse compatibility
  layer
- Streams audio to ElevenLabs Scribe Realtime for transcription
- Uses OpenAI for manual answer, summary, and commentary actions
- Shows live transcript and assistant output in a Tauri desktop UI
- Flags detected questions explicitly in the UI
- Lets you configure a default interview-assistant instruction
- Lets you upload priming documents such as your CV, job description, or notes
- Stores sessions, transcript segments, settings, priming documents, and assistant output locally
- Supports tray controls, global shortcuts, history, export, and `systemd --user`
  installation

## Quick start

1. Copy the config:

```bash
cp config.example.toml config.toml
```

2. Put your provider keys in `keys.env`:

```bash
OPENAI_API_KEY=...
ELEVENLABS_API_KEY=...
```

3. Start the backend:

```bash
cargo run -p app_backend
```

4. Start the desktop UI:

```bash
cargo run -p app_ui
```

5. In the desktop UI, open **Agent Configuration** to:
- edit the default interview-assistant instruction
- upload priming documents for the model to use

Best results come from text or markdown files. PDF upload also works when
`pdftotext` is installed on the machine.

6. If you want the terminal debug client instead:

```bash
cargo run -p app_ui --bin terminal_ui
```

## Install as a user service

```bash
./scripts/install-user-service.sh
soundmind
```

This installs release binaries under `~/.local`, writes config to
`~/.config/soundmind`, and enables `soundmind-backend.service` with
`systemctl --user`.

## Default shortcuts

- `Ctrl+Alt+Shift+M`: show or hide the main window
- `Ctrl+Alt+Shift+A`: answer the detected or last question
- `Ctrl+Alt+Shift+S`: summarise the last minute
- `Ctrl+Alt+Shift+C`: comment on the current topic

If another app already owns one of these shortcuts, Soundmind still launches.

## Documentation

- [Installation guide](docs/INSTALLATION.md)
- [User guide](docs/USER_GUIDE.md)
- [Development guide](docs/DEVELOPMENT.md)
- [Architecture notes](docs/architecture.md)
- [Implementation roadmap](docs/tasks.md)

## Current status

The current build includes:

- real system-audio capture
- real ElevenLabs realtime STT integration
- OpenAI-backed manual answer, summary, and commentary actions
- question detection surfaced in the desktop UI
- local settings, history, export, and privacy status
- configurable interview instruction plus uploaded priming documents
- tray integration and best-effort global shortcuts
- packaging helpers and `systemd --user` assets
