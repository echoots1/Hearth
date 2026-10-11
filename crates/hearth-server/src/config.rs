//! Typed configuration parsed once from the environment.
//!
//! Every variable here is documented in `.env.example`. Integration sections
//! are `Option`s: absent env → integration disabled.

use std::env;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct Config {
    /// From `APP_URL`, or the default. The app's own setting may override it
    /// at runtime (see `services::server`).
    pub app_url: String,
    pub port: u16,
    pub trust_proxy: bool,
    pub database: DatabaseConfig,
    pub app_secret: String,
    pub app_secret_source: SecretSource,
    pub app_encryption_key: Option<String>,
    pub app_encryption_key_source: SecretSource,
    /// Where generated secrets live (`HEARTH_DATA_DIR`).
    pub data_dir: std::path::PathBuf,
    /// `MCP_ALLOWED_HOSTS` as given; `None` means derive from the public URL.
    pub mcp_allowed_hosts: Option<Vec<String>>,
    pub jobs: JobsConfig,
    pub plaid: Option<PlaidConfig>,
    pub privacy: Option<PrivacyConfig>,
    pub ntfy: Option<NtfyConfig>,
    pub smtp: Option<SmtpConfig>,
}

/// Where a secret came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSource {
    /// Set in the environment.
    Env,
    /// Generated on first start and kept in the data directory.
    Generated,
}

/// `name` from the environment, else a random value kept in `<data_dir>/<file>`
/// (created on first use, mode 0600). Lets a deploy supply nothing but the
/// database and still have stable session and encryption keys.
fn secret_from_env_or_file(
    name: &str,
    file: &str,
    data_dir: &std::path::Path,
) -> anyhow::Result<(String, SecretSource)> {
    if let Some(v) = var(name) {
        if v.len() < 32 {
            bail!("{name} must be at least 32 characters");
        }
        return Ok((v, SecretSource::Env));
    }
    let path = data_dir.join(file);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim().to_string();
        if existing.len() >= 32 {
            return Ok((existing, SecretSource::Generated));
        }
        bail!(
            "{} exists but is too short to be a {name}; delete it to generate a new one",
            path.display()
        );
    }
    std::fs::create_dir_all(data_dir).with_context(|| {
        format!(
            "{name} is not set and the data directory {} cannot be created; set {name} or mount a writable volume there (HEARTH_DATA_DIR)",
            data_dir.display()
        )
    })?;
    let bytes: [u8; 48] = rand::random();
    let value = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&path).with_context(|| {
            format!(
                "{name} is not set and {} cannot be written; set {name} or mount a writable volume there (HEARTH_DATA_DIR)",
                path.display()
            )
        })?;
        f.write_all(value.as_bytes())?;
    }
    tracing::info!(file = %path.display(), "generated {name}; back up the data directory with the database");
    Ok((value, SecretSource::Generated))
}

/// Where Postgres is. Either a full `DATABASE_URL`, or the pieces, so that a
/// password never has to be URL-escaped.
#[derive(Clone, Debug)]
pub enum DatabaseConfig {
    Url(String),
    Parts {
        host: String,
        port: u16,
        user: String,
        password: String,
        database: String,
    },
}

impl DatabaseConfig {
    pub fn connect_options(&self) -> anyhow::Result<sqlx::postgres::PgConnectOptions> {
        use sqlx::postgres::PgConnectOptions;
        match self {
            DatabaseConfig::Url(url) => url.parse::<PgConnectOptions>().map_err(|e| {
                anyhow::anyhow!(
                    "DATABASE_URL is not a valid postgres:// URL ({e}). A password with \
                     characters like @ : / # or % must be percent-encoded there; or leave \
                     DATABASE_URL unset and use POSTGRES_HOST / POSTGRES_USER / \
                     POSTGRES_PASSWORD / POSTGRES_DB instead."
                )
            }),
            DatabaseConfig::Parts {
                host,
                port,
                user,
                password,
                database,
            } => Ok(PgConnectOptions::new()
                .host(host)
                .port(*port)
                .username(user)
                .password(password)
                .database(database)),
        }
    }

