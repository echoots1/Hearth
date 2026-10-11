//! Integrations: status, Plaid Link, Privacy.com sync, CSV import.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::{Json, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ApiError, AppState};
use crate::auth::extract::AuthUser;
use crate::config::{NtfyConfig, PlaidConfig, PrivacyConfig, SmtpConfig};
use crate::integrations::plaid::Plaid;
use crate::integrations::privacy::Privacy;
use crate::integrations::{ntfy, smtp};
use crate::services::integrations::{self as integ, Source};
use crate::services::{self, Ctx, csv_import};

fn owner_only(u: &AuthUser) -> Result<(), ApiError> {
    if u.is_owner() {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "only the owner can change integrations",
        ))
    }
}

async fn plaid_client(state: &AppState, ctx: &Ctx) -> Result<Plaid, ApiError> {
    let (cfg, _) = integ::plaid(ctx, &state.config).await?.ok_or_else(|| {
        ApiError::bad_request("Plaid is not configured: add your keys under More → Integrations")
    })?;
    Plaid::new(&cfg).map_err(|e| ApiError::bad_request(e.to_string()))
}

fn source_json(s: Option<Source>) -> Value {
    match s {
        Some(Source::App) => json!("app"),
        Some(Source::Env) => json!("env"),
        None => Value::Null,
    }
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
    let plaid = integ::plaid(&ctx, &state.config).await?;
    let privacy = integ::privacy(&ctx, &state.config).await?;
    // Where card charges land, or why they can't yet.
    let (funding, funding_error) = match &privacy {
        Some((cfg, _)) => {
            match services::privacy::funding_account(&ctx, cfg.funding_account_id).await {
                Ok(a) => (
                    Some(json!({ "id": a.id, "name": a.name, "mask": a.mask })),
                    None,
                ),
                Err(services::Error::Invalid(m)) => (None, Some(m)),
                Err(e) => return Err(e.into()),
            }
        }
        None => (None, None),
    };
    let ntfy = integ::ntfy(&ctx, &state.config).await?;
    let smtp = integ::smtp(&ctx, &state.config).await?;
    let encryption_configured = state.crypto.is_some();
    Ok(Json(json!({
        "encryption_configured": encryption_configured,
        "plaid": {
            "configured": plaid.is_some(), "source": source_json(plaid.as_ref().map(|(_, s)| *s)),
            "env": plaid.as_ref().map(|(p, _)| p.env.clone()), "client_id": plaid.as_ref().map(|(p, _)| p.client_id.clone()),
            "products": plaid.as_ref().map(|(p, _)| p.products.clone()), "country_codes": plaid.as_ref().map(|(p, _)| p.country_codes.clone()),
            "encryption_configured": encryption_configured, "items": plaid_items, "last_sync": last("sync:plaid").await?,
        },
        "privacy": {
            "configured": privacy.is_some(), "source": source_json(privacy.as_ref().map(|(_, s)| *s)),
            "env": privacy.as_ref().map(|(p, _)| p.env.clone()),
            "funding_account_id": privacy.as_ref().and_then(|(p, _)| p.funding_account_id),
            "funding_account": funding, "funding_error": funding_error, "last_sync": last("sync:privacy").await?,
        },
        "ntfy": {
            "configured": ntfy.is_some(), "source": source_json(ntfy.as_ref().map(|(_, s)| *s)),
            "url": ntfy.as_ref().map(|(n, _)| n.url.clone()), "topic": ntfy.as_ref().map(|(n, _)| n.topic.clone()),
            "has_token": ntfy.as_ref().is_some_and(|(n, _)| n.token.is_some()),
        },
        "smtp": {
            "configured": smtp.is_some(), "source": source_json(smtp.as_ref().map(|(_, s)| *s)),
            "host": smtp.as_ref().map(|(s, _)| s.host.clone()), "port": smtp.as_ref().map(|(s, _)| s.port),
            "user": smtp.as_ref().and_then(|(s, _)| s.user.clone()), "has_pass": smtp.as_ref().is_some_and(|(s, _)| s.pass.is_some()),
            "from": smtp.as_ref().map(|(s, _)| s.from.clone()), "alert_to": smtp.as_ref().map(|(s, _)| s.alert_to.clone()),
        },
        "csv": { "presets": presets, "batches": batches },
    })))
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
}

fn list(v: &Value, key: &str) -> Option<Vec<String>> {
    match v.get(key) {
        Some(Value::Array(a)) => Some(
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect(),
        ),
        Some(Value::String(t)) => Some(
            t.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect(),
        ),
        _ => None,
    }
    .filter(|v: &Vec<String>| !v.is_empty())
}

fn need(v: &Value, key: &str, label: &str) -> Result<String, ApiError> {
    s(v, key).ok_or_else(|| ApiError::bad_request(format!("{label} is required")))
}

