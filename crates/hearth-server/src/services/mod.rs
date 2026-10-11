//! Domain services: the only layer that talks to the database.
//!
//! Handlers (HTTP), MCP tools, and jobs all call these. Every mutating call
//! takes an [`Actor`] and writes an audit row.

pub mod accounts;
pub mod alerts;
pub mod assistant;
pub mod audit;
pub mod balance;
pub mod bills;
pub mod budget;
pub mod csv_import;
pub mod debts;
pub mod goals;
pub mod household;
pub mod notifications;
pub mod plaid;
pub mod planned;
pub mod privacy;
pub mod push;
pub mod questions;
pub mod rules;
pub mod secrets;
pub mod settings;
pub mod sinking;
pub mod transactions;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use hearth_core::calendar::{Month, today_in};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::models::ActorKind;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("forbidden")]
    Forbidden,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

/// Who is performing a write, for the audit log.
#[derive(Clone, Debug)]
pub enum Actor {
    User {
        id: Uuid,
        name: String,
    },
    /// The assistant acting through an MCP API key (or API mode).
    Assistant {
        key_id: Option<Uuid>,
        name: String,
    },
    Job(String),
    System,
}

impl Actor {
    pub fn kind(&self) -> ActorKind {
        match self {
            Actor::User { .. } => ActorKind::User,
            Actor::Assistant { .. } => ActorKind::Assistant,
            Actor::Job(_) => ActorKind::Job,
            Actor::System => ActorKind::System,
        }
    }

    pub fn id(&self) -> Option<String> {
        match self {
            Actor::User { id, .. } => Some(id.to_string()),
            Actor::Assistant { key_id, .. } => key_id.map(|k| k.to_string()),
            Actor::Job(name) => Some(name.clone()),
            Actor::System => None,
        }
    }

    /// Display name recorded on acks/answers/decisions.
    pub fn is_assistant(&self) -> bool {
        matches!(self, Actor::Assistant { .. })
    }

    pub fn display(&self) -> String {
        match self {
            Actor::User { name, .. } => name.clone(),
            Actor::Assistant { .. } => "Claude".to_string(),
            Actor::Job(name) => format!("job:{name}"),
            Actor::System => "system".into(),
        }
    }
}

/// Everything a service call needs: the pool, the household, and "now".
#[derive(Clone)]
pub struct Ctx {
    pub pool: PgPool,
    pub household_id: Uuid,
    pub tz: Tz,
    pub now: DateTime<Utc>,
    /// Present when `APP_ENCRYPTION_KEY` is configured.
    pub crypto: Option<secrets::Crypto>,
}

impl Ctx {
    pub fn today(&self) -> NaiveDate {
        today_in(self.tz, self.now)
    }

    pub fn month(&self) -> Month {
        Month::of(self.today())
    }
}

/// Cursor pagination helper: `limit` clamped to 1..=500.
pub fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(100).clamp(1, 500)
}

pub fn parse_month(s: &str) -> Result<Month> {
    s.parse()
        .map_err(|e: hearth_core::calendar::MonthParseError| invalid(e.to_string()))
}
