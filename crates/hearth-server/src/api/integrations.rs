//! Integrations: status, Plaid Link, Privacy.com sync, CSV import.

use axum::extract::{Path, State};
use axum::{Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ApiError, AppState};
use crate::auth::extract::AuthUser;
use crate::integrations::plaid::Plaid;
use crate::services::{self, csv_import};

fn owner_only(u: &AuthUser) -> Result<(), ApiError> {
    if u.is_owner() {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "only the owner can change integrations",
        ))
    }
}

fn plaid_client(state: &AppState) -> Result<Plaid, ApiError> {
    let cfg = state.config.plaid.as_ref().ok_or_else(|| {
        ApiError::bad_request("Plaid is not configured (PLAID_CLIENT_ID / PLAID_SECRET)")
    })?;
    Plaid::new(cfg).map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn status(State(state): State<AppState>, _u: AuthUser) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let items = services::plaid::items(&ctx).await?;
    let mut plaid_items = Vec::new();
    for it in items {
        let accounts: Vec<Value> = sqlx::query_as::<_, crate::db::models::Account>("select * from accounts where plaid_item_id = $1 order by name")
            .bind(it.id)
            .fetch_all(&state.pool)
            .await?
            .into_iter()
            .map(|a| json!({ "id": a.id, "name": a.name, "type": a.account_type, "mask": a.mask, "is_active": a.is_active, "bank_balance_cents": a.bank_balance_cents, "bank_balance_as_of": a.bank_balance_as_of }))
            .collect();
        plaid_items.push(json!({
            "id": it.id, "institution_name": it.institution_name, "status": it.status, "last_error": it.last_error,
            "last_synced_at": it.last_synced_at, "consecutive_failures": it.consecutive_failures, "created_at": it.created_at, "accounts": accounts,
        }));
    }
    let privacy_cards: i64 = sqlx::query_scalar(
        "select count(*) from accounts where household_id = $1 and source = 'privacy'",
    )
    .bind(state.household_id)
    .fetch_one(&state.pool)
    .await?;
    let last = |job: &str| {
        let pool = state.pool.clone();
        let job = job.to_string();
        async move {
            sqlx::query_as::<_, crate::db::models::JobRun>("select * from job_runs where job = $1 and status <> 'skipped' order by started_at desc limit 1")
                .bind(job)
                .fetch_optional(&pool)
                .await
        }
    };
    let batches = csv_import::batches(&ctx).await?;
    let presets = csv_import::presets(&ctx).await?;
    Ok(Json(json!({
        "plaid": { "configured": state.config.plaid.is_some(), "env": state.config.plaid.as_ref().map(|p| p.env.clone()), "encryption_configured": state.crypto.is_some(), "items": plaid_items, "last_sync": last("sync:plaid").await? },
        "privacy": { "configured": state.config.privacy.is_some(), "env": state.config.privacy.as_ref().map(|p| p.env.clone()), "cards": privacy_cards, "last_sync": last("sync:privacy").await? },
        "csv": { "presets": presets, "batches": batches },
        "ntfy": state.config.ntfy.is_some(),
        "smtp": state.config.smtp.is_some(),
    })))
}

pub async fn plaid_link_token(
    State(state): State<AppState>,
    u: AuthUser,
) -> Result<Json<Value>, ApiError> {
    owner_only(&u)?;
    if state.crypto.is_none() {
        return Err(ApiError::bad_request(
            "APP_ENCRYPTION_KEY is required before linking a bank",
        ));
    }
    let client = plaid_client(&state)?;
    let token = client
        .link_token(&u.0.id.to_string(), &state.config.app_url)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({ "link_token": token })))
}

#[derive(Deserialize)]
pub struct Exchange {
    public_token: String,
}

/// Link succeeded in the browser: store the item, create accounts, pull history.
pub async fn plaid_exchange(
    State(state): State<AppState>,
    u: AuthUser,
    Json(b): Json<Exchange>,
) -> Result<Json<Value>, ApiError> {
    owner_only(&u)?;
    let client = plaid_client(&state)?;
    let ctx = state.ctx().await?;
    let actor = u.actor();
    let (item, accounts) = services::plaid::link(&ctx, &actor, &client, &b.public_token).await?;
    let sync = services::plaid::sync_item(&ctx, &actor, &client, &item)
        .await
        .ok();
    let _ = crate::jobs::run_named(state.clone(), "process", "plaid-link").await;
    Ok(Json(
        json!({ "item": item, "accounts": accounts, "sync": sync }),
    ))
}

