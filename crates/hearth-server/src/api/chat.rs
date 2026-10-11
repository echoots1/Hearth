//! API mode: chat threads and the pass on demand.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ApiError, AppState};
use crate::auth::extract::AuthUser;
use crate::services;

pub async fn threads(State(state): State<AppState>, _u: AuthUser) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    Ok(Json(serde_json::to_value(
        services::chat::threads(&ctx).await?,
    )?))
}

pub async fn create_thread(
    State(state): State<AppState>,
    AuthUser(u): AuthUser,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    Ok(Json(serde_json::to_value(
        services::chat::create_thread(&ctx, u.id).await?,
    )?))
}

pub async fn messages(
    State(state): State<AppState>,
    _u: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let thread = services::chat::get_thread(&ctx, id).await?;
    let messages = services::chat::messages(&ctx, id).await?;
    Ok(Json(json!({ "thread": thread, "messages": messages })))
}

#[derive(Deserialize)]
pub struct SendBody {
    text: String,
}

pub async fn send(
    State(state): State<AppState>,
    AuthUser(u): AuthUser,
    Path(id): Path<Uuid>,
    Json(b): Json<SendBody>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let rows = services::chat::send(&state, &ctx, &u, id, &b.text)
        .await
        .map_err(upstream)?;
    Ok(Json(serde_json::to_value(rows)?))
}

/// "Let Claude take a pass" from the UI, in API mode.
pub async fn pass_now(
    State(state): State<AppState>,
    _u: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    // Surface configuration problems before starting a job run.
    services::chat::client(&ctx).await?;
    let run = crate::jobs::run_named(state.clone(), "assistant:pass", "ui").await?;
    if run.status == crate::db::models::JobRunStatus::Failed
        && let Some(e) = &run.error
    {
        return Err(ApiError {
            status: axum::http::StatusCode::BAD_GATEWAY,
            message: e.clone(),
        });
    }
    Ok(Json(serde_json::to_value(run)?))
}

/// A failure talking to Claude is the caller's to see (bad key, cap, outage), not a 500.
fn upstream(e: services::Error) -> ApiError {
    match e {
        services::Error::Other(inner) => ApiError {
            status: axum::http::StatusCode::BAD_GATEWAY,
            message: inner.to_string(),
        },
        other => other.into(),
    }
}
