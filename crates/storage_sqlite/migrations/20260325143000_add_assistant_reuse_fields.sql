ALTER TABLE assistant_events ADD COLUMN model_id TEXT;
ALTER TABLE assistant_events ADD COLUMN request_kind TEXT;
ALTER TABLE assistant_events ADD COLUMN request_key TEXT;
ALTER TABLE assistant_events ADD COLUMN request_text TEXT;
ALTER TABLE assistant_events ADD COLUMN reused_from_event_id TEXT;
ALTER TABLE assistant_events ADD COLUMN reused_from_history INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS assistant_events_lookup_idx
  ON assistant_events (model_id, request_kind, request_key, created_at DESC);
