//! HTTP API: router, shared state, error mapping, and handlers.
//!
//! Mutations and most reads go through `/api/tools/{name}` (the same registry
//! the MCP server exposes). The rest is auth, push, keys, users, secrets,
//! health, and the notification action endpoint.

mod auth;
mod chat;
mod error;
mod health;
mod integrations;
mod keys;
mod links;
mod push;
mod server;
mod tools;
mod users;

use std::sync::{Arc, RwLock};

use axum::{
    Router,
    extract::Request,
    routing::{get, post, put},
};
use chrono::Utc;
use chrono_tz::Tz;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::auth::ratelimit;
use crate::services::secrets::Crypto;
use crate::services::server::Runtime;
use crate::services::{self, Ctx};
use crate::{assets, config::Config};

pub use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    /// The environment as read at start.
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub household_id: Uuid,
    pub crypto: Option<Crypto>,
    /// Effective server settings: the app's overrides over the environment.
    runtime: Arc<RwLock<Arc<Runtime>>>,
}

impl AppState {
    pub async fn new(config: Config, pool: PgPool) -> anyhow::Result<Self> {
        let h = services::household::ensure(&pool).await?;
        let crypto = config
            .app_encryption_key
            .as_deref()
            .map(Crypto::from_passphrase);
        let stored = services::server::load_from(&pool, h.id).await?;
        let runtime = Runtime::compute(&config, &stored);
        Ok(Self {
            config: Arc::new(config),
            pool,
            household_id: h.id,
            crypto,
            runtime: Arc::new(RwLock::new(Arc::new(runtime))),
        })
    }

    /// The effective server settings right now (cheap: an `Arc` clone).
    pub fn runtime(&self) -> Arc<Runtime> {
        self.runtime
            .read()
            .map(|r| r.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    /// Rebuild the effective settings after a save.
    pub async fn reload_runtime(&self) -> anyhow::Result<Arc<Runtime>> {
        let stored = services::server::load_from(&pool_ref(self), self.household_id).await?;
        let next = Arc::new(Runtime::compute(&self.config, &stored));
        match self.runtime.write() {
            Ok(mut w) => *w = next.clone(),
            Err(e) => *e.into_inner() = next.clone(),
        }
        Ok(next)
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

fn pool_ref(state: &AppState) -> PgPool {
    state.pool.clone()
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health::health))
        .route("/auth/status", get(auth::status))
        .route("/auth/setup", post(auth::setup))
        .route(
            "/auth/login",
            post(auth::login).layer(axum::middleware::from_fn_with_state(
                (
                    state.clone(),
                    ratelimit::Limiter::new(10, std::time::Duration::from_secs(300)),
                ),
                ratelimit::limit,
            )),
        )
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/tools", get(tools::list))
        .route("/tools/{name}", post(tools::call))
        .route("/proposals/{id}/approve", post(tools::approve_proposal))
        .route("/proposals/{id}/decline", post(tools::decline_proposal))
        .route("/actions/{id}/undo", post(tools::undo_action))
        .route("/n/{token}", get(links::act))
        .route(
            "/chat/threads",
            get(chat::threads).post(chat::create_thread),
        )
        .route("/chat/threads/{id}", get(chat::messages))
        .route("/chat/threads/{id}/messages", post(chat::send))
        .route("/assistant/pass", post(chat::pass_now))
        .route("/integrations/status", get(integrations::status))
        .route(
            "/integrations/{name}",
            put(integrations::configure).delete(integrations::unconfigure),
        )
        .route("/integrations/{name}/test", post(integrations::test))
        .route("/plaid/link-token", post(integrations::plaid_link_token))
        .route("/plaid/exchange", post(integrations::plaid_exchange))
        .route("/plaid/items/{id}/sync", post(integrations::plaid_sync))
        .route(
            "/plaid/items/{id}",
            axum::routing::delete(integrations::plaid_remove),
        )
        .route("/plaid/webhook", post(integrations::plaid_webhook))
        .route("/privacy/sync", post(integrations::privacy_sync))
        .route("/import/csv/headers", post(integrations::csv_headers))
        .route("/import/csv/preview", post(integrations::csv_preview))
        .route("/import/csv", post(integrations::csv_import))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
        .route(
            "/import/batches/{id}/rollback",
            post(integrations::csv_rollback),
        )
        .route("/import/presets", post(integrations::presets_save))
        .route(
            "/import/presets/{name}",
            axum::routing::delete(integrations::presets_delete),
        )
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
        .route("/settings/server", get(server::get).put(server::put))
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