/// Save an integration's settings in the app. A secret field may be left
/// empty to keep the one already stored.
pub async fn configure(
    State(state): State<AppState>,
    u: AuthUser,
    Path(name): Path<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    owner_only(&u)?;
    if state.crypto.is_none() {
        return Err(ApiError::bad_request(
            "APP_ENCRYPTION_KEY is not set, so secrets cannot be stored; set it and restart",
        ));
    }
    let ctx = state.ctx().await?;
    let actor = u.actor();
    match name.as_str() {
        "plaid" => {
            let prev: Option<PlaidConfig> = integ::stored(&ctx, integ::PLAID).await?;
            let cfg = PlaidConfig {
                client_id: need(&b, "client_id", "Client ID")?,
                secret: s(&b, "secret")
                    .or_else(|| prev.as_ref().map(|p| p.secret.clone()))
                    .ok_or_else(|| ApiError::bad_request("Secret is required"))?,
                env: s(&b, "env").unwrap_or_else(|| "sandbox".into()),
                products: list(&b, "products").unwrap_or_else(|| vec!["transactions".into()]),
                country_codes: list(&b, "country_codes").unwrap_or_else(|| vec!["US".into()]),
            };
            Plaid::new(&cfg).map_err(|e| ApiError::bad_request(e.to_string()))?;
            integ::save(&ctx, &actor, integ::PLAID, &cfg).await?;
        }
        "privacy" => {
            let prev: Option<PrivacyConfig> = integ::stored(&ctx, integ::PRIVACY).await?;
            let funding_account_id = match s(&b, "funding_account_id") {
                None => None,
                Some(id) => {
                    let id: uuid::Uuid = id
                        .parse()
                        .map_err(|_| ApiError::bad_request("Funding account is not valid"))?;
                    // Must be one of the household's bank accounts.
                    services::privacy::funding_account(&ctx, Some(id)).await?;
                    Some(id)
                }
            };
            let cfg = PrivacyConfig {
                api_key: s(&b, "api_key")
                    .or_else(|| prev.as_ref().map(|p| p.api_key.clone()))
                    .ok_or_else(|| ApiError::bad_request("API key is required"))?,
                env: s(&b, "env").unwrap_or_else(|| "sandbox".into()),
                funding_account_id,
            };
            Privacy::new(&cfg).map_err(|e| ApiError::bad_request(e.to_string()))?;
            integ::save(&ctx, &actor, integ::PRIVACY, &cfg).await?;
        }
        "ntfy" => {
            let prev: Option<NtfyConfig> = integ::stored(&ctx, integ::NTFY).await?;
            let url = s(&b, "url").unwrap_or_else(|| "https://ntfy.sh".into());
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(ApiError::bad_request(
                    "Server URL must start with http:// or https://",
                ));
            }
            let cfg = NtfyConfig {
                url: url.trim_end_matches('/').to_string(),
                topic: need(&b, "topic", "Topic")?,
                token: s(&b, "token").or_else(|| prev.and_then(|p| p.token)),
            };
            integ::save(&ctx, &actor, integ::NTFY, &cfg).await?;
        }
        "smtp" => {
            let prev: Option<SmtpConfig> = integ::stored(&ctx, integ::SMTP).await?;
            let port = match b.get("port") {
                None | Some(Value::Null) => 587,
                Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u16,
                Some(Value::String(t)) if t.trim().is_empty() => 587,
                Some(Value::String(t)) => t
                    .trim()
                    .parse::<u16>()
                    .map_err(|_| ApiError::bad_request("Port must be a number"))?,
                _ => return Err(ApiError::bad_request("Port must be a number")),
            };
            if port == 0 {
                return Err(ApiError::bad_request("Port must be between 1 and 65535"));
            }
            let cfg = SmtpConfig {
                host: need(&b, "host", "SMTP host")?,
                port,
                user: s(&b, "user"),
                pass: s(&b, "pass").or_else(|| prev.and_then(|p| p.pass)),
                from: need(&b, "from", "From address")?,
                alert_to: need(&b, "alert_to", "Send alerts to")?,
            };
            for (label, addr) in [
                ("From address", &cfg.from),
                ("Send alerts to", &cfg.alert_to),
            ] {
                addr.parse::<lettre::message::Mailbox>()
                    .map_err(|e| ApiError::bad_request(format!("{label}: {e}")))?;
            }
            integ::save(&ctx, &actor, integ::SMTP, &cfg).await?;
        }
        other => {
            return Err(ApiError::bad_request(format!(
                "unknown integration {other:?}"
            )));
        }
    }
    Ok(Json(json!({ "stored": true })))
}

/// Forget the app's copy. The environment, if set, applies again.
pub async fn unconfigure(
    State(state): State<AppState>,
    u: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    owner_only(&u)?;
    let secret = integ::secret_name(&name)?;
    let ctx = state.ctx().await?;
    integ::clear(&ctx, &u.actor(), secret).await?;
    Ok(Json(json!({ "stored": false })))
}

