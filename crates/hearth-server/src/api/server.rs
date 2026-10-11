//! `/api/settings/server`: the owner's server settings and what is in effect.

use axum::{Json, extract::State};
use serde_json::{Value, json};

use super::{ApiError, AppState};
use crate::auth::extract::AuthUser;
use crate::services::server::{self, ServerSettings};

async fn view(state: &AppState, settings: &ServerSettings) -> Result<Value, ApiError> {
    let runtime = state.runtime();
    let c = &state.config;
    Ok(json!({
        "settings": settings,
        "effective": *runtime,
        "env": {
            "app_url": c.app_url,
            "trust_proxy": c.trust_proxy,
            "mcp_allowed_hosts": c.mcp_allowed_hosts,
            "evaluate_minutes": c.jobs.evaluate_minutes,
            "plaid_sync_minutes": c.jobs.plaid_sync_minutes,
            "privacy_sync_minutes": c.jobs.privacy_sync_minutes,
            "jobs_enabled": c.jobs.enabled,
            "port": c.port,
        },
        "secrets": {
            "app_secret": c.app_secret_source,
            "app_encryption_key": c.app_encryption_key_source,
            "data_dir": c.data_dir.display().to_string(),
        },
    }))
}

/// Owner only: the view names environment values and the data directory.
pub async fn get(State(state): State<AppState>, u: AuthUser) -> Result<Json<Value>, ApiError> {
    if !u.is_owner() {
        return Err(ApiError::forbidden(
            "only the owner can see the server settings",
        ));
    }
    let ctx = state.ctx().await?;
    let settings = server::load(&ctx).await?;
    Ok(Json(view(&state, &settings).await?))
}

pub async fn put(
    State(state): State<AppState>,
    u: AuthUser,
    Json(body): Json<ServerSettings>,
) -> Result<Json<Value>, ApiError> {
    if !u.is_owner() {
        return Err(ApiError::forbidden(
            "only the owner can change server settings",
        ));
    }
    let ctx = state.ctx().await?;
    let saved = server::save(&ctx, &u.actor(), body).await?;
    state.reload_runtime().await?;
    Ok(Json(view(&state, &saved).await?))
}