pub async fn plaid_sync(
    State(state): State<AppState>,
    u: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let client = plaid_client(&state)?;
    let ctx = state.ctx().await?;
    let item = services::plaid::get_item(&ctx, id).await?;
    let s = services::plaid::sync_item(&ctx, &u.actor(), &client, &item).await?;
    let _ = crate::jobs::run_named(state.clone(), "process", "plaid-sync").await;
    Ok(Json(serde_json::to_value(s)?))
}

pub async fn plaid_remove(
    State(state): State<AppState>,
    u: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    owner_only(&u)?;
    let client = plaid_client(&state)?;
    let ctx = state.ctx().await?;
    services::plaid::remove_item(&ctx, &u.actor(), &client, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Plaid posts item and transaction webhooks here. Deliveries are a hint to
/// sync sooner; the scheduled sync is the source of truth, so this only logs.
pub async fn plaid_webhook(Json(v): Json<Value>) -> StatusCode {
    tracing::info!(webhook_type = %v.get("webhook_type").and_then(|w| w.as_str()).unwrap_or("?"), code = %v.get("webhook_code").and_then(|w| w.as_str()).unwrap_or("?"), "plaid webhook");
    StatusCode::OK
}

pub async fn privacy_sync(
    State(state): State<AppState>,
    _u: AuthUser,
) -> Result<Json<Value>, ApiError> {
    if state.config.privacy.is_none() {
        return Err(ApiError::bad_request(
            "Privacy.com is not configured (PRIVACY_API_KEY)",
        ));
    }
    let run = crate::jobs::run_named(state.clone(), "sync:privacy", "ui").await?;
    Ok(Json(serde_json::to_value(run)?))
}

#[derive(Deserialize)]
pub struct CsvBody {
    account_id: Uuid,
    #[serde(default)]
    filename: Option<String>,
    text: String,
    mapping: csv_import::Mapping,
    /// Save the mapping under this preset name on import.
    #[serde(default)]
    save_preset: Option<String>,
}

pub async fn csv_preview(
    State(state): State<AppState>,
    _u: AuthUser,
    Json(b): Json<CsvBody>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let p = csv_import::preview(&ctx, b.account_id, &b.text, &b.mapping).await?;
    Ok(Json(serde_json::to_value(p)?))
}

pub async fn csv_import(
    State(state): State<AppState>,
    u: AuthUser,
    Json(b): Json<CsvBody>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let actor = u.actor();
    let batch = csv_import::commit(
        &ctx,
        &actor,
        b.account_id,
        b.filename.as_deref().unwrap_or("upload.csv"),
        &b.text,
        &b.mapping,
    )
    .await?;
    if let Some(name) = b.save_preset.filter(|n| !n.trim().is_empty()) {
        csv_import::save_preset(
            &ctx,
            &actor,
            csv_import::Preset {
                name,
                mapping: b.mapping.clone(),
                account_id: Some(b.account_id),
            },
        )
        .await?;
    }
    let _ = crate::jobs::run_named(state.clone(), "process", "csv-import").await;
    Ok(Json(serde_json::to_value(batch)?))
}

/// Just the headers, so the UI can offer a column picker before mapping.
#[derive(Deserialize)]
pub struct HeadersBody {
    text: String,
    #[serde(default = "t")]
    has_header: bool,
}
fn t() -> bool {
    true
}

pub async fn csv_headers(
    _u: AuthUser,
    Json(b): Json<HeadersBody>,
) -> Result<Json<Value>, ApiError> {
    let (headers, rows) = csv_import::read(&b.text, b.has_header).map_err(ApiError::bad_request)?;
    let sample: Vec<&Vec<String>> = rows.iter().take(3).collect();
    Ok(Json(
        json!({ "headers": headers, "rows": rows.len(), "sample": sample }),
    ))
}

pub async fn csv_rollback(
    State(state): State<AppState>,
    u: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let b = csv_import::rollback(&ctx, &u.actor(), id).await?;
    Ok(Json(serde_json::to_value(b)?))
}

pub async fn presets_save(
    State(state): State<AppState>,
    u: AuthUser,
    Json(p): Json<csv_import::Preset>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    Ok(Json(serde_json::to_value(
        csv_import::save_preset(&ctx, &u.actor(), p).await?,
    )?))
}

pub async fn presets_delete(
    State(state): State<AppState>,
    u: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    Ok(Json(serde_json::to_value(
        csv_import::delete_preset(&ctx, &u.actor(), &name).await?,
    )?))
}