/// Exercise the effective configuration once and report what happened.
pub async fn test(
    State(state): State<AppState>,
    u: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    owner_only(&u)?;
    let ctx = state.ctx().await?;
    let bad = |e: anyhow::Error| ApiError::bad_request(e.to_string());
    let detail = match name.as_str() {
        "plaid" => {
            let client = plaid_client(&state, &ctx).await?;
            client
                .link_token(&u.0.id.to_string(), &state.runtime().app_url)
                .await
                .map_err(bad)?;
            "Plaid accepted the keys (a Link token was issued).".to_string()
        }
        "privacy" => {
            let (cfg, _) = integ::privacy(&ctx, &state.config)
                .await?
                .ok_or_else(|| ApiError::bad_request("Privacy.com is not configured"))?;
            let client = Privacy::new(&cfg).map_err(bad)?;
            let cards = client.cards().await.map_err(bad)?;
            format!(
                "Privacy.com answered: {} card{}.",
                cards.len(),
                if cards.len() == 1 { "" } else { "s" }
            )
        }
        "ntfy" => {
            let (cfg, _) = integ::ntfy(&ctx, &state.config)
                .await?
                .ok_or_else(|| ApiError::bad_request("ntfy is not configured"))?;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|e| bad(e.into()))?;
            let msg = ntfy::Message {
                title: "Hearth test",
                body: "Notifications from Hearth will arrive here.",
                url: Some(&state.runtime().app_url),
                priority: 3,
                tag: Some("fire"),
                actions: vec![],
            };
            ntfy::send(&client, &cfg, &msg).await.map_err(bad)?;
            format!("Sent a test message to topic {}.", cfg.topic)
        }
        "smtp" => {
            let (cfg, _) = integ::smtp(&ctx, &state.config)
                .await?
                .ok_or_else(|| ApiError::bad_request("email is not configured"))?;
            smtp::send(
                &cfg,
                "Hearth test",
                &format!(
                    "Alerts from Hearth will arrive here.

Open Hearth: {}
",
                    state.runtime().app_url
                ),
            )
            .await
            .map_err(bad)?;
            format!("Sent a test email to {}.", cfg.alert_to)
        }
        other => {
            return Err(ApiError::bad_request(format!(
                "unknown integration {other:?}"
            )));
        }
    };
    Ok(Json(json!({ "ok": true, "detail": detail })))
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
    let ctx = state.ctx().await?;
    let client = plaid_client(&state, &ctx).await?;
    let token = client
        .link_token(&u.0.id.to_string(), &state.runtime().app_url)
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
    let ctx = state.ctx().await?;
    let client = plaid_client(&state, &ctx).await?;
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
    let ctx = state.ctx().await?;
    let client = plaid_client(&state, &ctx).await?;
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
    let ctx = state.ctx().await?;
    let client = plaid_client(&state, &ctx).await?;
    services::plaid::remove_item(&ctx, &u.actor(), &client, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Plaid posts item and transaction webhooks here. Every delivery must carry a
/// valid `Plaid-Verification` signature; a transactions or item webhook then
/// brings the next sync forward. The scheduled sync stays the source of truth.
pub async fn plaid_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let ctx = state.ctx().await?;
    let Some((cfg, _)) = integ::plaid(&ctx, &state.config).await? else {
        return Err(ApiError::not_found());
    };
    let unauthorized = |m: &str| ApiError {
        status: StatusCode::UNAUTHORIZED,
        message: m.to_string(),
    };
    let jwt = headers
        .get("plaid-verification")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| unauthorized("missing Plaid-Verification header"))?;
    let client = Plaid::new(&cfg).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if let Err(e) = client
        .verify_webhook(jwt, &body, chrono::Utc::now().timestamp())
        .await
    {
        tracing::warn!(error = %e, "plaid webhook rejected");
        return Err(unauthorized("webhook verification failed"));
    }
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let kind = v
        .get("webhook_type")
        .and_then(|w| w.as_str())
        .unwrap_or("?")
        .to_string();
    let code = v
        .get("webhook_code")
        .and_then(|w| w.as_str())
        .unwrap_or("?")
        .to_string();
    tracing::info!(webhook_type = %kind, code = %code, "plaid webhook");
    if matches!(kind.as_str(), "TRANSACTIONS" | "ITEM") {
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::jobs::run_named(state, "sync:plaid", "webhook").await {
                tracing::warn!(error = %e, "plaid webhook: sync failed to start");
            }
        });
    }
    Ok(StatusCode::OK)
}

pub async fn privacy_sync(
    State(state): State<AppState>,
    _u: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    if integ::privacy(&ctx, &state.config).await?.is_none() {
        return Err(ApiError::bad_request(
            "Privacy.com is not configured: add your API key under More → Integrations",
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
