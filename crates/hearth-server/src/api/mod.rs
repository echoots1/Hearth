//! HTTP API: router, shared state, error mapping, and handlers.
//!
//! Mutations and most reads go through `/api/tools/{name}` (the same registry
//! the MCP server exposes). The rest is auth, push, keys, users, secrets,
//! health, and the notification action endpoint.

mod auth;
mod error;
mod health;
mod keys;
mod links;
mod push;
mod tools;
mod users;

use std::sync::Arc;

use axum::{
    Router,
    extract::Request,
    routing::{get, post},
};
use chrono::Utc;
use chrono_tz::Tz;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::services::secrets::Crypto;
use crate::services::{self, Ctx};
use crate::{assets, config::Config};

pub use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub household_id: Uuid,
    pub crypto: Option<Crypto>,
}

impl AppState {
    pub async fn new(config: Config, pool: PgPool) -> anyhow::Result<Self> {
        let h = services::household::ensure(&pool).await?;
        let crypto = config
            .app_encryption_key
            .as_deref()
            .map(Crypto::from_passphrase);
        Ok(Self {
            config: Arc::new(config),
            pool,
            household_id: h.id,
            crypto,
        })
    }

    /// A service context for "now", with the household's current timezone.
    pub async fn ctx(&self) -> anyhow::Result<Ctx> {
        let tz: Tz =
            sqlx::query_scalar::<_, String>("select timezone from households where id = $1")
                .bind(self.household_id)
                .fetch_one(&self.pool)
                .await?
                .parse()
                .unwrap_or(chrono_tz::UTC);
        Ok(Ctx {
            pool: self.pool.clone(),
            household_id: self.household_id,
            tz,
            now: Utc::now(),
            crypto: self.crypto.clone(),
        })
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health::health))
        .route("/auth/status", get(auth::status))
        .route("/auth/setup", post(auth::setup))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/tools", get(tools::list))
        .route("/tools/{name}", post(tools::call))
        .route("/proposals/{id}/approve", post(tools::approve_proposal))
        .route("/proposals/{id}/decline", post(tools::decline_proposal))
        .route("/actions/{id}/undo", post(tools::undo_action))
        .route("/n/{token}", get(links::act))
        .route("/push/vapid", get(push::vapid_public_key))
        .route("/push/subscribe", post(push::subscribe))
        .route("/push/unsubscribe", post(push::unsubscribe))
        .route("/keys", get(keys::list).post(keys::create))
        .route("/keys/{id}/revoke", post(keys::revoke))
        .route("/users", get(users::list).post(users::create))
        .route("/users/{id}/password", post(users::set_password))
        .route(
            "/settings/assistant",
            get(users::assistant_settings).post(users::set_assistant_settings),
        )
        .route(
            "/settings/anthropic-key",
            post(users::set_anthropic_key).delete(users::clear_anthropic_key),
        );

    Router::new()
        .nest("/api", api)
        .merge(crate::mcp::router(state.clone()))
        .fallback(assets::spa_fallback)
        .layer(TraceLayer::new_for_http().make_span_with(|req: &Request| {
            tracing::info_span!(
                "request",
                method = %req.method(),
                path = %crate::mcp::redact_path(req.uri().path()),
            )
        }))
        .with_state(state)
}
