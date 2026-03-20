# Architecture: Ubuntu System-Audio Assistant

## 1. Purpose

Build a desktop application for Ubuntu that:

- captures system audio output independently of any single application,
- transcribes speech from that audio in near real time using ElevenLabs Scribe Realtime,
- uses OpenAI to comment on what is being said and answer detected or user-triggered questions,
- stores transcripts and assistant outputs locally,
- gives the user clear privacy controls and visible recording state.

This is **not** a browser extension and **not** tied to Zoom, Chrome, Teams, VLC, or any other specific application.

---

## 2. Product goals

### Primary goals
- Capture all speech audible through the current output device.
- Provide low-latency live transcription.
- Let the user ask for:
  - “What was just said?”
  - “Answer the last question.”
  - “Summarise the last minute.”
- Optionally auto-detect likely questions and answer them.
- Run as a normal Ubuntu desktop application with a small UI.

### Non-goals for v1
- Perfect speaker diarization.
- Perfect separation of speech from music.
- Kernel-level audio capture.
- Deep app-specific integrations.
- Autonomous voice agent behaviour without user control.

---

## 3. High-level architecture

```text
+----------------------+
| Ubuntu Audio Output  |
| (PipeWire/Pulse)     |
+----------+-----------+
           |
           v
+----------------------+
| Audio Capture        |
| Sink monitor source  |
+----------+-----------+
           |
           v
+----------------------+
| Audio Pipeline       |
| resample, mono, VAD, |
| chunking, buffering  |
+----------+-----------+
           |
           v
+----------------------+
| STT Adapter          |
| ElevenLabs Scribe RT |
+----------+-----------+
           |
           v
+----------------------+
| Transcript Core      |
| partial/final merge  |
| rolling transcript   |
+----------+-----------+
           |
           +----------------------+
           |                      |
           v                      v
+----------------------+  +----------------------+
| Context / Policy     |  | Local Storage        |
| detect question,     |  | SQLite               |
| rate limit, privacy  |  | sessions/transcript  |
+----------+-----------+  +----------------------+
           |
           v
+----------------------+
| LLM Adapter          |
| OpenAI Responses API |
+----------+-----------+
           |
           v
+----------------------+
| UI / Overlay / Tray  |
| transcript + answers |
+----------------------+
```

---

## 4. Runtime components

## 4.1 Backend daemon

A Rust backend daemon is responsible for:

- audio capture,
- audio preprocessing,
- STT streaming,
- transcript state,
- policy enforcement,
- OpenAI requests,
- persistence.

Suggested binary name:

- `soundmindd`

This should run as a user service via `systemd --user`.

## 4.2 Desktop UI

A small desktop UI attaches to the backend and shows:

- current capture status,
- current sink/output device,
- live transcript,
- latest answer/comment,
- controls for pause/resume,
- manual actions such as “answer last question”.

Suggested binary name:

- `soundmind-ui`

---

## 5. Audio capture design

## 5.1 Capture source

The app captures audio from the current default output sink’s **monitor source**.

Rationale:
- this makes it application-independent,
- it works at the desktop audio stack level,
- it captures what the user actually hears.

## 5.2 Requirements

The capture layer must:

- discover the default sink,
- find the corresponding monitor source,
- reconnect when the default sink changes,
- survive Bluetooth/headphone output changes,
- expose capture state events to the rest of the system.

## 5.3 Backend strategy

### Phase 1
Use the PulseAudio-compatible API, which should work on Ubuntu systems using PulseAudio directly or PipeWire’s compatibility layer.

### Phase 2
Optionally add a native PipeWire backend later.

## 5.4 Audio format

Normalize captured audio to a single internal format:

- PCM float or signed 16-bit internally,
- mono,
- 16 kHz target sample rate for the STT path.

Maintain timestamps for every frame/block.

---

## 6. Audio pipeline

## 6.1 Preprocessing steps

1. Read audio frames from capture backend.
2. Downmix stereo to mono.
3. Resample to target sample rate.
4. Run a simple energy gate and/or VAD.
5. Buffer into small chunks for streaming.
6. Tag silence/music-heavy periods where possible.

