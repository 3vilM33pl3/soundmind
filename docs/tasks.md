# Tasks: Ubuntu System-Audio Assistant

## 1. Overview

This file breaks the project into implementable work packages for a Rust-based Ubuntu desktop app that:

- captures system audio output,
- transcribes it in real time with ElevenLabs Scribe Realtime,
- uses OpenAI for commentary and question answering,
- stores sessions locally,
- exposes a small desktop UI.

The goal is to provide a practical build order for Codex or another coding agent.

---

## 2. Project phases

## Phase 0 — repository and scaffolding
Goal: establish workspace, standards, and baseline project shape.

### Tasks
- Create Rust workspace.
- Add crates:
  - `audio_capture`
  - `audio_pipeline`
  - `stt_scribe`
  - `transcript_core`
  - `context_engine`
  - `policy_engine`
  - `llm_openai`
  - `storage_sqlite`
  - `app_backend`
  - `app_ui`
  - `ipc_schema`
- Configure linting:
  - `rustfmt`
  - `clippy`
- Add logging and config dependencies.
- Add root README.
- Add `.env.example`.
- Add `config.example.toml`.
- Add license and contribution notes if desired.

### Deliverables
- Compiling empty workspace
- Shared config loading
- Shared error type strategy
- CI check for build + clippy + fmt

---

## Phase 1 — audio capture MVP
Goal: reliably capture Ubuntu system audio output from the default sink monitor.

### Tasks
#### 1.1 Capture backend abstraction
- Define `AudioSource` trait.
- Define types:
  - `AudioFrame`
  - `AudioFormat`
  - `CaptureDevice`
  - `CaptureEvent`
- Add unit tests for type conversions and timestamp handling.

#### 1.2 PulseAudio-compatible backend
- Implement backend discovery for default sink.
- Resolve matching monitor source.
- Start capture stream.
- Emit raw frames with timestamps.
- Emit device-change events.

#### 1.3 Device switching
- Detect default output device changes.
- Rebind capture stream without app restart.
- Handle disconnect/reconnect cases.

#### 1.4 Diagnostics
- CLI or debug log showing:
  - current sink,
  - current monitor source,
  - frame rate,
  - error states.

### Deliverables
- Running backend that captures system output
- Logs confirm active device and frame flow
- Survives switching headphones/speakers

### Acceptance criteria
- Can capture speech from browser or media player without app-specific integration
- Capture continues or reconnects after output device switch
- No crash when sink disappears temporarily

---

## Phase 2 — audio pipeline
Goal: transform raw captured audio into STT-friendly chunks.

### Tasks
#### 2.1 Format normalization
- Downmix stereo to mono.
- Resample to 16 kHz.
- Standardize sample format.

#### 2.2 Frame buffering
- Implement rolling frame buffer.
- Group frames into chunk packets.
- Preserve start/end timestamps.

#### 2.3 Voice activity detection
- Add simple energy gate first.
- Add pluggable VAD interface for future improvement.
- Suppress long silence segments.

#### 2.4 Pipeline testing
- Feed prerecorded PCM/WAV fixtures.
- Verify chunk timing and ordering.
- Verify no duplicate chunk emission.

### Deliverables
- `audio_pipeline` crate producing `AudioChunk`
- Test fixtures and pipeline tests

### Acceptance criteria
- Chunks are correctly timestamped
- Silence suppression works
- Pipeline latency remains low enough for realtime use

---

## Phase 3 — ElevenLabs Scribe integration
Goal: stream chunks to ElevenLabs Scribe Realtime and receive transcript events.

### Tasks
#### 3.1 STT trait and event model
- Define `Transcriber` trait.
- Define:
  - `PartialTranscript`
  - `FinalTranscript`
  - `TranscriberEvent`
  - `TranscriberHealth`

#### 3.2 WebSocket client
- Implement authenticated connection.
- Send realtime audio chunks.
- Receive provider events.
- Parse partial and final transcript payloads.

#### 3.3 Resilience
- Add reconnect logic.
- Add heartbeat/health status if useful.
- Backoff on transient errors.
- Surface fatal auth/config errors clearly.

#### 3.4 Local simulation mode
- Add mock or replay STT adapter for development.
- Feed canned transcript events without provider dependency.

### Deliverables
- Working `stt_scribe` crate
- Mock adapter for tests/dev
- Provider error mapping

### Acceptance criteria
- Partial transcript events appear within a reasonable delay
- Final transcript segments are received and parsed
- Temporary disconnects do not require full app restart

---

## Phase 4 — transcript core
Goal: create stable transcript state from partial/final STT events.

### Tasks
#### 4.1 Partial/final handling
- Show partial transcript separately from committed transcript.
- Commit only final transcript segments to stable history.
- Clear/update partial state appropriately.

#### 4.2 Segment model
- Add transcript segment IDs.
- Store timestamps, source, session ID.
- Support retrieval of recent transcript windows.

#### 4.3 Merge logic
- Merge adjacent small final segments when appropriate.
- Avoid duplicate final lines.
- Preserve original ordering.

