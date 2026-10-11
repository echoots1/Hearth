use axum::{Json, extract::State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, AppState};
use crate::auth::extract::AuthUser;
use crate::services::{self, secrets};

/// The VAPID public key the browser needs to subscribe; generated on first ask.
pub async fn vapid_public_key(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    if ctx.crypto.is_none() {
        return Err(ApiError::bad_request(
            "push needs APP_ENCRYPTION_KEY to store the VAPID key",
        ));
    }
    let existing = secrets::get(&ctx, secrets::VAPID_PUBLIC_KEY).await?;
    let public = match existing {
        Some(p) => p,
        None => {
            let (public, private) = crate::integrations::webpush::generate_vapid()?;
            secrets::set(&ctx, secrets::VAPID_PRIVATE_KEY, &private).await?;
            secrets::set(&ctx, secrets::VAPID_PUBLIC_KEY, &public).await?;
            public
        }
    };
    Ok(Json(json!({ "public_key": public })))
}

pub async fn subscribe(
    State(state): State<AppState>,
    AuthUser(u): AuthUser,
    Json(b): Json<services::push::Subscribe>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let s = services::push::subscribe(&ctx, u.id, b).await?;
    Ok(Json(json!({ "id": s.id })))
}

#[derive(Deserialize)]
pub struct Unsub {
    endpoint: String,
}

pub async fn unsubscribe(
    State(state): State<AppState>,
    AuthUser(u): AuthUser,
    Json(b): Json<Unsub>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let removed = services::push::unsubscribe(&ctx, u.id, &b.endpoint).await?;
    Ok(Json(json!({ "removed": removed })))
}
