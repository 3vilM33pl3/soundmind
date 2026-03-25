use chrono::{DateTime, Utc};
use stt_core::{FinalTranscript, PartialTranscript, TranscriberEvent};
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
            TranscriberEvent::PartialTranscript(mut partial) => {
                partial.text = sanitize_transcript_text(&partial.text);
                self.partial = if partial.text.is_empty() { None } else { Some(partial) };
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

    pub fn clear_partial(&mut self) {
        self.partial = None;
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
        self.committed.iter().rev().find(|segment| is_question_candidate(&segment.text)).cloned()
    }

    fn commit_final(
        &mut self,
        session_id: Uuid,
        mut final_transcript: FinalTranscript,
    ) -> Option<TranscriptSegment> {
        final_transcript.text = sanitize_transcript_text(&final_transcript.text);
        if final_transcript.text.is_empty() {
            return None;
        }

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

pub fn is_question_candidate(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.ends_with('?') {
        return true;
    }

    let normalized = trimmed
        .trim_matches(|character: char| character.is_ascii_punctuation() && character != '\'' && character != '-')
        .to_lowercase();

    if normalized.is_empty() {
        return false;
    }

    const QUESTION_PREFIXES: [&str; 26] = [
        "who ",
        "what ",
        "when ",
        "where ",
        "why ",
        "how ",
        "which ",
        "can ",
        "could ",
        "would ",
        "will ",
        "do ",
        "did ",
        "does ",
        "have ",
        "has ",
        "is ",
        "are ",
        "was ",
        "were ",
        "tell me ",
        "describe ",
        "explain ",
        "walk me through ",
        "talk me through ",
        "give me ",
    ];

    if QUESTION_PREFIXES.iter().any(|prefix| normalized.starts_with(prefix)) {
        return true;
    }

    const QUESTION_PHRASES: [&str; 13] = [
        "what's ",
        "who's ",
        "how's ",
        "can you ",
        "could you ",
        "would you ",
        "will you ",
        "do you ",
        "did you ",
        "have you ",
        "what do ",
        "what would ",
        "how would ",
    ];

    if QUESTION_PHRASES.iter().any(|phrase| normalized.starts_with(phrase)) {
        return true;
    }

    normalized.starts_with("if ")
}

fn sanitize_transcript_text(text: &str) -> String {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let mut cleaned = Vec::with_capacity(tokens.len());
    let mut index = 0;

    while index < tokens.len() {
        let token = tokens[index];

        if is_timestamp_token(token) {
            index += 1;
            continue;
        }

        if is_timestamp_arrow(token) {
            index += 1;
            continue;
        }

        if is_cue_number(token)
            && tokens.get(index + 1).is_some_and(|next| is_timestamp_token(next))
        {
            index += 1;
            continue;
        }

        cleaned.push(token);
        index += 1;
    }

    cleaned.join(" ").trim().trim_matches('-').trim().to_string()
}

fn is_cue_number(token: &str) -> bool {
    let normalized = token.trim_matches(|character: char| !character.is_ascii_digit());
    !normalized.is_empty()
        && normalized.len() <= 4
        && normalized.chars().all(|character| character.is_ascii_digit())
}

fn is_timestamp_arrow(token: &str) -> bool {
    matches!(token.trim(), "--" | "-->" | "->")
}

fn is_timestamp_token(token: &str) -> bool {
    let normalized = token.trim_matches(|character: char| {
        !character.is_ascii_alphanumeric()
            && character != ':'
            && character != ','
            && character != '.'
    });
    let Some((hours, rest)) = normalized.split_once(':') else {
        return false;
    };
    let Some((minutes, rest)) = rest.split_once(':') else {
        return false;
    };
    let Some((seconds, millis)) = rest.split_once(',').or_else(|| rest.split_once('.')) else {
        return false;
    };

    [hours, minutes, seconds]
        .into_iter()
        .all(|part| part.len() == 2 && part.chars().all(|character| character.is_ascii_digit()))
        && millis.len() == 3
        && millis.chars().all(|character| character.is_ascii_digit())
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
        assert!(is_question_candidate("Can someone explain this?"));
    }

    #[test]
    fn recognizes_question_phrasings_without_a_trailing_mark() {
        assert!(is_question_candidate("Tell me about yourself and what you've studied so far."));
        assert!(is_question_candidate("If requirements changed during the project, how would you handle that."));
        assert!(is_question_candidate("Walk me through your approach to monitoring."));
        assert!(is_question_candidate("What would you use inside Databricks besides PySpark when working directly with data"));
    }

    #[test]
    fn avoids_labeling_plain_statements_as_questions() {
        assert!(!is_question_candidate("I studied computer science and worked on a few data pipelines."));
        assert!(!is_question_candidate("This is a summary of the interview so far."));
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

    #[test]
    fn strips_subtitle_timestamp_artifacts_from_final_transcript() {
        let session_id = Uuid::new_v4();
        let mut state = TranscriptState::default();

        let segment = state.apply_event(
            session_id,
            TranscriberEvent::FinalTranscript(FinalTranscript {
                start_ms: 0,
                end_ms: 500,
                text: "This is obviously a shorter version 00:00:59,000 -- 00:01:01,000 for a real job interview.".to_string(),
                source: "mock".to_string(),
            }),
        );

        let segment = segment.expect("segment should survive cleanup");
        assert_eq!(segment.text, "This is obviously a shorter version for a real job interview.");
    }

    #[test]
    fn strips_cue_numbers_and_standalone_timestamps_from_partial_transcript() {
        let session_id = Uuid::new_v4();
        let mut state = TranscriptState::default();

        state.apply_event(
            session_id,
            TranscriberEvent::PartialTranscript(PartialTranscript {
                start_ms: 0,
                end_ms: 500,
                text: "Your typical interview 24 00:01:03,000 -- 00:01:04,000 lasts for about"
                    .to_string(),
                source: "mock".to_string(),
            }),
        );

        assert_eq!(state.partial_text(), Some("Your typical interview lasts for about"));
    }
}
