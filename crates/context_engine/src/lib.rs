use transcript_core::{TranscriptSegment, TranscriptState};

pub fn recent_transcript_window(state: &TranscriptState, seconds: u64) -> Vec<TranscriptSegment> {
    state.last_n_seconds(seconds)
}

pub fn last_question_candidate(state: &TranscriptState) -> Option<TranscriptSegment> {
    state.last_question_candidate()
}
