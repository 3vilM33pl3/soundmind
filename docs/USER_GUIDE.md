# User Guide

## Main workflow

The desktop UI is built around two live panels:

- `Live Transcript`: partial speech, committed transcript, and explicit question
  detection
- `Assistant`: latest answer, summary, or commentary

If Soundmind recognizes a likely question, it shows a `Question detected` banner
above the transcript and relabels the answer action accordingly.

## Core controls

- `Start Capture`: resume local capture
- `Stop Capture`: pause local capture
- `Pause Cloud`: stop sending audio for STT / LLM processing
- `Resume Cloud`: resume cloud processing
- `Answer Last Question`: answer the detected or most recent question
- `Summarise Last Minute`: summarise recent transcript context
- `Comment Current Topic`: generate short commentary on the latest topic

## Settings

The UI stores settings locally in SQLite:

- default mode
- transcript retention period
- transcript storage enabled or disabled
- cloud auto-start enabled or disabled

Retention `0` means "keep history indefinitely."

## History and export

The session history panel lets you:

- inspect past sessions
- view stored transcript and assistant output
- export a session as JSON
- export a session as Markdown
- delete stored sessions
- purge sessions older than the configured retention period

## Privacy model

The UI always shows:

- capture state
- cloud state
- current sink and monitor source
- whether transcript storage is enabled
- whether local capture or cloud processing is paused

Local transcript storage can be disabled, but live cloud transcription still
requires sending audio to ElevenLabs while cloud processing is enabled.

## Tray and shortcuts

Tray menu:

- show or hide the window
- start or stop capture
- pause or resume cloud processing
- trigger answer, summary, and commentary actions
- quit the app

Default shortcuts:

- `Ctrl+Alt+Shift+M`: show or hide the window
- `Ctrl+Alt+Shift+A`: answer the detected or last question
- `Ctrl+Alt+Shift+S`: summarise the last minute
- `Ctrl+Alt+Shift+C`: comment on the current topic

If a shortcut is already claimed by another app, Soundmind logs it and continues
to run.
