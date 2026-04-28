# Meeting Export Structure Spec

## Goal

Improve Soundmind exports so downstream AI assistants can reliably reconstruct meetings, identify uncertainty, and answer questions like:

- What happened yesterday?
- Who said this?
- Which calendar meeting was this?
- What are the action items, risks, and follow-ups?
- Was this one conversation or several stitched together?

The core problem is not only transcript quality. It is also structure, segmentation, identity, and metadata quality.

## Problem Statement

Current exports can contain multiple conversations stitched together with weak or missing structure. This causes downstream AI systems to:

- misidentify people
- confuse separate meetings
- invent continuity across unrelated segments
- fail to match transcript segments to calendar events
- produce incorrect summaries with too much confidence

We need exports that are meeting-shaped, segment-shaped, and confidence-aware.

## Requirements

### 1. Preserve real meeting boundaries

Exports should represent either:

- one true meeting/session, or
- a multi-segment artifact explicitly labeled as such

If multiple conversations are present, emit per-segment metadata:

- `segment_id`
- `segment_start_time`
- `segment_end_time`
- `segment_confidence`
- `segment_summary`

Never silently merge unrelated recordings into one flat transcript.

### 2. Add wall-clock timestamps

Relative transcript milliseconds are not enough.

For every session and segment include:

- `started_at` as ISO timestamp
- `ended_at` as ISO timestamp
- timezone
- duration
- per-utterance wall-clock timestamps if possible
- relative offsets if useful

This is necessary for calendar matching.

### 3. Add calendar linkage

For each session or segment, attempt calendar matching and emit:

- `calendar_match_status`: `exact | fuzzy | none`
- `calendar_event_id`
- `calendar_title`
- `calendar_start`
- `calendar_end`
- `calendar_attendees`
- `calendar_organizer`
- `calendar_match_confidence`

If no match exists, say so explicitly.

### 4. Improve speaker identity handling

For each speaker turn, store:

- stable `speaker_id`
- optional `speaker_name`
- `speaker_name_confidence`
- `speaker_source`: `calendar | voiceprint | manual | inferred | unknown`

Rules:

- if identity is uncertain, use `unknown`
- never emit a guessed name without confidence metadata
- support cross-session speaker consistency where possible

### 5. Add explicit confidence metadata

Every important inference should expose uncertainty.

Recommended fields:

- `segment_boundary_confidence`
- `speaker_identity_confidence`
- `calendar_match_confidence`
- `summary_confidence`
- `action_item_confidence`

### 6. Separate raw facts from inferred summaries

Do not blend direct transcript facts and inferred interpretation into one summary blob.

Export structure should distinguish:

- `raw_transcript`
- `segment_summaries`
- `factual_observations`
- `inferred_themes`
- `action_items`
- `decisions`
- `open_questions`
- `risks_or_ambiguities`

Rules:

- `factual_observations` must be directly grounded in transcript text
- `inferred_themes` may generalize, but must be marked as inference
- `risks_or_ambiguities` should explicitly call out unclear identities, stitched meetings, or uncertain interpretations

### 7. Add entity extraction

Extract structured entities from each session or segment.

Required entity types:

- people
- companies
- projects
- tickets
- repos
- tools/systems
- dates
- action items
- deadlines
- decisions
- blockers

Each entity should include:

- `type`
- `value`
- `source_span`
- `confidence`

### 8. Support user corrections

Users should be able to correct:

- speaker identity
- calendar event match
- segment boundaries
- summary mistakes
- extracted actions/entities

Corrections should be stored and optionally applied to:

- future exports
- same-speaker matching
- same-meeting matching
- entity normalization

### 9. Emit ambiguity-friendly exports

If the system is unsure, the export should say so clearly.

Instead of asserting a guessed identity, prefer outputs like:

- likely Toblerone delivery-lead intro
- `speaker_name: unknown`
- `calendar_match_status: fuzzy`
- `calendar_match_confidence: 0.42`

### 10. Optional audio evidence hooks

For ambiguous moments, keep the original evidence available.

Allow transcript chunks or speaker turns to reference:

- short audio clip URL/path
- waveform snippet ID
- confidence rationale

