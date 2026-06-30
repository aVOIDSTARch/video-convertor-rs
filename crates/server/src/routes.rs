//! HTTP route handlers. The API exposes **only** ffmpeg operations — never server or
//! admin controls. File-producing ops and probe are queued; capabilities/presets are
//! answered inline.

use crate::state::{AppState, NoReporter};
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use media_convertor_core::api_queue::{Attachment, Method, UniversalRequest};
use media_convertor_core::security;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

type ApiError = (StatusCode, String);

// ── Health ─────────────────────────────────────────────────────

pub async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

// ── Inline capabilities & presets ──────────────────────────────

pub async fn capabilities(
    State(state): State<AppState>,
    Query(params): Query<BTreeMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let kind = params.get("kind").cloned().unwrap_or_else(|| "all".to_string());
    let req = UniversalRequest::new(Method::Get, "capabilities").with_query("kind", kind);
    run_inline(&state, &req)
}

pub async fn presets(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let req = UniversalRequest::new(Method::Get, "presets");
    run_inline(&state, &req)
}

fn run_inline(state: &AppState, req: &UniversalRequest) -> Result<Json<Value>, ApiError> {
    let result = state
        .handler
        .execute(Uuid::new_v4(), req, &NoReporter)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(result.body.unwrap_or(Value::Null)))
}

// ── Submit a queued operation ──────────────────────────────────

/// Handle a multipart submission for `op` (convert/extract-audio/thumbnail/filter/
/// concat/probe/raw): store files, parse the `request` JSON, and enqueue.
pub async fn submit(
    state: AppState,
    op: &str,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ApiError> {
    let upload_dir = state.config.upload_dir();
    let mut attachments: Vec<Attachment> = Vec::new();
    let mut body: Value = Value::Null;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "request" {
            let text = field
                .text()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            if !text.trim().is_empty() {
                body = serde_json::from_str(&text)
                    .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid request JSON: {e}")))?;
            }
        } else if name == "file" || name.starts_with("file") {
            let original = field.file_name().unwrap_or("upload").to_string();
            let safe = security::sanitize_filename(&original);
            let stored = format!("{}-{}", Uuid::new_v4(), safe);
            let dest = security::confine_path(&upload_dir, std::path::Path::new(&stored))
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            tokio::fs::write(&dest, &data)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            attachments.push(Attachment {
                field: name,
                filename: safe,
                path: dest,
            });
        }
    }

    let mut request = UniversalRequest::new(Method::Post, op).with_body(body);
    request.attachments = attachments;

    // Validate routing/parameters up-front so the client gets an immediate 400 rather
    // than an async failure.
    media_convertor_core::operation::Operation::from_request(&request)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let job_id = state.queue.submit(request);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job_id, "status": "queued" })),
    ))
}

// ── Job status / result / delete ───────────────────────────────

pub async fn job_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let job = state
        .queue
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "job not found".to_string()))?;
    Ok(Json(json!({
        "job_id": job.id,
        "status": job.status,
        "progress": job.progress,
        "error": job.error,
    })))
}

pub async fn job_result(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, ApiError> {
    let job = state
        .queue
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "job not found".to_string()))?;

    let result = job
        .result
        .ok_or((StatusCode::CONFLICT, format!("job is {:?}, no result", job.status)))?;

    // Inline JSON result (probe/capabilities/presets).
    if let Some(body) = result.body {
        return Ok(Json(body).into_response());
    }

    let path = result
        .output_path
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "no output path".to_string()))?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let content_type = result
        .content_type
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let filename = result.output_name.unwrap_or_else(|| "output".to_string());

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub async fn delete_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let job = state
        .queue
        .remove(&id)
        .ok_or((StatusCode::NOT_FOUND, "job not found".to_string()))?;

    // Clean up produced output and uploaded inputs.
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(result) = &job.result {
        if let Some(p) = &result.output_path {
            paths.push(p.clone());
        }
    }
    for att in &job.request.attachments {
        paths.push(att.path.clone());
    }
    for p in paths {
        let _ = tokio::fs::remove_file(p).await;
    }

    Ok(Json(json!({ "deleted": id })))
}