#### 4.4 Rolling context windows
- Add helper APIs:
  - `last_n_seconds`
  - `last_n_segments`
  - `last_question_candidate`

### Deliverables
- `transcript_core` crate
- Stable transcript timeline
- Query helpers for recent context

### Acceptance criteria
- Transcript display does not flicker excessively
- Final transcript is coherent and not duplicated
- Recent windows can be assembled reliably

---

## Phase 5 — SQLite storage
Goal: persist sessions, transcript segments, assistant events, and settings.

### Tasks
#### 5.1 Database schema
- Create schema migrations.
- Tables:
  - `sessions`
  - `transcript_segments`
  - `assistant_events`
  - `settings`
  - `audit_events`

#### 5.2 Repository layer
- Insert session start/stop.
- Insert committed transcript segments.
- Insert assistant outputs.
- Read recent sessions and transcript history.

#### 5.3 Retention controls
- Add configurable retention policy.
- Add delete session operation.
- Add purge old sessions operation.

### Deliverables
- `storage_sqlite` crate
- Migrations
- Repository tests

### Acceptance criteria
- New sessions and transcript segments are persisted
- Data can be queried for recent session playback/export
- Purge operations do not corrupt database

---

## Phase 6 — OpenAI integration
Goal: generate answers, commentary, and summaries from recent transcript windows.

### Tasks
#### 6.1 LLM adapter
- Implement OpenAI client wrapper.
- Support configuration via API key and model name.
- Define request/response types.

#### 6.2 Structured output layer
- Define internal schema for:
  - answer response
  - commentary response
  - summary response
- Parse and validate model output.

#### 6.3 Prompt templates
- Implement prompts for:
  - `answer_question`
  - `commentary`
  - `summarise_recent`
- Keep prompts concise and deterministic.

#### 6.4 Error handling
- Handle timeouts, auth failures, malformed output.
- Return user-safe fallback errors.

### Deliverables
- `llm_openai` crate
- Prompt builders
- Structured output parsing

### Acceptance criteria
- App can answer a manually selected recent question
- App can summarise recent transcript window
- Failure to reach OpenAI does not break transcription flow

---

## Phase 7 — context engine and policy engine
Goal: decide when the assistant should respond.

### Tasks
#### 7.1 Context assembly
- Build helper functions to extract:
  - recent transcript window,
  - last likely question,
  - recent assistant outputs.

#### 7.2 Manual actions
- Implement:
  - answer last question
  - summarise last minute
  - comment on current topic

#### 7.3 Question detection
- Implement simple heuristics first:
  - question marks from transcript,
  - interrogative openings,
  - phrases like “can someone”, “how do”, “what is”.
- Keep heuristic module replaceable.

#### 7.4 Rate limiting and suppression
- Throttle assistant outputs.
- Suppress repeated commentary.
- Respect privacy pause and cloud pause.

### Deliverables
- `context_engine` crate
- `policy_engine` crate
- Manual and automatic trigger logic

### Acceptance criteria
- Manual actions work reliably
- Auto-answer can be enabled without excessive spam
- Policy state is inspectable in logs/debug view

---

## Phase 8 — backend orchestration
Goal: compose all backend services into one working runtime.

### Tasks
#### 8.1 Event bus
- Define shared event types in `ipc_schema`.
- Wire modules together via channels.
- Ensure backpressure is handled.

#### 8.2 Session lifecycle
- Start session on capture begin.
- Stop session on user action/app shutdown.
- Persist audit events.

#### 8.3 Configuration
- Load config from file and env.
- Validate required provider credentials.
- Expose runtime mode settings.

#### 8.4 Logging
- Add structured logs.
- Log provider states, policy decisions, session lifecycle.

### Deliverables
- `app_backend` crate
- Single running process with all services connected

### Acceptance criteria
- End-to-end backend works without UI
- Logs clearly show capture -> STT -> transcript -> LLM flow

---

## Phase 9 — desktop UI
Goal: provide a small usable desktop interface.

### Tasks
#### 9.1 Base window
- Create compact UI with:
  - status indicator,
  - current sink,
  - live transcript pane,
  - latest assistant output pane.

#### 9.2 Controls
- Buttons:
  - Start/Stop
  - Pause cloud
  - Answer last question
  - Summarise last minute

#### 9.3 Error surface
- Show non-blocking error messages for:
  - missing API keys,
  - provider disconnects,
  - paused capture.

#### 9.4 Tray integration
- Add tray icon if framework supports it well.
- Add quick actions from tray menu.

### Deliverables
- `app_ui` crate
- Working desktop app attached to backend

### Acceptance criteria
- User can see live transcript
- User can trigger answer/summary actions
- User can clearly tell when cloud processing is active

---

## Phase 10 — hotkeys and ergonomics
Goal: improve usability without changing core architecture.

### Tasks
- Add global hotkeys:
  - answer last question
  - summarise recent audio
  - pause/resume
- Add copy transcript action.
- Add export session action.
- Add session list/history view.

