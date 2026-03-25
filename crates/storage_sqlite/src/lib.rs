use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use ipc_schema::{
    AppSettingsDto, AssistantEventDto, PrimingDocumentDto, SessionDetailDto, SessionSummaryDto,
    TranscriptSegmentDto, default_assistant_instruction,
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use transcript_core::{TranscriptSegment, is_question_candidate};
use uuid::Uuid;

pub struct Storage {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct PrimingDocumentRecord {
    pub id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    pub extracted_text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredAssistantEvent {
    pub id: Uuid,
    pub session_id: Uuid,
    pub kind: String,
    pub content: String,
    pub confidence: f32,
    pub created_at: DateTime<Utc>,
    pub model_id: Option<String>,
    pub request_kind: Option<String>,
    pub request_key: Option<String>,
    pub request_text: Option<String>,
    pub reused_from_history: bool,
    pub reused_from_event_id: Option<Uuid>,
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

    pub async fn load_settings(&self) -> Result<Option<AppSettingsDto>> {
        let rows = sqlx::query("SELECT key, value FROM settings").fetch_all(&self.pool).await?;
        if rows.is_empty() {
            return Ok(None);
        }

        let mut settings = AppSettingsDto::default();
        for row in rows {
            let key: String = row.try_get("key")?;
            let value: String = row.try_get("value")?;
            match key.as_str() {
                "retention_days" => settings.retention_days = value.parse().unwrap_or(0),
                "transcript_storage_enabled" => {
                    settings.transcript_storage_enabled = value.parse().unwrap_or(true)
                }
                "auto_start_cloud" => settings.auto_start_cloud = value.parse().unwrap_or(false),
                "default_mode" => {
                    settings.default_mode =
                        serde_json::from_str(&value).unwrap_or(settings.default_mode)
                }
                "openai_model" => {
                    settings.openai_model =
                        if value.trim().is_empty() { settings.openai_model.clone() } else { value }
                }
                "assistant_instruction" => {
                    settings.assistant_instruction = if value.trim().is_empty() {
                        default_assistant_instruction()
                    } else {
                        value
                    }
                }
                _ => {}
            }
        }

        Ok(Some(settings))
    }

    pub async fn save_settings(&self, settings: &AppSettingsDto) -> Result<()> {
        self.upsert_setting("retention_days", settings.retention_days.to_string()).await?;
        self.upsert_setting(
            "transcript_storage_enabled",
            settings.transcript_storage_enabled.to_string(),
        )
        .await?;
        self.upsert_setting("auto_start_cloud", settings.auto_start_cloud.to_string()).await?;
        self.upsert_setting("default_mode", serde_json::to_string(&settings.default_mode)?).await?;
        self.upsert_setting("openai_model", settings.openai_model.clone()).await?;
        self.upsert_setting("assistant_instruction", settings.assistant_instruction.clone())
            .await?;
        Ok(())
    }

    pub async fn list_priming_documents(&self) -> Result<Vec<PrimingDocumentDto>> {
        let rows = sqlx::query(
            r#"
            SELECT id, file_name, mime_type, extracted_text, created_at
            FROM priming_documents
            ORDER BY created_at DESC, file_name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_priming_document).collect()
    }

    pub async fn list_priming_document_records(&self) -> Result<Vec<PrimingDocumentRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, file_name, mime_type, extracted_text, created_at
            FROM priming_documents
            ORDER BY created_at DESC, file_name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_priming_record).collect()
    }

    pub async fn insert_priming_document(
        &self,
        file_name: &str,
        mime_type: &str,
        extracted_text: &str,
    ) -> Result<PrimingDocumentDto> {
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO priming_documents (id, file_name, mime_type, extracted_text, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(id.to_string())
        .bind(file_name)
        .bind(mime_type)
        .bind(extracted_text)
        .bind(created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(PrimingDocumentDto {
            id,
            file_name: file_name.to_string(),
            mime_type: mime_type.to_string(),
            char_count: extracted_text.chars().count() as u32,
            preview_text: clip_preview(extracted_text),
            created_at,
        })
    }

    pub async fn delete_priming_document(&self, document_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM priming_documents WHERE id = ?1")
            .bind(document_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
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
        model_id: &str,
        request_kind: &str,
        request_key: &str,
        request_text: &str,
        reused_from_event_id: Option<Uuid>,
        reused_from_history: bool,
    ) -> Result<StoredAssistantEvent> {
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO assistant_events (
              id, session_id, kind, input_window_ref, content, confidence, created_at,
              model_id, request_kind, request_key, request_text, reused_from_event_id, reused_from_history
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        )
        .bind(id.to_string())
        .bind(session_id.to_string())
        .bind(kind)
        .bind("recent_window")
        .bind(content)
        .bind(confidence)
        .bind(created_at.to_rfc3339())
        .bind(model_id)
        .bind(request_kind)
        .bind(request_key)
        .bind(request_text)
        .bind(reused_from_event_id.map(|value| value.to_string()))
        .bind(if reused_from_history { 1 } else { 0 })
        .execute(&self.pool)
        .await?;
        Ok(StoredAssistantEvent {
            id,
            session_id,
            kind: kind.to_string(),
            content: content.to_string(),
            confidence,
            created_at,
            model_id: Some(model_id.to_string()),
            request_kind: Some(request_kind.to_string()),
            request_key: Some(request_key.to_string()),
            request_text: Some(request_text.to_string()),
            reused_from_history,
            reused_from_event_id,
        })
    }

    pub async fn find_reusable_assistant_event(
        &self,
        model_id: &str,
        request_kind: &str,
        request_key: &str,
    ) -> Result<Option<StoredAssistantEvent>> {
        let row = sqlx::query(
            r#"
            SELECT
              id, session_id, kind, content, confidence, created_at, model_id,
              request_kind, request_key, request_text, reused_from_history, reused_from_event_id
            FROM assistant_events
            WHERE model_id = ?1
              AND request_kind = ?2
              AND request_key = ?3
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(model_id)
        .bind(request_kind)
        .bind(request_key)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_stored_assistant_event).transpose()
    }

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummaryDto>> {
        let rows = sqlx::query(
            r#"
            SELECT
              s.id,
              s.started_at,
              s.ended_at,
              s.capture_device,
              s.mode,
              COUNT(DISTINCT ts.id) AS transcript_segment_count,
              COUNT(DISTINCT ae.id) AS assistant_event_count,
              (
                SELECT text
                FROM transcript_segments last_ts
                WHERE last_ts.session_id = s.id
                ORDER BY last_ts.created_at DESC
                LIMIT 1
              ) AS latest_transcript_excerpt
            FROM sessions s
            LEFT JOIN transcript_segments ts ON ts.session_id = s.id
            LEFT JOIN assistant_events ae ON ae.session_id = s.id
            GROUP BY s.id
            ORDER BY s.started_at DESC
            LIMIT ?1
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_session_summary).collect()
    }

    pub async fn get_session_detail(&self, session_id: Uuid) -> Result<Option<SessionDetailDto>> {
        let row = sqlx::query(
            r#"
            SELECT
              s.id,
              s.started_at,
              s.ended_at,
              s.capture_device,
              s.mode,
              COUNT(DISTINCT ts.id) AS transcript_segment_count,
              COUNT(DISTINCT ae.id) AS assistant_event_count,
              (
                SELECT text
                FROM transcript_segments last_ts
                WHERE last_ts.session_id = s.id
                ORDER BY last_ts.created_at DESC
                LIMIT 1
              ) AS latest_transcript_excerpt
            FROM sessions s
            LEFT JOIN transcript_segments ts ON ts.session_id = s.id
            LEFT JOIN assistant_events ae ON ae.session_id = s.id
            WHERE s.id = ?1
            GROUP BY s.id
            "#,
        )
        .bind(session_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let summary = row_to_session_summary(row)?;
        let transcript_segments = sqlx::query(
            r#"
            SELECT id, session_id, start_ms, end_ms, text, source, created_at
            FROM transcript_segments
            WHERE session_id = ?1
            ORDER BY start_ms ASC, created_at ASC
            "#,
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(row_to_transcript_segment)
        .collect::<Result<Vec<_>>>()?;

        let assistant_events = sqlx::query(
            r#"
            SELECT
              id, session_id, kind, content, confidence, created_at,
              model_id, request_kind, request_text, reused_from_history, reused_from_event_id
            FROM assistant_events
            WHERE session_id = ?1
            ORDER BY created_at ASC
            "#,
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(row_to_assistant_event)
        .collect::<Result<Vec<_>>>()?;

        Ok(Some(SessionDetailDto { session: summary, transcript_segments, assistant_events }))
    }

    pub async fn delete_session(&self, session_id: Uuid) -> Result<()> {
        let session = session_id.to_string();
        sqlx::query("DELETE FROM transcript_segments WHERE session_id = ?1")
            .bind(&session)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM assistant_events WHERE session_id = ?1")
            .bind(&session)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM audit_events WHERE session_id = ?1")
            .bind(&session)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM sessions WHERE id = ?1")
            .bind(&session)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn purge_sessions_older_than_days(&self, retention_days: u32) -> Result<u64> {
        if retention_days == 0 {
            return Ok(0);
        }

        let stale_rows = sqlx::query(
            r#"
            SELECT id
            FROM sessions
            WHERE started_at < datetime('now', printf('-%d days', ?1))
            "#,
        )
        .bind(retention_days as i64)
        .fetch_all(&self.pool)
        .await?;

        let session_ids = stale_rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("id"))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        for session_id in &session_ids {
            self.delete_session(Uuid::parse_str(session_id)?).await?;
        }

        Ok(session_ids.len() as u64)
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

    async fn upsert_setting(&self, key: &str, value: String) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO settings (key, value)
            VALUES (?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn row_to_session_summary(row: sqlx::sqlite::SqliteRow) -> Result<SessionSummaryDto> {
    Ok(SessionSummaryDto {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        started_at: parse_sqlite_datetime(&row.try_get::<String, _>("started_at")?)?,
        ended_at: row
            .try_get::<Option<String>, _>("ended_at")?
            .map(|value| parse_sqlite_datetime(&value))
            .transpose()?,
        capture_device: row.try_get("capture_device")?,
        mode: row.try_get("mode")?,
        transcript_segment_count: row.try_get::<i64, _>("transcript_segment_count")? as u32,
        assistant_event_count: row.try_get::<i64, _>("assistant_event_count")? as u32,
        latest_transcript_excerpt: row.try_get("latest_transcript_excerpt")?,
    })
}

fn row_to_transcript_segment(row: sqlx::sqlite::SqliteRow) -> Result<TranscriptSegmentDto> {
    let text: String = row.try_get("text")?;
    Ok(TranscriptSegmentDto {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        session_id: Uuid::parse_str(&row.try_get::<String, _>("session_id")?)?,
        start_ms: row.try_get::<i64, _>("start_ms")? as u64,
        end_ms: row.try_get::<i64, _>("end_ms")? as u64,
        text: text.clone(),
        source: row.try_get("source")?,
        created_at: parse_sqlite_datetime(&row.try_get::<String, _>("created_at")?)?,
        is_question_candidate: is_question_candidate(&text),
    })
}

fn row_to_assistant_event(row: sqlx::sqlite::SqliteRow) -> Result<AssistantEventDto> {
    Ok(AssistantEventDto {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        session_id: Uuid::parse_str(&row.try_get::<String, _>("session_id")?)?,
        kind: row.try_get("kind")?,
        request_kind: row.try_get("request_kind")?,
        content: row.try_get("content")?,
        confidence: row.try_get::<f64, _>("confidence")? as f32,
        created_at: parse_sqlite_datetime(&row.try_get::<String, _>("created_at")?)?,
        model_id: row.try_get("model_id")?,
        request_text: row.try_get("request_text")?,
        reused_from_history: row.try_get::<i64, _>("reused_from_history").unwrap_or(0) != 0,
        reused_from_event_id: row
            .try_get::<Option<String>, _>("reused_from_event_id")?
            .map(|value| Uuid::parse_str(&value))
            .transpose()?,
    })
}

fn row_to_stored_assistant_event(row: sqlx::sqlite::SqliteRow) -> Result<StoredAssistantEvent> {
    Ok(StoredAssistantEvent {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        session_id: Uuid::parse_str(&row.try_get::<String, _>("session_id")?)?,
        kind: row.try_get("kind")?,
        content: row.try_get("content")?,
        confidence: row.try_get::<f64, _>("confidence")? as f32,
        created_at: parse_sqlite_datetime(&row.try_get::<String, _>("created_at")?)?,
        model_id: row.try_get("model_id")?,
        request_kind: row.try_get("request_kind")?,
        request_key: row.try_get("request_key")?,
        request_text: row.try_get("request_text")?,
        reused_from_history: row.try_get::<i64, _>("reused_from_history").unwrap_or(0) != 0,
        reused_from_event_id: row
            .try_get::<Option<String>, _>("reused_from_event_id")?
            .map(|value| Uuid::parse_str(&value))
            .transpose()?,
    })
}

fn row_to_priming_document(row: sqlx::sqlite::SqliteRow) -> Result<PrimingDocumentDto> {
    let extracted_text: String = row.try_get("extracted_text")?;
    Ok(PrimingDocumentDto {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        file_name: row.try_get("file_name")?,
        mime_type: row.try_get("mime_type")?,
        char_count: extracted_text.chars().count() as u32,
        preview_text: clip_preview(&extracted_text),
        created_at: parse_sqlite_datetime(&row.try_get::<String, _>("created_at")?)?,
    })
}

fn row_to_priming_record(row: sqlx::sqlite::SqliteRow) -> Result<PrimingDocumentRecord> {
    Ok(PrimingDocumentRecord {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
        file_name: row.try_get("file_name")?,
        mime_type: row.try_get("mime_type")?,
        extracted_text: row.try_get("extracted_text")?,
        created_at: parse_sqlite_datetime(&row.try_get::<String, _>("created_at")?)?,
    })
}

fn clip_preview(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut clipped = normalized.chars().take(220).collect::<String>();
    if normalized.chars().count() > 220 {
        clipped.push_str("...");
    }
    clipped
}

fn parse_sqlite_datetime(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
        })
        .with_context(|| format!("failed to parse datetime {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn finds_reusable_assistant_event_by_model_kind_and_key() -> Result<()> {
        let database_path =
            std::env::temp_dir().join(format!("soundmind-storage-test-{}.db", Uuid::new_v4()));
        let database_url = format!("sqlite://{}", database_path.display());
        let storage = Storage::connect(&database_url).await?;
        let session_id = Uuid::new_v4();

        storage
            .insert_assistant_event(
                session_id,
                "answer",
                "Old answer",
                0.4,
                "gpt-4o-mini",
                "answer",
                "question one?",
                "Question one?",
                None,
                false,
            )
            .await?;

        let reused = storage
            .insert_assistant_event(
                session_id,
                "answer",
                "New answer",
                0.9,
                "gpt-4o-mini",
                "answer",
                "question one?",
                "Question one?",
                None,
                false,
            )
            .await?;

        storage
            .insert_assistant_event(
                session_id,
                "answer",
                "Different model answer",
                0.8,
                "gpt-5.4",
                "answer",
                "question one?",
                "Question one?",
                None,
                false,
            )
            .await?;

        let found = storage
            .find_reusable_assistant_event("gpt-4o-mini", "answer", "question one?")
            .await?
            .expect("expected reusable answer");

        assert_eq!(found.id, reused.id);
        assert_eq!(found.content, "New answer");

        let missing = storage
            .find_reusable_assistant_event("gpt-5.4-mini", "answer", "question one?")
            .await?;
        assert!(missing.is_none());

        let _ = std::fs::remove_file(database_path);
        Ok(())
    }
}