## 6.2 Design goals

- low latency,
- stable timestamps,
- no duplicated chunk emission,
- minimal CPU overhead,
- graceful handling of silence.

## 6.3 Chunk sizing

Suggested approach:

- small frame size internally, e.g. 20 ms,
- streaming packet grouping around 100–250 ms,
- rolling context buffer of 30–90 seconds.

---

## 7. STT subsystem

## 7.1 Provider

Primary STT provider:
- ElevenLabs Scribe Realtime

## 7.2 Responsibilities

The STT adapter must:

- open and maintain a realtime session,
- stream prepared audio chunks,
- receive partial transcript events,
- receive final transcript events,
- expose provider errors and reconnect logic,
- surface latency and health metrics.

## 7.3 Interface

Define a provider trait:

```rust
pub trait Transcriber {
    fn start(&mut self) -> Result<()>;
    fn push_audio(&mut self, chunk: AudioChunk) -> Result<()>;
    fn poll_event(&mut self) -> Option<TranscriberEvent>;
    fn stop(&mut self) -> Result<()>;
}
```

This allows future support for:
- local Whisper/faster-whisper,
- OpenAI transcription,
- offline backends.

## 7.4 STT events

Core event types:

```rust
enum TranscriberEvent {
    PartialTranscript(PartialTranscript),
    FinalTranscript(FinalTranscript),
    Error(TranscriberError),
    Health(TranscriberHealth),
}
```

---

## 8. Transcript core

## 8.1 Responsibilities

The transcript subsystem converts unstable provider events into a coherent timeline.

It must:

- display partial text separately from committed text,
- merge final segments into a stable transcript,
- attach timestamps,
- maintain a rolling context window,
- persist committed transcript segments.

## 8.2 Data model

Example transcript segment:

```json
{
  "id": "seg_001",
  "session_id": "sess_001",
  "start_ms": 124000,
  "end_ms": 126400,
  "text": "Can someone explain what BGP does?",
  "is_final": true,
  "source": "elevenlabs_scribe_realtime"
}
```

## 8.3 Rolling windows

Maintain:
- recent transcript window: last 30–90 seconds,
- recent question candidate window,
- optional summary window for the last N minutes.

---

## 9. Context and policy engine

## 9.1 Purpose

This layer decides whether the app should:

- stay quiet,
- provide a short commentary,
- answer a detected question,
- summarise a recent window.

Without this layer the app becomes noisy and annoying.

## 9.2 Inputs

- committed transcript segments,
- partial transcripts if useful,
- manual UI actions,
- hotkey actions,
- privacy mode,
- model/backend health.

## 9.3 Decision rules

The first version should use explicit policy rules rather than a fully learned controller.

### Initial rules
- Never answer more than once every N seconds.
- Never answer when cloud processing is paused.
- Prefer user-triggered actions over automatic responses.
- Only auto-answer if transcript looks like a question.
- Suppress responses when transcript confidence is low.
- Avoid repeated commentary on similar consecutive segments.

## 9.4 Modes

Support these modes:

### Captions only
- transcription only,
- no OpenAI reasoning.

### Manual QA
- transcript continuously,
- user clicks or hotkeys “answer last question”.

### Assisted mode
- transcript continuously,
- app may auto-answer likely questions with throttling.

### Summary mode
- transcript continuously,
- user can request summaries of recent windows.

---

## 10. OpenAI reasoning subsystem

## 10.1 Provider

Primary LLM provider:
- OpenAI

## 10.2 Responsibilities

This subsystem:

- receives recent transcript windows,
- builds prompt payloads,
- requests structured output,
- returns commentary, answers, and summaries.

## 10.3 Why structured outputs

The app should not rely on unconstrained prose. It needs predictable output such as:

- whether to respond,
- response mode,
- answer text,
- confidence,
- whether speech output should occur.

Example internal schema:

