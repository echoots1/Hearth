//! Typed configuration parsed once from the environment.
//!
//! Every variable here is documented in `.env.example`. Integration sections
//! are `Option`s: absent env → integration disabled.

use std::env;

use anyhow::{Context, bail};

#[derive(Clone, Debug)]
pub struct Config {
    pub app_url: String,
    pub port: u16,
    pub trust_proxy: bool,
    pub database_url: String,
    pub app_secret: String,
    pub app_encryption_key: Option<String>,
    /// `Host` values the MCP endpoint accepts (DNS-rebinding guard).
    pub mcp_allowed_hosts: Vec<String>,
    pub jobs: JobsConfig,
    pub plaid: Option<PlaidConfig>,
    pub privacy: Option<PrivacyConfig>,
    pub ntfy: Option<NtfyConfig>,
    pub smtp: Option<SmtpConfig>,
}

#[derive(Clone, Debug)]
pub struct JobsConfig {
    pub enabled: bool,
    pub tick_seconds: u64,
    pub plaid_sync_minutes: u64,
    pub privacy_sync_minutes: u64,
    pub evaluate_minutes: u64,
}

#[derive(Clone, Debug)]
pub struct PlaidConfig {
    pub client_id: String,
    pub secret: String,
    pub env: String,
    pub products: Vec<String>,
    pub country_codes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PrivacyConfig {
    pub api_key: String,
    pub env: String,
}

#[derive(Clone, Debug)]
pub struct NtfyConfig {
    pub url: String,
    pub topic: String,
    pub token: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub pass: Option<String>,
    pub from: String,
    pub alert_to: String,
}

fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn required(name: &str) -> anyhow::Result<String> {
    var(name).with_context(|| format!("{name} is required"))
}

fn parse<T: std::str::FromStr>(name: &str, default: T) -> anyhow::Result<T>
where
    T::Err: std::fmt::Display,
{
    match var(name) {
        None => Ok(default),
        Some(v) => v
            .parse()
            .map_err(|e| anyhow::anyhow!("{name}={v:?} is invalid: {e}")),
    }
}

fn flag(name: &str, default: bool) -> anyhow::Result<bool> {
    match var(name).map(|v| v.to_ascii_lowercase()) {
        None => Ok(default),
        Some(v) if ["1", "true", "yes", "on"].contains(&v.as_str()) => Ok(true),
        Some(v) if ["0", "false", "no", "off"].contains(&v.as_str()) => Ok(false),
        Some(v) => bail!("{name}={v:?} is not a boolean"),
    }
}

fn list(name: &str, default: &[&str]) -> Vec<String> {
    match var(name) {
        None => default.iter().map(|s| s.to_string()).collect(),
        Some(v) => v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Config> {
        let app_secret = required("APP_SECRET")?;
        if app_secret.len() < 32 {
            bail!("APP_SECRET must be at least 32 characters");
        }
        let app_encryption_key = var("APP_ENCRYPTION_KEY");
        if let Some(k) = &app_encryption_key
            && k.len() < 32
        {
            bail!("APP_ENCRYPTION_KEY must be at least 32 characters");
        }

        let plaid = match (var("PLAID_CLIENT_ID"), var("PLAID_SECRET")) {
            (Some(client_id), Some(secret)) => {
                if app_encryption_key.is_none() {
                    bail!("APP_ENCRYPTION_KEY is required when Plaid is configured");
                }
                Some(PlaidConfig {
                    client_id,
                    secret,
                    env: var("PLAID_ENV").unwrap_or_else(|| "sandbox".into()),
                    products: list("PLAID_PRODUCTS", &["transactions"]),
                    country_codes: list("PLAID_COUNTRY_CODES", &["US"]),
                })
            }
            (None, None) => None,
            _ => bail!("PLAID_CLIENT_ID and PLAID_SECRET must be set together"),
        };

        let privacy = var("PRIVACY_API_KEY").map(|api_key| PrivacyConfig {
            api_key,
            env: var("PRIVACY_ENV").unwrap_or_else(|| "sandbox".into()),
        });

        let ntfy = var("NTFY_TOPIC").map(|topic| NtfyConfig {
            url: var("NTFY_URL").unwrap_or_else(|| "https://ntfy.sh".into()),
            topic,
            token: var("NTFY_TOKEN"),
        });

        let smtp = match var("SMTP_HOST") {
            Some(host) => Some(SmtpConfig {
                host,
                port: parse("SMTP_PORT", 587)?,
                user: var("SMTP_USER"),
                pass: var("SMTP_PASS"),
                from: required("SMTP_FROM")?,
                alert_to: required("ALERT_EMAIL_TO")?,
            }),
            None => None,
        };

        let app_url = var("APP_URL")
            .unwrap_or_else(|| "http://localhost:3000".into())
            .trim_end_matches('/')
            .to_string();
        let port: u16 = parse("PORT", 3000)?;
        let mcp_allowed_hosts = match var("MCP_ALLOWED_HOSTS") {
            Some(_) => list("MCP_ALLOWED_HOSTS", &[]),
            None => default_mcp_hosts(&app_url, port),
        };

        Ok(Config {
            app_url,
            port,
            trust_proxy: flag("TRUST_PROXY", false)?,
            database_url: required("DATABASE_URL")?,
            app_secret,
            app_encryption_key,
            mcp_allowed_hosts,
            jobs: JobsConfig {
                enabled: flag("JOBS_ENABLED", true)?,
                tick_seconds: parse("SCHEDULER_TICK_SECONDS", 30)?,
                plaid_sync_minutes: parse("PLAID_SYNC_MINUTES", 60)?,
                privacy_sync_minutes: parse("PRIVACY_SYNC_MINUTES", 10)?,
                evaluate_minutes: parse("EVALUATE_MINUTES", 15)?,
            },
            plaid,
            privacy,
            ntfy,
            smtp,
        })
    }

    pub fn is_https(&self) -> bool {
        self.app_url.starts_with("https://")
    }
}

/// The host (with and without its port) from `APP_URL`, plus loopback on the
/// listening port, so local and proxied access both pass the `Host` check.
fn default_mcp_hosts(app_url: &str, port: u16) -> Vec<String> {
    let mut hosts = vec![
        "localhost".to_string(),
        format!("localhost:{port}"),
        "127.0.0.1".to_string(),
        format!("127.0.0.1:{port}"),
        "[::1]".to_string(),
        format!("[::1]:{port}"),
    ];
    if let Some(authority) = app_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split('/').next())
        .filter(|a| !a.is_empty())
    {
        let authority = authority.to_ascii_lowercase();
        let bare = match authority.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.is_empty() => h.to_string(),
            _ => authority.clone(),
        };
        for h in [bare, authority] {
            if !hosts.contains(&h) {
                hosts.push(h);
            }
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::default_mcp_hosts;

    #[test]
    fn mcp_hosts_from_app_url() {
        let h = default_mcp_hosts("https://money.example.net", 3000);
        assert!(h.contains(&"money.example.net".to_string()));
        assert!(h.contains(&"localhost:3000".to_string()));
        let h = default_mcp_hosts("http://home-server:8080", 3000);
        assert!(h.contains(&"home-server".to_string()));
        assert!(h.contains(&"home-server:8080".to_string()));
    }
}
