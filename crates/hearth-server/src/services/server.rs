//! Server settings the owner changes in the app: the public URL, whether a
//! reverse proxy sits in front, extra MCP hosts, and job intervals. Each is
//! optional; an unset field falls back to the environment (or the default).
//! The effective values live in `Runtime`, cached in `AppState` and rebuilt
//! after every save.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Actor, Ctx, Result, invalid};
use crate::config::{Config, default_mcp_hosts};

pub const KEY: &str = "server";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerSettings {
    pub app_url: Option<String>,
    pub trust_proxy: Option<bool>,
    /// Extra `Host` values the MCP endpoint accepts, on top of the derived ones.
    pub mcp_extra_hosts: Vec<String>,
    pub evaluate_minutes: Option<u64>,
    pub plaid_sync_minutes: Option<u64>,
    pub privacy_sync_minutes: Option<u64>,
}

/// What the server actually runs with right now.
#[derive(Clone, Debug, Serialize)]
pub struct Runtime {
    pub app_url: String,
    pub trust_proxy: bool,
    pub mcp_allowed_hosts: Vec<String>,
    pub evaluate_minutes: u64,
    pub plaid_sync_minutes: u64,
    pub privacy_sync_minutes: u64,
}

impl Runtime {
    pub fn is_https(&self) -> bool {
        self.app_url.starts_with("https://")
    }

    pub fn compute(config: &Config, s: &ServerSettings) -> Runtime {
        let app_url = s
            .app_url
            .clone()
            .unwrap_or_else(|| config.app_url.clone())
            .trim_end_matches('/')
            .to_string();
        let mut hosts = match &config.mcp_allowed_hosts {
            Some(list) => list.clone(),
            None => default_mcp_hosts(&app_url, config.port),
        };
        for h in &s.mcp_extra_hosts {
            let h = h.trim().to_ascii_lowercase();
            if !h.is_empty() && !hosts.contains(&h) {
                hosts.push(h);
            }
        }
        Runtime {
            app_url,
            trust_proxy: s.trust_proxy.unwrap_or(config.trust_proxy),
            mcp_allowed_hosts: hosts,
            evaluate_minutes: s
                .evaluate_minutes
                .unwrap_or(config.jobs.evaluate_minutes)
                .max(1),
            plaid_sync_minutes: s
                .plaid_sync_minutes
                .unwrap_or(config.jobs.plaid_sync_minutes)
                .max(5),
            privacy_sync_minutes: s
                .privacy_sync_minutes
                .unwrap_or(config.jobs.privacy_sync_minutes)
                .max(2),
        }
    }

    /// `Host` header check for the MCP endpoint: exact authority, or the bare
    /// host when the list names it without a port.
    pub fn host_allowed(&self, host: &str) -> bool {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() {
            return false;
        }
        let bare = match host.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.is_empty() => h.to_string(),
            _ => host.clone(),
        };
        self.mcp_allowed_hosts
            .iter()
            .any(|a| *a == host || *a == bare)
    }
}

pub async fn load(ctx: &Ctx) -> Result<ServerSettings> {
    load_from(&ctx.pool, ctx.household_id).await
}

pub async fn load_from(pool: &sqlx::PgPool, household_id: uuid::Uuid) -> Result<ServerSettings> {
    let row: Option<(Value,)> =
        sqlx::query_as("select value from settings where household_id = $1 and key = $2")
            .bind(household_id)
            .bind(KEY)
            .fetch_optional(pool)
            .await?;
    match row {
        None => Ok(ServerSettings::default()),
        Some((v,)) => serde_json::from_value(v)
            .map_err(|e| anyhow::anyhow!("stored server settings are invalid: {e}").into()),
    }
}

fn validate(s: &ServerSettings) -> Result<()> {
    if let Some(u) = &s.app_url {
        if !u.starts_with("http://") && !u.starts_with("https://") {
            return Err(invalid("Public URL must start with http:// or https://"));
        }
        let rest = &u[u.find("://").map(|i| i + 3).unwrap_or(0)..];
        if rest.is_empty() || rest.starts_with('/') {
            return Err(invalid("Public URL needs a host name"));
        }
    }
    for (label, v) in [
        ("Evaluate interval", s.evaluate_minutes),
        ("Plaid sync interval", s.plaid_sync_minutes),
        ("Privacy.com sync interval", s.privacy_sync_minutes),
    ] {
        if let Some(m) = v
            && !(1..=24 * 60).contains(&m)
        {
            return Err(invalid(format!(
                "{label} must be between 1 and 1440 minutes"
            )));
        }
    }
    for h in &s.mcp_extra_hosts {
        if h.contains('/') || h.contains(' ') {
            return Err(invalid(format!("{h:?} is not a host name")));
        }
    }
    Ok(())
}

/// Validate and store. The caller rebuilds the runtime afterwards.
pub async fn save(ctx: &Ctx, actor: &Actor, mut s: ServerSettings) -> Result<ServerSettings> {
    if let Some(u) = &mut s.app_url {
        *u = u.trim().trim_end_matches('/').to_string();
        if u.is_empty() {
            s.app_url = None;
        }
    }
    s.mcp_extra_hosts = s
        .mcp_extra_hosts
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect();
    validate(&s)?;
    let before = load(ctx).await?;
    let value = serde_json::to_value(&s).map_err(anyhow::Error::from)?;
    sqlx::query(
        "insert into settings (household_id, key, value) values ($1, $2, $3)
         on conflict (household_id, key) do update set value = excluded.value, updated_at = now()",
    )
    .bind(ctx.household_id)
    .bind(KEY)
    .bind(&value)
    .execute(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "settings.set",
        "settings",
        KEY,
        Some(&serde_json::to_value(&before).map_err(anyhow::Error::from)?),
        Some(&value),
    )
    .await?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_check_accepts_bare_and_authority() {
        let r = Runtime {
            app_url: "https://money.example.net".into(),
            trust_proxy: true,
            mcp_allowed_hosts: vec!["money.example.net".into(), "localhost:3000".into()],
            evaluate_minutes: 15,
            plaid_sync_minutes: 60,
            privacy_sync_minutes: 10,
        };
        assert!(r.host_allowed("money.example.net"));
        assert!(r.host_allowed("MONEY.example.net:443"));
        assert!(r.host_allowed("localhost:3000"));
        assert!(!r.host_allowed("localhost:8080"));
        assert!(!r.host_allowed("evil.example"));
        assert!(!r.host_allowed(""));
    }

    #[test]
    fn validation() {
        let mut s = ServerSettings {
            app_url: Some("ftp://x".into()),
            ..Default::default()
        };
        assert!(validate(&s).is_err());
        s.app_url = Some("https://hearth.example".into());
        assert!(validate(&s).is_ok());
        s.evaluate_minutes = Some(0);
        assert!(validate(&s).is_err());
    }
}
