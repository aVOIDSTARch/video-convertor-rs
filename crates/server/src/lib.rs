//! media-convertor HTTP API library: router, state, and middleware.
//!
//! The binary ([`main`]) is a thin wrapper that builds state from the environment and
//! serves [`build_app`]. Exposing these as a library lets integration tests drive the
//! real router.

pub mod routes;
pub mod state;

use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::Router;
use media_convertor_core::api_queue::{Queue, QueueHandler};
use media_convertor_core::security;
use media_convertor_core::{Config, Engine, FfmpegHandler};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub use state::AppState;

/// Build a [`Config`] from `MEDIA_CONVERTOR_*` environment variables.
pub fn config_from_env() -> Config {
    let mut c = Config::default();
    if let Ok(v) = std::env::var("MEDIA_CONVERTOR_HOST") {
        c.host = v;
    }
    if let Some(v) = env_parse("MEDIA_CONVERTOR_PORT") {
        c.port = v;
    }
    if let Some(v) = env_parse("MEDIA_CONVERTOR_WORKERS") {
        c.workers = v;
    }
    if let Some(v) = env_parse("MEDIA_CONVERTOR_TIMEOUT") {
        c.job_timeout_secs = v;
    }
    if let Ok(v) = std::env::var("MEDIA_CONVERTOR_DATA") {
        c.work_dir = v.into();
    }
    if let Ok(v) = std::env::var("MEDIA_CONVERTOR_TOKEN") {
        if !v.is_empty() {
            c.token = Some(v);
        }
    }
    if let Ok(v) = std::env::var("MEDIA_CONVERTOR_RAW") {
        c.raw_enabled = matches!(v.as_str(), "1" | "true" | "yes");
    }
    c
}

fn env_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

/// Build the shared application state: engine, handler, and persistent queue.
pub fn build_state(config: Config) -> anyhow::Result<AppState> {
    config.ensure_dirs()?;
    let engine = Engine::new(&config)?;
    tracing::info!(
        "ffmpeg {} ready: {} encoders, {} filters discovered",
        engine.tools().version,
        engine.capabilities().encoders.len(),
        engine.capabilities().filters.len(),
    );
    let handler = Arc::new(FfmpegHandler::new(
        engine,
        config.output_dir(),
        config.raw_enabled,
    ));
    let queue = Arc::new(Queue::new(
        config.jobs_dir(),
        config.workers,
        handler.clone() as Arc<dyn QueueHandler>,
    )?);
    Ok(AppState {
        config: Arc::new(config),
        queue,
        handler,
    })
}

/// Build the axum router from application state.
pub fn build_app(state: AppState) -> Router {
    let max_upload = state.config.max_upload_bytes as usize;
    Router::new()
        .route("/api/v1/health", get(routes::health))
        .route("/api/v1/capabilities", get(routes::capabilities))
        .route("/api/v1/presets", get(routes::presets))
        .route("/api/v1/convert", post(op_convert))
        .route("/api/v1/extract-audio", post(op_extract_audio))
        .route("/api/v1/thumbnail", post(op_thumbnail))
        .route("/api/v1/filter", post(op_filter))
        .route("/api/v1/concat", post(op_concat))
        .route("/api/v1/probe", post(op_probe))
        .route("/api/v1/raw", post(op_raw))
        .route("/api/v1/jobs/:id/status", get(routes::job_status))
        .route("/api/v1/jobs/:id/result", get(routes::job_result))
        .route("/api/v1/jobs/:id", delete(routes::delete_job))
        .layer(DefaultBodyLimit::max(max_upload))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// One explicit handler per operation; each forwards to the shared submit logic.
macro_rules! op_handler {
    ($name:ident, $op:literal) => {
        async fn $name(State(state): State<AppState>, multipart: Multipart) -> axum::response::Response {
            use axum::response::IntoResponse;
            routes::submit(state, $op, multipart).await.into_response()
        }
    };
}

op_handler!(op_convert, "convert");
op_handler!(op_extract_audio, "extract-audio");
op_handler!(op_thumbnail, "thumbnail");
op_handler!(op_filter, "filter");
op_handler!(op_concat, "concat");
op_handler!(op_probe, "probe");
op_handler!(op_raw, "raw");

/// Bearer-token auth middleware (skipped for health, and when no token is configured).
async fn auth(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if path == "/api/v1/health" {
        return Ok(next.run(req).await);
    }
    if let Some(expected) = &state.config.token {
        let provided = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if !security::token_matches(expected, provided) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    Ok(next.run(req).await)
}