```json
{
  "mode": "answer_question",
  "should_respond": true,
  "answer": "BGP is the routing protocol used between autonomous systems to exchange reachability information.",
  "confidence": 0.86,
  "speak_answer": false
}
```

## 10.4 Prompt templates

### Template: answer question
Inputs:
- recent transcript segments,
- detected question text,
- style constraints.

Instruction goals:
- concise,
- accurate,
- clear,
- acknowledge ambiguity when necessary.

### Template: commentary
Inputs:
- recent transcript context,
- last assistant output,
- rate-limit status.

Instruction goals:
- one brief useful observation only,
- no filler,
- no repetition.

### Template: summary
Inputs:
- recent transcript window,
- output style requested by user.

Instruction goals:
- concise summary,
- optionally action items,
- key points only.

---

## 11. Optional speech output

## 11.1 Scope

Optional feature for later phases.

The app may speak assistant answers aloud using TTS.

## 11.2 Provider

Use OpenAI TTS for generated spoken replies.

## 11.3 Policy

Speech output must be:
- off by default,
- explicitly user-enabled,
- easy to mute immediately.

---

## 12. Local storage

## 12.1 Storage engine

Use SQLite for v1.

Reasons:
- zero external infrastructure,
- easy packaging,
- enough for single-user local history,
- easy export.

## 12.2 Core tables

### `sessions`
Tracks each listening session.

Fields:
- `id`
- `started_at`
- `ended_at`
- `capture_device`
- `mode`
- `privacy_flags`

### `transcript_segments`
Committed transcript entries.

Fields:
- `id`
- `session_id`
- `start_ms`
- `end_ms`
- `text`
- `source`
- `created_at`

### `assistant_events`
Assistant answers/commentary/summaries.

Fields:
- `id`
- `session_id`
- `kind`
- `input_window_ref`
- `content`
- `confidence`
- `created_at`

### `settings`
User preferences and runtime config.

### `audit_events`
Capture start/stop, pause, provider errors, privacy toggles.

---

## 13. UI design

## 13.1 MVP UI

A compact desktop window or tray panel with:

- status indicator,
- current output device,
- live transcript area,
- current assistant output,
- action buttons:
  - start/stop,
  - pause cloud,
  - answer last question,
  - summarise last minute.

## 13.2 Important states

UI must visibly distinguish:
- idle,
- capturing locally,
- sending to STT provider,
- sending transcript to OpenAI,
- paused,
- error state.

## 13.3 UX priorities

- obvious privacy status,
- quick manual controls,
- readable transcript,
- no intrusive notifications by default.

---

## 14. Security and privacy

## 14.1 Risks

Because the app captures system audio and uses cloud services, it may process:
- meetings,
- videos,
- personal conversations,
- copyrighted audio.

## 14.2 Required controls

The application must provide:

- visible recording/capture indicator,
- one-click pause,
- one-click stop,
- explicit cloud-processing status,
- configurable local retention,
- optional transcript deletion.

## 14.3 Recommended defaults

- manual start by default,
- cloud processing visible at all times,
- assistant speech disabled by default,
- auto-answer disabled by default.

---

## 15. Packaging and deployment

## 15.1 Ubuntu packaging target

Primary packaging target:
- Debian package (`.deb`)

Install components:
- backend daemon binary,
- UI binary,
- desktop entry,
- icon assets,
- systemd user unit files,
- config file template.

## 15.2 Runtime model

The backend should run under the current user via `systemd --user`.

Benefits:
- starts with login,
- integrates with desktop session,
- no root daemon required,
- cleaner per-user state and permissions.

---

## 16. Configuration

Provide config via:

- `config.toml` file,
- environment variables for secrets,
- UI settings persisted to SQLite and/or config file.

Example settings:
- ElevenLabs API key,
- OpenAI API key,
- capture mode,
- auto-answer enabled,
- TTS enabled,
- retention days,
- hotkeys.

---

## 17. Internal module layout

Suggested Rust workspace:

```text
soundmind/
  Cargo.toml
  crates/
    audio_capture/
    audio_pipeline/
    stt_scribe/
    transcript_core/
    context_engine/
    policy_engine/
    llm_openai/
    tts_openai/
    storage_sqlite/
    ipc_schema/
    app_backend/
    app_ui/
```