## Recommended Export Schema

```json
{
  "session_id": "string",
  "source_type": "live_call|manual_recording|imported_audio|resumed_session",
  "started_at": "ISO-8601",
  "ended_at": "ISO-8601",
  "timezone": "Europe/London",
  "duration_seconds": 1234,
  "calendar_match": {
    "status": "exact|fuzzy|none",
    "confidence": 0.0,
    "event_id": "string|null",
    "title": "string|null",
    "start": "ISO-8601|null",
    "end": "ISO-8601|null",
    "organizer": "string|null",
    "attendees": ["string"]
  },
  "segments": [
    {
      "segment_id": "string",
      "started_at": "ISO-8601",
      "ended_at": "ISO-8601",
      "boundary_confidence": 0.0,
      "summary": "string",
      "summary_confidence": 0.0,
      "participants": [
        {
          "speaker_id": "spk_1",
          "speaker_name": "string|null",
          "speaker_name_confidence": 0.0,
          "speaker_source": "calendar|voiceprint|manual|inferred|unknown"
        }
      ],
      "utterances": [
        {
          "speaker_id": "spk_1",
          "speaker_name": "string|null",
          "speaker_name_confidence": 0.0,
          "started_at": "ISO-8601",
          "ended_at": "ISO-8601",
          "relative_start_ms": 0,
          "relative_end_ms": 1200,
          "text": "string",
          "audio_clip_ref": "string|null"
        }
      ],
      "factual_observations": ["string"],
      "inferred_themes": ["string"],
      "action_items": [
        {
          "text": "string",
          "owner": "string|null",
          "due": "string|null",
          "confidence": 0.0
        }
      ],
      "decisions": ["string"],
      "open_questions": ["string"],
      "risks_or_ambiguities": ["string"],
      "entities": [
        {
          "type": "person|project|ticket|repo|tool|date|company|decision|blocker",
          "value": "string",
          "confidence": 0.0,
          "source_span": "string"
        }
      ]
    }
  ]
}
```

## ElevenLabs-Specific Opportunities

Since Soundmind already works with ElevenLabs, use that where helpful for:

- better speaker separation
- cleaner transcript punctuation and turn segmentation
- optional voiceprint-based speaker consistency across meetings
- short replayable audio snippets for uncertain sections

Do not overfit identity from voice alone. Always expose confidence.

## Product Behavior Rules

### Hard rules

- never silently merge separate conversations
- never assign a person name without confidence
- never present inferred summaries as raw facts
- never hide uncertainty when meeting matching fails

### Preferred behavior

- be explicit
- be structured
- be correction-friendly
- optimize for downstream auditability, not only pretty summaries

## Prioritization

### Phase 1

1. true session/segment boundaries
2. wall-clock timestamps
3. calendar matching
4. confidence-aware speaker labeling

### Phase 2

5. factual vs inferred summary separation
6. entity/action extraction
7. user correction loop

### Phase 3

8. audio clip references
9. persistent speaker identity refinement
10. cross-session memory improvements

## Acceptance Tests

### Test 1: stitched export detection

Input:
- one audio file containing three separate conversations

Expected:
- output contains three segments
- each segment has independent summary and boundary confidence
- no single blended summary

### Test 2: calendar alignment

Input:
- transcript from a meeting that exists in calendar

Expected:
- matched calendar event title, attendees, start/end, confidence

### Test 3: uncertain speaker

Input:
- diarized speaker with no reliable identity

Expected:
- `speaker_name = null` or `unknown`
- nonzero but low confidence
- no invented person name

### Test 4: correction persistence

Input:
- user corrects speaker or meeting identity

Expected:
- corrected metadata stored
- future exports reuse correction where appropriate

### Test 5: downstream summary safety

Input:
- mixed-confidence transcript

Expected:
- factual observations remain conservative
- inferred themes are marked as inference
- ambiguities are surfaced explicitly

## Desired Outcome

After this change, a downstream AI assistant should be able to:

- accurately summarize a day of meetings
- map transcript segments to real calendar events
- identify uncertainty instead of hallucinating names
- extract action items and project signals safely
- improve over time through corrections