    /// For logs: never includes the password.
    pub fn describe(&self) -> String {
        match self {
            DatabaseConfig::Url(url) => {
                let tail = url.rsplit_once('@').map(|(_, t)| t).unwrap_or("…");
                format!("DATABASE_URL (…@{tail})")
            }
            DatabaseConfig::Parts {
                host,
                port,
                user,
                database,
                ..
            } => format!("{user}@{host}:{port}/{database}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct JobsConfig {
    pub enabled: bool,
    pub tick_seconds: u64,
    pub plaid_sync_minutes: u64,
    pub privacy_sync_minutes: u64,
    pub evaluate_minutes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaidConfig {
    pub client_id: String,
    pub secret: String,
    pub env: String,
    #[serde(default = "default_plaid_products")]
    pub products: Vec<String>,
    #[serde(default = "default_country_codes")]
    pub country_codes: Vec<String>,
}

fn default_plaid_products() -> Vec<String> {
    vec!["transactions".into()]
}
fn default_country_codes() -> Vec<String> {
    vec!["US".into()]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub api_key: String,
    #[serde(default = "default_sandbox")]
    pub env: String,
}

fn default_sandbox() -> String {
    "sandbox".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NtfyConfig {
    #[serde(default = "default_ntfy_url")]
    pub url: String,
    pub topic: String,
    #[serde(default)]
    pub token: Option<String>,
}

fn default_ntfy_url() -> String {
    "https://ntfy.sh".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub pass: Option<String>,
    pub from: String,
    pub alert_to: String,
}

fn default_smtp_port() -> u16 {
    587
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
        let data_dir =
            std::path::PathBuf::from(var("HEARTH_DATA_DIR").unwrap_or_else(|| "data".into()));
        let (app_secret, app_secret_source) =
            secret_from_env_or_file("APP_SECRET", "app_secret", &data_dir)?;
        let (key, app_encryption_key_source) =
            secret_from_env_or_file("APP_ENCRYPTION_KEY", "app_encryption_key", &data_dir)?;
        let app_encryption_key = Some(key);

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

        let database = match var("DATABASE_URL") {
            Some(url) => DatabaseConfig::Url(url),
            None => DatabaseConfig::Parts {
                host: var("POSTGRES_HOST").unwrap_or_else(|| "db".into()),
                port: parse("POSTGRES_PORT", 5432)?,
                user: var("POSTGRES_USER").unwrap_or_else(|| "hearth".into()),
                password: var("POSTGRES_PASSWORD").with_context(|| {
                    "POSTGRES_PASSWORD is required (or set DATABASE_URL for an external database)"
                })?,
                database: var("POSTGRES_DB").unwrap_or_else(|| "hearth".into()),
            },
        };

        let app_url = var("APP_URL")
            .unwrap_or_else(|| "http://localhost:3000".into())
            .trim_end_matches('/')
            .to_string();
        let port: u16 = parse("PORT", 3000)?;
        let mcp_allowed_hosts = var("MCP_ALLOWED_HOSTS").map(|_| list("MCP_ALLOWED_HOSTS", &[]));

        Ok(Config {
            app_url,
            port,
            trust_proxy: flag("TRUST_PROXY", false)?,
            database,
            app_secret,
            app_secret_source,
            app_encryption_key,
            app_encryption_key_source,
            data_dir,
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
}

/// The host (with and without its port) from the public URL, plus loopback on
/// the listening port, so local and proxied access both pass the `Host` check.
pub fn default_mcp_hosts(app_url: &str, port: u16) -> Vec<String> {
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
    use super::{DatabaseConfig, default_mcp_hosts};

    #[test]
    fn database_parts_take_any_password() {
        let db = DatabaseConfig::Parts {
            host: "db".into(),
            port: 5432,
            user: "hearth".into(),
            password: "p@ss:w/rd#100%".into(),
            database: "hearth".into(),
        };
        assert!(db.connect_options().is_ok());
        assert_eq!(db.describe(), "hearth@db:5432/hearth");
    }

    #[test]
    fn database_url_with_raw_password_explains_itself() {
        // `#` starts a URL fragment, so the host becomes `hearth` and the port `pa`.
        let db = DatabaseConfig::Url("postgres://hearth:pa#ss@db:5432/hearth".into());
        let err = db.connect_options().unwrap_err().to_string();
        assert!(err.contains("percent-encoded"), "{err}");
        assert!(!db.describe().contains("pa#ss"));
        let ok = DatabaseConfig::Url("postgres://hearth:pa%23ss@db:5432/hearth".into());
        assert!(ok.connect_options().is_ok());
    }

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