### Module responsibilities

- `audio_capture`: sink monitor capture and device events
- `audio_pipeline`: resampling, VAD, chunking
- `stt_scribe`: ElevenLabs realtime adapter
- `transcript_core`: transcript merge and rolling windows
- `context_engine`: recent context assembly
- `policy_engine`: trigger logic and rate limiting
- `llm_openai`: OpenAI integration
- `tts_openai`: optional speech output
- `storage_sqlite`: DB schema and persistence
- `ipc_schema`: event/data contracts
- `app_backend`: backend composition
- `app_ui`: Tauri or desktop frontend

---

## 18. Event-driven design

Use typed internal events to decouple modules.

Example event set:

```rust
enum AppEvent {
    AudioFrameCaptured(AudioFrame),
    AudioChunkReady(AudioChunk),
    SpeechActivityChanged(SpeechState),
    SttPartialReceived(PartialTranscript),
    SttFinalReceived(FinalTranscript),
    TranscriptCommitted(TranscriptSegment),
    QuestionDetected(QuestionCandidate),
    UserAction(UserAction),
    LlmRequestStarted(LlmRequestMeta),
    LlmResponseReceived(LlmResponse),
    UiNotification(UiNotification),
    ProviderError(AppError),
}
```

Benefits:
- easier testing,
- clearer logging,
- easier replacement of providers later.

---

## 19. Observability

The app should log:

- provider connection state,
- audio device changes,
- STT latency,
- OpenAI request latency,
- transcript commit counts,
- dropped/retried requests,
- policy decisions.

Prefer structured logs.

For debug builds, expose a developer panel with:
- current sink,
- current session ID,
- chunk timings,
- last provider error.

---

## 20. Failure handling

## 20.1 Audio failures
- no sink available,
- monitor source disappears,
- format negotiation fails.

Fallback:
- surface error,
- retry discovery,
- do not crash the UI.

## 20.2 STT failures
- websocket disconnect,
- auth failure,
- rate limit,
- malformed event.

Fallback:
- exponential backoff,
- preserve local capture state,
- allow reconnection without restarting app.

## 20.3 OpenAI failures
- auth failure,
- network timeout,
- malformed response,
- structured output parse failure.

Fallback:
- show transcript only,
- keep app usable,
- surface non-blocking UI error.

---

## 21. MVP scope

## 21.1 Must have
- system audio capture,
- live transcript,
- local persistence,
- “answer last question” action,
- “summarise last minute” action,
- privacy indicator.

## 21.2 Should have
- tray UI,
- hotkeys,
- pause cloud processing,
- reconnect on device change.

## 21.3 Later
- auto-answer,
- TTS replies,
- native PipeWire backend,
- local STT backend,
- transcript search,
- semantic memory.

---

## 22. Main architecture decisions

1. **Capture at system audio sink monitor level**
   - application-independent
   - no browser coupling

2. **Use ElevenLabs for realtime STT**
   - specialized transcription layer
   - keeps OpenAI focused on reasoning

3. **Use OpenAI for structured reasoning**
   - answers, commentary, summaries
   - predictable app behaviour

4. **Use SQLite locally**
   - simple, fast, package-friendly

5. **Use a user-level backend service**
   - good Ubuntu desktop fit
   - no root requirement

6. **Default to manual and privacy-conscious behaviour**
   - avoid intrusive automation
   - safer first-run experience

---

## 23. Future extension points

- local/offline transcription backend
- app-specific audio filters
- semantic transcript search
- meeting mode with action-item extraction
- user memory integration
- export to markdown/json
- plugin prompt packs

---

## 24. Final recommendation

Build v1 as a **quiet assistant** rather than an autonomous companion:

- capture system audio,
- transcribe reliably,
- let the user trigger answers and summaries,
- keep cloud usage visible,
- store everything locally.

That gives the best balance of usefulness, privacy, and implementation complexity.
