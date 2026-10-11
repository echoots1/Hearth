use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ApiError, AppState};
use crate::auth::api_keys;
use crate::auth::extract::AuthUser;
use crate::db::models::ApiKeyScope;
use crate::services::{self, Actor};

pub async fn list(State(state): State<AppState>, _user: AuthUser) -> Result<Json<Value>, ApiError> {
    let keys = api_keys::list(&state.pool, state.household_id).await?;
    Ok(Json(serde_json::to_value(keys)?))
}

#[derive(Deserialize)]
pub struct CreateKey {
    name: String,
    scope: ApiKeyScope,
}

/// Owners only. The plaintext key is in this response and nowhere else.
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(b): Json<CreateKey>,
) -> Result<Json<Value>, ApiError> {
    if !user.is_owner() {
        return Err(ApiError::forbidden("only the owner can create API keys"));
    }
    if b.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let (row, plaintext) = api_keys::create(
        &state.pool,
        state.household_id,
        &b.name,
        b.scope,
        Some(user.0.id),
    )
    .await?;
    let ctx = state.ctx().await?;
    services::audit::record(
        &ctx.pool,
        ctx.household_id,
        &user.actor(),
        "api_key.create",
        "api_keys",
        row.id,
        None::<&()>,
        Some(&row),
    )
    .await?;
    Ok(Json(json!({ "key": row, "plaintext": plaintext })))
}

pub async fn revoke(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !user.is_owner() {
        return Err(ApiError::forbidden("only the owner can revoke API keys"));
    }
    let ok = api_keys::revoke(&state.pool, state.household_id, id).await?;
    if !ok {
        return Err(ApiError::not_found());
    }
    let ctx = state.ctx().await?;
    services::audit::record(
        &ctx.pool,
        ctx.household_id,
        &user.actor(),
        "api_key.revoke",
        "api_keys",
        id,
        None::<&()>,
        None::<&()>,
    )
    .await?;
    let _ = Actor::System;
    Ok(Json(json!({ "revoked": true })))
}
