use app_core::{
    AppCoreState, DeletePrimingDocumentResponse, SessionExportQuery, UploadPrimingDocumentRequest,
};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ipc_schema::{
    AppSettingsDto, BackendStatusSnapshot, PrimingDocumentDto, SessionDetailDto, SessionSummaryDto,
    UserAction,
};
use tracing::warn;
use uuid::Uuid;

pub(crate) fn router(state: AppCoreState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/actions", post(post_action))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/priming-documents", get(get_priming_documents).post(post_priming_document))
        .route("/priming-documents/{document_id}", axum::routing::delete(delete_priming_document))
        .route("/sessions", get(get_sessions))
        .route("/sessions/purge", post(post_purge_sessions))
        .route("/sessions/{session_id}", get(get_session_detail).delete(delete_session))
        .route("/sessions/{session_id}/export", get(get_session_export))
        .with_state(state)
}

async fn health(State(state): State<AppCoreState>) -> Json<BackendStatusSnapshot> {
    Json(state.snapshot().await)
}

async fn post_action(
    State(state): State<AppCoreState>,
    Json(action): Json<UserAction>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    state.dispatch_action(action).await.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn get_settings(State(state): State<AppCoreState>) -> Json<AppSettingsDto> {
    Json(state.get_settings().await)
}

async fn get_priming_documents(
    State(state): State<AppCoreState>,
) -> Result<Json<Vec<PrimingDocumentDto>>, StatusCode> {
    let documents = state.list_priming_documents().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(documents))
}

async fn post_priming_document(
    State(state): State<AppCoreState>,
    Json(request): Json<UploadPrimingDocumentRequest>,
) -> Result<Json<PrimingDocumentDto>, StatusCode> {
    let document = state.upload_priming_document(request).await.map_err(|error| {
        warn!(?error, "failed to ingest priming document");
        StatusCode::BAD_REQUEST
    })?;
    Ok(Json(document))
}

async fn delete_priming_document(
    State(state): State<AppCoreState>,
    AxumPath(document_id): AxumPath<Uuid>,
) -> Result<Json<DeletePrimingDocumentResponse>, StatusCode> {
    let response = state.delete_priming_document(document_id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(response))
}

async fn put_settings(
    State(state): State<AppCoreState>,
    Json(settings): Json<AppSettingsDto>,
) -> Result<Json<AppSettingsDto>, StatusCode> {
    let saved = state.save_settings(settings).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(saved))
}

async fn get_sessions(
    State(state): State<AppCoreState>,
) -> Result<Json<Vec<SessionSummaryDto>>, StatusCode> {
    let sessions = state.list_sessions().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(sessions))
}

async fn get_session_detail(
    State(state): State<AppCoreState>,
    AxumPath(session_id): AxumPath<Uuid>,
) -> Result<Json<SessionDetailDto>, StatusCode> {
    let session = state.get_session_detail(session_id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    session.map(Json).ok_or(StatusCode::NOT_FOUND)
}

async fn delete_session(
    State(state): State<AppCoreState>,
    AxumPath(session_id): AxumPath<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.delete_session(session_id).await.map_err(|error| {
        if error.to_string().contains("currently active session") {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    Ok(Json(serde_json::json!({ "deleted": session_id })))
}

async fn post_purge_sessions(
    State(state): State<AppCoreState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let deleted = state.purge_sessions().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

async fn get_session_export(
    State(state): State<AppCoreState>,
    AxumPath(session_id): AxumPath<Uuid>,
    Query(query): Query<SessionExportQuery>,
) -> Result<Response, StatusCode> {
    let Some(session) = state.export_session(session_id, query.format.as_deref()).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? else {
        return Err(StatusCode::NOT_FOUND);
    };
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_static(session.content_type))], session.body)
        .into_response())
}
