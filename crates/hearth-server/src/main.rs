//! Hearth — household money, on plan.
//!
//! Single binary: HTTP API + embedded web UI + MCP server + job scheduler.

mod api;
mod assets;
#[allow(dead_code)] // fields are consumed as integrations land
mod config;
mod db;
#[allow(dead_code)] // consumed by the API, MCP and jobs steps
mod services;

use std::net::SocketAddr;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;

#[derive(Parser)]
#[command(name = "hearth", version, about = "Hearth — household money, on plan.")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run migrations and serve the app (default).
    Serve,
    /// Run pending database migrations and exit.
    Migrate,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // .env is optional and only for local development; Docker passes real env.
    let _ = dotenvy::dotenv();
    init_tracing();

    let cli = Cli::parse();
    let config = Config::from_env().context("invalid configuration")?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Migrate => {
            let pool = db::connect(&config.database_url).await?;
            db::migrate(&pool).await?;
            tracing::info!("migrations complete");
        }
        Command::Serve => serve(config).await?,
    }
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("LOG_LEVEL")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info"));
    let json = std::env::var("LOG_FORMAT").is_ok_and(|v| v == "json");
    let registry = tracing_subscriber::registry().with(filter);
    if json {
        registry
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        registry.with(tracing_subscriber::fmt::layer()).init();
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    let state = api::AppState::new(config.clone(), pool);
    let app = api::router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, app_url = %config.app_url, "hearth listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;
    tracing::info!("shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
