use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ApiError, AppState};
use crate::auth::extract::AuthUser;
use crate::auth::{passwords, sessions};
use crate::db::models::{User, UserRole};
use crate::services::{self, secrets};

#[derive(Serialize)]
pub struct UserInfo {
    id: Uuid,
    email: String,
    display_name: String,
    role: UserRole,
}

pub async fn list(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<Json<Vec<UserInfo>>, ApiError> {
    let rows = sqlx::query_as::<_, User>(
        "select * from users where household_id = $1 order by created_at",
    )
    .bind(state.household_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|u| UserInfo {
                id: u.id,
                email: u.email,
                display_name: u.display_name,
                role: u.role,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct CreateUser {
    email: String,
    display_name: String,
    password: String,
    role: Option<UserRole>,
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(b): Json<CreateUser>,
) -> Result<Json<UserInfo>, ApiError> {
    if !user.is_owner() {
        return Err(ApiError::forbidden("only the owner can add people"));
    }
    passwords::validate(&b.password).map_err(ApiError::bad_request)?;
    if !b.email.contains('@') {
        return Err(ApiError::bad_request("a valid email is required"));
    }
    let hash = passwords::hash(&b.password)
        .map_err(|_| ApiError::bad_request("could not hash password"))?;
    let u = sqlx::query_as::<_, User>(
        "insert into users (household_id, email, display_name, password_hash, role) values ($1, $2, $3, $4, $5) returning *",
    )
    .bind(state.household_id)
    .bind(b.email.trim().to_lowercase())
    .bind(b.display_name.trim())
    .bind(hash)
    .bind(b.role.unwrap_or(UserRole::Member))
    .fetch_one(&state.pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => ApiError { status: axum::http::StatusCode::CONFLICT, message: "that email already has an account".into() },
        _ => e.into(),
    })?;
    let ctx = state.ctx().await?;
    services::audit::record(
        &ctx.pool,
        ctx.household_id,
        &user.actor(),
        "user.create",
        "users",
        u.id,
        None::<&()>,
        Some(&json!({ "email": u.email, "role": u.role })),
    )
    .await?;
    Ok(Json(UserInfo {
        id: u.id,
        email: u.email,
        display_name: u.display_name,
        role: u.role,
    }))
}

#[derive(Deserialize)]
pub struct SetPassword {
    password: String,
}

/// A person changes their own password; the owner can reset anyone's. Signs out other sessions.
pub async fn set_password(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(b): Json<SetPassword>,
) -> Result<Json<Value>, ApiError> {
    if id != user.0.id && !user.is_owner() {
        return Err(ApiError::forbidden("you can only change your own password"));
    }
    passwords::validate(&b.password).map_err(ApiError::bad_request)?;
    let hash = passwords::hash(&b.password)
        .map_err(|_| ApiError::bad_request("could not hash password"))?;
    let n = sqlx::query("update users set password_hash = $2, updated_at = now() where id = $1 and household_id = $3")
        .bind(id)
        .bind(hash)
        .bind(state.household_id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found());
    }
    sessions::delete_all_for_user(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true, "signed_out": true })))
}

/// The owner removes a person: their sessions and push subscriptions go with
/// them (cascade); things they created stay, with the author cleared. Not
/// yourself, and never the last owner.
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !user.is_owner() {
        return Err(ApiError::forbidden("only the owner can remove people"));
    }
    if id == user.0.id {
        return Err(ApiError::bad_request(
            "you cannot remove yourself; have another owner do it",
        ));
    }
    let target =
        sqlx::query_as::<_, User>("select * from users where id = $1 and household_id = $2")
            .bind(id)
            .bind(state.household_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(ApiError::not_found)?;
    if target.role == UserRole::Owner {
        let owners: i64 = sqlx::query_scalar(
            "select count(*) from users where household_id = $1 and role = 'owner'",
        )
        .bind(state.household_id)
        .fetch_one(&state.pool)
        .await?;
        if owners <= 1 {
            return Err(ApiError::bad_request(
                "the household needs at least one owner",
            ));
        }
    }
    sqlx::query("delete from users where id = $1 and household_id = $2")
        .bind(id)
        .bind(state.household_id)
        .execute(&state.pool)
        .await?;
    let ctx = state.ctx().await?;
    services::audit::record(
        &ctx.pool,
        ctx.household_id,
        &user.actor(),
        "user.delete",
        "users",
        id,
        Some(&json!({ "email": target.email, "role": target.role })),
        None::<&()>,
    )
    .await?;
    Ok(Json(json!({ "removed": true })))
}

/// The assistant setting (mode, autonomy, models, cap) plus whether an API key is stored and this month's spend.
pub async fn assistant_settings(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let s = services::settings::load(&ctx).await?;
    let has_key = ctx.crypto.is_some() && secrets::exists(&ctx, secrets::ANTHROPIC_API_KEY).await?;
    let (spent, calls) = services::assistant::usage_this_month(&ctx).await?;
    Ok(Json(
        json!({ "assistant": s.assistant, "has_api_key": has_key, "encryption_configured": ctx.crypto.is_some(), "month_spent_cents": spent, "month_calls": calls }),
    ))
}

/// UI-only: the autonomy switch and mode. Never reachable through the tool registry.
pub async fn set_assistant_settings(
    State(state): State<AppState>,
    user: AuthUser,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    if !user.is_owner() {
        return Err(ApiError::forbidden(
            "only the owner can change the assistant settings",
        ));
    }
    let ctx = state.ctx().await?;
    let out = services::settings::set(&ctx, &user.actor(), "assistant", v).await?;
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct KeyBody {
    api_key: String,
}

pub async fn set_anthropic_key(
    State(state): State<AppState>,
    user: AuthUser,
    Json(b): Json<KeyBody>,
) -> Result<Json<Value>, ApiError> {
    if !user.is_owner() {
        return Err(ApiError::forbidden("only the owner can set the API key"));
    }
    let k = b.api_key.trim();
    if !k.starts_with("sk-ant-") || k.len() < 20 {
        return Err(ApiError::bad_request(
            "that doesn't look like an Anthropic API key",
        ));
    }
    let ctx = state.ctx().await?;
    secrets::set(&ctx, secrets::ANTHROPIC_API_KEY, k).await?;
    services::audit::record(
        &ctx.pool,
        ctx.household_id,
        &user.actor(),
        "secret.set",
        "secrets",
        secrets::ANTHROPIC_API_KEY,
        None::<&()>,
        None::<&()>,
    )
    .await?;
    Ok(Json(json!({ "stored": true })))
}

pub async fn clear_anthropic_key(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    if !user.is_owner() {
        return Err(ApiError::forbidden("only the owner can clear the API key"));
    }
    let ctx = state.ctx().await?;
    secrets::delete(&ctx, secrets::ANTHROPIC_API_KEY).await?;
    services::audit::record(
        &ctx.pool,
        ctx.household_id,
        &user.actor(),
        "secret.delete",
        "secrets",
        secrets::ANTHROPIC_API_KEY,
        None::<&()>,
        None::<&()>,
    )
    .await?;
    Ok(Json(json!({ "stored": false })))
}
