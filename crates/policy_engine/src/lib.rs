use chrono::{DateTime, Duration, Utc};
use ipc_schema::AppMode;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PolicyState {
    pub mode: AppMode,
    pub cloud_paused: bool,
    pub last_response_at: Option<DateTime<Utc>>,
    pub min_response_interval: Duration,
    pub last_auto_question_id: Option<Uuid>,
    pub last_auto_summary_segment_id: Option<Uuid>,
}

impl Default for PolicyState {
    fn default() -> Self {
        Self {
            mode: AppMode::ManualQa,
            cloud_paused: false,
            last_response_at: None,
            min_response_interval: Duration::seconds(15),
            last_auto_question_id: None,
            last_auto_summary_segment_id: None,
        }
    }
}

impl PolicyState {
    pub fn can_generate_manual_response(&self) -> bool {
        !self.cloud_paused
    }

    pub fn can_generate_automatic_response(&self, now: DateTime<Utc>) -> bool {
        if self.cloud_paused {
            return false;
        }

        match self.last_response_at {
            Some(last_response_at) => now - last_response_at >= self.min_response_interval,
            None => true,
        }
    }

    pub fn mark_response_sent(&mut self, now: DateTime<Utc>) {
        self.last_response_at = Some(now);
    }

    pub fn should_auto_answer_question(&self, question_id: Uuid, now: DateTime<Utc>) -> bool {
        self.mode == AppMode::Assisted
            && self.last_auto_question_id != Some(question_id)
            && self.can_generate_automatic_response(now)
    }

    pub fn mark_auto_question_answered(&mut self, question_id: Uuid, now: DateTime<Utc>) {
        self.last_auto_question_id = Some(question_id);
        self.mark_response_sent(now);
    }

    pub fn should_auto_summarise(&self, segment_id: Uuid, now: DateTime<Utc>) -> bool {
        self.mode == AppMode::Summary
            && self.last_auto_summary_segment_id != Some(segment_id)
            && self.can_generate_automatic_response(now)
    }

    pub fn mark_auto_summary_sent(&mut self, segment_id: Uuid, now: DateTime<Utc>) {
        self.last_auto_summary_segment_id = Some(segment_id);
        self.mark_response_sent(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assisted_mode_auto_answers_only_once_per_question() {
        let now = Utc::now();
        let question_id = Uuid::new_v4();
        let mut policy = PolicyState { mode: AppMode::Assisted, ..PolicyState::default() };

        assert!(policy.should_auto_answer_question(question_id, now));
        policy.mark_auto_question_answered(question_id, now);
        assert!(!policy.should_auto_answer_question(question_id, now));
    }

    #[test]
    fn manual_responses_ignore_cooldown_but_respect_cloud_pause() {
        let now = Utc::now();
        let mut policy = PolicyState::default();
        policy.mark_response_sent(now);

        assert!(policy.can_generate_manual_response());

        policy.cloud_paused = true;
        assert!(!policy.can_generate_manual_response());
    }
}
