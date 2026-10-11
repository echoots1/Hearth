//! HTTP API: router, shared state, and handlers.

mod health;

use std::sync::Arc;

use axum::{Router, routing::get};
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

use crate::{assets, config::Config};

#[derive(Clone)]
pub struct AppState {
    #[allow(dead_code)]
    pub config: Arc<Config>,
    pub pool: PgPool,
}

impl AppState {
    pub fn new(config: Config, pool: PgPool) -> Self {
        Self {
            config: Arc::new(config),
            pool,
        }
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new().route("/health", get(health::health));

    Router::new()
        .nest("/api", api)
        .fallback(assets::spa_fallback)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
