//! Integration credentials: set in the app (stored encrypted in `secrets`,
//! one JSON document per integration) or, as a fallback, taken from the
//! environment. The app's copy wins when both exist.

use serde::{Serialize, de::DeserializeOwned};

use super::{Actor, Ctx, Result, invalid, secrets};
use crate::config::{Config, NtfyConfig, PlaidConfig, PrivacyConfig, SmtpConfig};

/// Where an effective configuration came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    App,
    Env,
}

pub const PLAID: &str = "integration_plaid";
pub const PRIVACY: &str = "integration_privacy";
pub const NTFY: &str = "integration_ntfy";
pub const SMTP: &str = "integration_smtp";

/// The secret name for a public integration name (`plaid` → `integration_plaid`).
pub fn secret_name(name: &str) -> Result<&'static str> {
    Ok(match name {
        "plaid" => PLAID,
        "privacy" => PRIVACY,
        "ntfy" => NTFY,
        "smtp" => SMTP,
        other => return Err(invalid(format!("unknown integration {other:?}"))),
    })
}

/// The app's stored copy, if any. Without an encryption key nothing can be
/// stored, so this is `None` rather than an error.
pub async fn stored<T: DeserializeOwned>(ctx: &Ctx, name: &str) -> Result<Option<T>> {
    if ctx.crypto.is_none() {
        return Ok(None);
    }
    match secrets::get(ctx, name).await? {
        None => Ok(None),
        Some(json) => Ok(Some(serde_json::from_str(&json).map_err(|e| {
            anyhow::anyhow!("stored {name} settings are invalid: {e}")
        })?)),
    }
}

async fn resolve<T: DeserializeOwned + Clone>(
    ctx: &Ctx,
    name: &str,
    env: Option<&T>,
) -> Result<Option<(T, Source)>> {
    if let Some(v) = stored::<T>(ctx, name).await? {
        return Ok(Some((v, Source::App)));
    }
    Ok(env.map(|v| (v.clone(), Source::Env)))
}

pub async fn plaid(ctx: &Ctx, env: &Config) -> Result<Option<(PlaidConfig, Source)>> {
    resolve(ctx, PLAID, env.plaid.as_ref()).await
}

pub async fn privacy(ctx: &Ctx, env: &Config) -> Result<Option<(PrivacyConfig, Source)>> {
    resolve(ctx, PRIVACY, env.privacy.as_ref()).await
}

pub async fn ntfy(ctx: &Ctx, env: &Config) -> Result<Option<(NtfyConfig, Source)>> {
    resolve(ctx, NTFY, env.ntfy.as_ref()).await
}

pub async fn smtp(ctx: &Ctx, env: &Config) -> Result<Option<(SmtpConfig, Source)>> {
    resolve(ctx, SMTP, env.smtp.as_ref()).await
}

/// Store the app's copy (encrypted) and audit that it changed; values are never
/// written to the audit log.
pub async fn save<T: Serialize>(ctx: &Ctx, actor: &Actor, name: &str, value: &T) -> Result<()> {
    let json = serde_json::to_string(value).map_err(anyhow::Error::from)?;
    secrets::set(ctx, name, &json).await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "integration.set",
        "secrets",
        name,
        None::<&()>,
        None::<&()>,
    )
    .await?;
    Ok(())
}

/// Forget the app's copy; the environment, if set, applies again.
pub async fn clear(ctx: &Ctx, actor: &Actor, name: &str) -> Result<()> {
    secrets::delete(ctx, name).await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "integration.clear",
        "secrets",
        name,
        None::<&()>,
        None::<&()>,
    )
    .await?;
    Ok(())
}
