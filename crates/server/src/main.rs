//! media-convertor HTTP API binary — a thin wrapper that builds state from the
//! environment and serves the router. All logic lives in the library
//! ([`media_convertor_server`]) so it can be integration-tested.

use media_convertor_server::{build_app, build_state, config_from_env};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = config_from_env();
    config.validate_server()?;

    let state = build_state(config)?;
    let (host, port, workers, raw) = (
        state.config.host.clone(),
        state.config.port,
        state.config.workers,
        state.config.raw_enabled,
    );

    let app = build_app(state);

    let addr = format!("{host}:{port}");
    tracing::info!("media-convertor server listening on {addr} ({workers} workers, raw={raw})");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
