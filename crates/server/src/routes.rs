//! HTTP API route handlers.

use crate::state::{AppState, Job, JobStatus};
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use media_convertor_core::container::Container;
use media_convertor_core::preset::Preset;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Health ─────────────────────────────────────────────────────

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

// ── Presets & Formats ──────────────────────────────────────────

pub async fn list_presets() -> impl IntoResponse {
    Json(Preset::all())
}

pub async fn list_formats() -> impl IntoResponse {
    #[derive(Serialize)]
    struct FormatsResponse {
        containers: Vec<ContainerInfo>,
        audio_codecs: Vec<CodecInfo>,
        video_codecs: Vec<CodecInfo>,
    }
    #[derive(Serialize)]
    struct ContainerInfo {
        name: String,
        extension: String,
        supports_video: bool,
        supports_audio: bool,
    }
    #[derive(Serialize)]
    struct CodecInfo {
        name: String,
        display_name: String,
    }

    let resp = FormatsResponse {
        containers: Container::all()
            .iter()
            .map(|c| ContainerInfo {
                name: format!("{:?}", c),
                extension: c.extension().to_string(),
                supports_video: c.supports_video(),
                supports_audio: c.supports_audio(),
            })
            .collect(),
        audio_codecs: media_convertor_core::codec::AudioCodec::all()
            .iter()
            .map(|c| CodecInfo {
                name: format!("{:?}", c),
                display_name: c.display_name().to_string(),
            })
            .collect(),
        video_codecs: media_convertor_core::codec::VideoCodec::all()
            .iter()
            .map(|c| CodecInfo {
                name: format!("{:?}", c),
                display_name: c.display_name().to_string(),
            })
            .collect(),
    };
    Json(resp)
}

// ── Probe ──────────────────────────────────────────────────────

pub async fn probe(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut file_path = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        if field.name() == Some("file") {
            let filename = field
                .file_name()
                .unwrap_or("upload")
                .to_string();
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

            let path = state.upload_dir.join(&filename);
            tokio::fs::write(&path, &data)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            file_path = Some(path);
        }
    }

    let path = file_path
        .ok_or((StatusCode::BAD_REQUEST, "missing 'file' field".to_string()))?;

    #[cfg(feature = "ffmpeg")]
    {
        let info = media_convertor_core::detect::probe_file(&path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let _ = tokio::fs::remove_file(&path).await;
        let json = serde_json::to_value(&info)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(json))
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        let _ = path;
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "FFmpeg not available".to_string(),
        ))
    }
}

// ── Convert (submit job) ───────────────────────────────────────

#[allow(dead_code)]
#[derive(Deserialize)]
pub struct ConvertParams {
    pub preset: Option<String>,
    pub format: Option<String>,
    pub audio_codec: Option<String>,
    pub video_codec: Option<String>,
}

pub async fn submit_convert(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut file_path = None;
    let mut preset_name = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        match field.name() {
            Some("file") => {
                let filename = field.file_name().unwrap_or("upload").to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                let path = state.upload_dir.join(&filename);
                tokio::fs::write(&path, &data)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                file_path = Some(path);
            }
            Some("preset") => {
                preset_name = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
                );
            }
            _ => {}
        }
    }

    let input_path = file_path
        .ok_or((StatusCode::BAD_REQUEST, "missing 'file' field".to_string()))?;

    let mut job = Job::new(input_path.clone());
    job.preset = preset_name.clone();

    // Determine output path
    let preset = preset_name
        .as_deref()
        .and_then(Preset::by_name)
        .unwrap_or_else(|| Preset::by_name("web-mp4").unwrap());

    let ext = preset.format.container.extension();
    let output_path = state.output_dir.join(format!("{}.{}", job.id, ext));
    job.output_path = Some(output_path.clone());

    let job_id = state.insert_job(job);

    // Spawn background conversion
    let state_clone = state.clone();
    tokio::task::spawn_blocking(move || {
        state_clone.update_status(&job_id, JobStatus::Running);

        #[cfg(feature = "ffmpeg")]
        {
            let transcode_job = media_convertor_core::transcode::TranscodeJob::new(
                input_path,
                output_path,
                preset.format.clone(),
            );
            match media_convertor_core::transcode::transcode_simple(transcode_job) {
                Ok(_) => {
                    state_clone.update_status(&job_id, JobStatus::Completed);
                }
                Err(e) => {
                    if let Some(job) = state_clone.jobs.write().unwrap().get_mut(&job_id) {
                        job.status = JobStatus::Failed;
                        job.error = Some(e.to_string());
                    }
                }
            }
        }

        #[cfg(not(feature = "ffmpeg"))]
        {
            if let Some(job) = state_clone.jobs.write().unwrap().get_mut(&job_id) {
                job.status = JobStatus::Failed;
                job.error = Some("FFmpeg not available".to_string());
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "job_id": job_id,
            "status": "queued"
        })),
    ))
}

// ── Job status ─────────────────────────────────────────────────

pub async fn job_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let job = state
        .get_job(&id)
        .ok_or((StatusCode::NOT_FOUND, "job not found".to_string()))?;

    Ok(Json(serde_json::json!({
        "job_id": job.id,
        "status": job.status,
        "progress": job.progress,
        "error": job.error,
    })))
}

// ── Job result (download) ──────────────────────────────────────

pub async fn job_result(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let job = state
        .get_job(&id)
        .ok_or((StatusCode::NOT_FOUND, "job not found".to_string()))?;

    if job.status != JobStatus::Completed {
        return Err((
            StatusCode::CONFLICT,
            format!("job is {:?}, not completed", job.status),
        ));
    }

    let output_path = job
        .output_path
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "no output path".to_string()))?;

    let bytes = tokio::fs::read(&output_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let container = Container::from_path(&output_path);
    let content_type = container
        .map(|c| c.mime_type())
        .unwrap_or("application/octet-stream");

    Ok((
        [(axum::http::header::CONTENT_TYPE, content_type)],
        bytes,
    ))
}

// ── Delete job ─────────────────────────────────────────────────

pub async fn delete_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let job = state
        .jobs
        .write()
        .unwrap()
        .remove(&id)
        .ok_or((StatusCode::NOT_FOUND, "job not found".to_string()))?;

    // Clean up files
    if let Some(ref p) = job.output_path {
        let _ = tokio::fs::remove_file(p).await;
    }
    let _ = tokio::fs::remove_file(&job.input_path).await;

    Ok(Json(serde_json::json!({ "deleted": id })))
}
