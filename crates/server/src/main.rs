//! media-convertor HTTP server.

mod routes;
mod state;

use axum::routing::{delete, get, post};
use axum::Router;
use state::AppState;
use std::path::PathBuf;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(routes::health))
        .route("/api/v1/presets", get(routes::list_presets))
        .route("/api/v1/formats", get(routes::list_formats))
        .route("/api/v1/probe", post(routes::probe))
        .route("/api/v1/convert", post(routes::submit_convert))
        .route("/api/v1/jobs/{id}/status", get(routes::job_status))
        .route("/api/v1/jobs/{id}/result", get(routes::job_result))
        .route("/api/v1/jobs/{id}", delete(routes::delete_job))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let port: u16 = std::env::var("MEDIA_CONVERTOR_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3400);

    let data_dir = std::env::var("MEDIA_CONVERTOR_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("media-convertor"));

    let upload_dir = data_dir.join("uploads");
    let output_dir = data_dir.join("output");
    tokio::fs::create_dir_all(&upload_dir).await?;
    tokio::fs::create_dir_all(&output_dir).await?;

    let state = AppState::new(upload_dir, output_dir);
    let app = build_app(state);

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("media-convertor server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
    tracing::info!("shutting down");
}
