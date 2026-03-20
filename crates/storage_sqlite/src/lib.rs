use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use transcript_core::TranscriptSegment;
use uuid::Uuid;

pub struct Storage {
    pool: SqlitePool,
}

impl Storage {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let connect_options = if let Some(path) = database_url.strip_prefix("sqlite://") {
            SqliteConnectOptions::new().filename(path).create_if_missing(true)
        } else {
            SqliteConnectOptions::from_str(database_url)?
        };
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(connect_options)
            .await
            .with_context(|| format!("failed to open sqlite database at {database_url}"))?;

        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn start_session(
        &self,
        session_id: Uuid,
        capture_device: &str,
        mode: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO sessions (id, started_at, capture_device, mode, privacy_flags)
            VALUES (?1, CURRENT_TIMESTAMP, ?2, ?3, ?4)
            "#,
        )
        .bind(session_id.to_string())
        .bind(capture_device)
        .bind(mode)
        .bind("manual_start")
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn end_session(&self, session_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE sessions SET ended_at = CURRENT_TIMESTAMP WHERE id = ?1")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_transcript_segment(&self, segment: &TranscriptSegment) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO transcript_segments (id, session_id, start_ms, end_ms, text, source, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(segment.id.to_string())
        .bind(segment.session_id.to_string())
        .bind(segment.start_ms as i64)
        .bind(segment.end_ms as i64)
        .bind(&segment.text)
        .bind(&segment.source)
        .bind(segment.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_assistant_event(
        &self,
        session_id: Uuid,
        kind: &str,
        content: &str,
        confidence: f32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO assistant_events (id, session_id, kind, input_window_ref, content, confidence, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id.to_string())
        .bind(kind)
        .bind("recent_window")
        .bind(content)
        .bind(confidence)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn append_audit_event(&self, session_id: Option<Uuid>, event: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO audit_events (id, session_id, event, created_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id.map(|id| id.to_string()))
        .bind(event)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn recent_transcript_count(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) AS count FROM transcript_segments")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("count")?)
    }
}
