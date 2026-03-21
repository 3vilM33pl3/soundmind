use chrono::{DateTime, Utc};
use stt_scribe::{FinalTranscript, PartialTranscript, TranscriberEvent};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptSegment {
    pub id: Uuid,
    pub session_id: Uuid,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct TranscriptState {
    partial: Option<PartialTranscript>,
    committed: Vec<TranscriptSegment>,
}

impl TranscriptState {
    pub fn apply_event(
        &mut self,
        session_id: Uuid,
        event: TranscriberEvent,
    ) -> Option<TranscriptSegment> {
        match event {
            TranscriberEvent::PartialTranscript(partial) => {
                self.partial = Some(partial);
                None
            }
            TranscriberEvent::FinalTranscript(final_transcript) => {
                self.partial = None;
                self.commit_final(session_id, final_transcript)
            }
            TranscriberEvent::Error(_) | TranscriberEvent::Health(_) => None,
        }
    }

    pub fn partial_text(&self) -> Option<&str> {
        self.partial.as_ref().map(|partial| partial.text.as_str())
    }

    pub fn segments(&self) -> &[TranscriptSegment] {
        &self.committed
    }

    pub fn last_n_seconds(&self, seconds: u64) -> Vec<TranscriptSegment> {
        let newest_end = self.committed.last().map(|segment| segment.end_ms).unwrap_or_default();
        let threshold = newest_end.saturating_sub(seconds * 1_000);
        self.committed.iter().filter(|segment| segment.end_ms >= threshold).cloned().collect()
    }

    pub fn last_n_segments(&self, count: usize) -> Vec<TranscriptSegment> {
        let start = self.committed.len().saturating_sub(count);
        self.committed[start..].to_vec()
    }

    pub fn last_question_candidate(&self) -> Option<TranscriptSegment> {
        self.committed.iter().rev().find(|segment| looks_like_question(&segment.text)).cloned()
    }

    fn commit_final(
        &mut self,
        session_id: Uuid,
        final_transcript: FinalTranscript,
    ) -> Option<TranscriptSegment> {
        if let Some(last) = self.committed.last() {
            if last.text == final_transcript.text {
                return None;
            }
        }

        let segment = TranscriptSegment {
            id: Uuid::new_v4(),
            session_id,
            start_ms: final_transcript.start_ms,
            end_ms: final_transcript.end_ms,
            text: final_transcript.text,
            source: final_transcript.source,
            created_at: Utc::now(),
        };

        self.committed.push(segment.clone());
        Some(segment)
    }
}

fn looks_like_question(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.ends_with('?')
        || ["who", "what", "when", "where", "why", "how", "can", "should"]
            .iter()
            .any(|prefix| trimmed.to_lowercase().starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_question_candidates() {
        let session_id = Uuid::new_v4();
        let mut state = TranscriptState::default();
        state.apply_event(
            session_id,
            TranscriberEvent::FinalTranscript(FinalTranscript {
                start_ms: 0,
                end_ms: 500,
                text: "Can someone explain this?".to_string(),
                source: "mock".to_string(),
            }),
        );

        assert!(state.last_question_candidate().is_some());
    }

    #[test]
    fn ignores_duplicate_final_segments() {
        let session_id = Uuid::new_v4();
        let mut state = TranscriptState::default();

        let first = state.apply_event(
            session_id,
            TranscriberEvent::FinalTranscript(FinalTranscript {
                start_ms: 0,
                end_ms: 500,
                text: "Repeated line".to_string(),
                source: "mock".to_string(),
            }),
        );
        let duplicate = state.apply_event(
            session_id,
            TranscriberEvent::FinalTranscript(FinalTranscript {
                start_ms: 500,
                end_ms: 1000,
                text: "Repeated line".to_string(),
                source: "mock".to_string(),
            }),
        );

        assert!(first.is_some());
        assert!(duplicate.is_none());
        assert_eq!(state.segments().len(), 1);
    }
}