### Deliverables
- Hotkey support
- Transcript export to markdown/json

### Acceptance criteria
- Frequent actions can be performed without opening full UI
- Transcript history is reusable

---

## Phase 11 — privacy and safety controls
Goal: make the app trustworthy to use.

### Tasks
- Add obvious capture indicator.
- Add explicit cloud-processing indicator.
- Add privacy pause mode.
- Add setting to disable automatic answering.
- Add setting to disable storage or shorten retention.
- Add warning text in onboarding/settings.

### Deliverables
- Privacy controls in UI and config
- Audit logging for start/stop/pause

### Acceptance criteria
- User can stop or pause capture immediately
- User can tell whether data is being sent to providers
- Defaults are conservative

---

## Phase 12 — optional TTS
Goal: let the assistant speak replies aloud.

### Tasks
- Create `tts_openai` crate.
- Add text-to-speech adapter.
- Add playback controls and mute.
- Add setting for speak-on-answer.
- Ensure TTS audio is not recursively re-captured where possible.

### Deliverables
- Spoken assistant output
- Mute and off-by-default policy

### Acceptance criteria
- Spoken reply works when enabled
- TTS can be muted instantly
- TTS does not create runaway feedback loops

---

## 3. Cross-cutting engineering tasks

## 3.1 Testing
- Unit tests for:
  - resampling helpers,
  - transcript merging,
  - policy rules,
  - schema validation.
- Integration tests for:
  - mock audio -> mock STT -> transcript,
  - transcript -> OpenAI response parsing.
- Manual end-to-end test scripts.

## 3.2 Error model
- Define shared error categories:
  - config,
  - audio,
  - STT provider,
  - LLM provider,
  - storage,
  - UI.
- Ensure user-facing messages are clean and non-technical.

## 3.3 Documentation
- Add root README.
- Add setup guide:
  - Ubuntu packages needed,
  - API key setup,
  - first run instructions.
- Add troubleshooting guide.

## 3.4 CI/CD
- Build and test on Ubuntu.
- Run fmt/clippy/tests.
- Optionally build `.deb` artifact in CI.

---

## 4. Suggested issue breakdown

## Epic A — audio capture
- A1 define capture abstractions
- A2 implement PulseAudio-compatible backend
- A3 device switch handling
- A4 capture diagnostics

## Epic B — audio pipeline
- B1 normalization
- B2 chunking
- B3 silence suppression/VAD
- B4 pipeline tests

## Epic C — STT
- C1 transcriber trait
- C2 ElevenLabs realtime adapter
- C3 reconnect logic
- C4 mock adapter

## Epic D — transcript
- D1 partial/final state
- D2 segment model
- D3 merge logic
- D4 rolling context API

## Epic E — storage
- E1 schema
- E2 repositories
- E3 retention/purge
- E4 export

## Epic F — reasoning
- F1 OpenAI adapter
- F2 structured outputs
- F3 prompts
- F4 fallback handling

## Epic G — policy
- G1 manual actions
- G2 question detection
- G3 rate limiting
- G4 assisted mode

## Epic H — UI
- H1 base window
- H2 controls
- H3 tray
- H4 settings page

## Epic I — privacy/safety
- I1 indicators
- I2 pause modes
- I3 retention controls
- I4 onboarding copy

## Epic J — packaging
- J1 systemd user service
- J2 desktop entry
- J3 `.deb` packaging
- J4 installation docs

---

## 5. Recommended implementation order

1. Repository scaffolding
2. Audio capture
3. Audio pipeline
4. STT integration
5. Transcript core
6. Minimal terminal/debug UI
7. SQLite persistence
8. OpenAI manual actions
9. Desktop UI
10. Policy engine
11. Privacy controls
12. Optional TTS
13. Packaging

This order minimizes wasted effort and gets to a testable transcript flow early.

---

## 6. Definition of MVP complete

The MVP is complete when all of the following are true:

- The app captures system audio from Ubuntu output independently of any single app.
- The app shows a live transcript from ElevenLabs Scribe Realtime.
- The app stores transcript sessions locally in SQLite.
- The user can press a button or hotkey to answer the last detected question using OpenAI.
- The user can request a summary of recent audio using OpenAI.
- The UI shows clear capture/cloud/privacy state.
- The app can be installed and run on Ubuntu without manual developer-only steps.

---

## 7. Nice-to-have backlog after MVP

- native PipeWire backend
- local/offline STT backend
- semantic transcript search
- richer meeting mode
- configurable prompt profiles
- speaker attribution improvements
- redaction support
- session replay
- plugin architecture

---

## 8. Notes for Codex

Implementation guidance:

- Prefer small focused crates.
- Keep provider integrations behind traits.
- Prefer typed events over ad hoc string messages.
- Keep prompt code isolated from UI code.
- Make privacy and pause behaviour first-class, not bolted on.
- Add mock backends early so core logic can be tested without live provider calls.
- Optimize for correctness and clarity before latency micro-optimizations.

The most important first milestone is:

**system audio capture -> realtime transcript visible on screen**

Everything else builds on that.
