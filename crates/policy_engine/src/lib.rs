use chrono::{DateTime, Duration, Utc};
use ipc_schema::AppMode;

#[derive(Debug, Clone)]
pub struct PolicyState {
    pub mode: AppMode,
    pub cloud_paused: bool,
    pub last_response_at: Option<DateTime<Utc>>,
    pub min_response_interval: Duration,
}

impl Default for PolicyState {
    fn default() -> Self {
        Self {
            mode: AppMode::ManualQa,
            cloud_paused: false,
            last_response_at: None,
            min_response_interval: Duration::seconds(30),
        }
    }
}

impl PolicyState {
    pub fn can_generate_response(&self, now: DateTime<Utc>) -> bool {
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
}
