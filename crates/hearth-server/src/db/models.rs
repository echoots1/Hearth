//! Row types and enums mirroring `docs/DATA-MODEL.md`.
//!
//! Each Postgres enum maps to a Rust enum with `#[sqlx(type_name = ...)]`;
//! each table has a `FromRow` struct. Money columns are `Cents` (bigint).

#![allow(dead_code)] // rows are consumed as services land

use chrono::{DateTime, NaiveDate, Utc};
use hearth_core::money::Cents;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

macro_rules! pg_enum {
    ($(#[$meta:meta])* $name:ident, $type_name:literal, { $($variant:ident => $lit:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
        #[sqlx(type_name = $type_name, rename_all = "snake_case")]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $lit),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s { $($lit => Ok($name::$variant),)+ _ => Err(format!("unknown {}: {s:?}", $type_name)) }
            }
        }
    };
}

pg_enum!(UserRole, "user_role", { Owner => "owner", Member => "member" });
pg_enum!(ApiKeyScope, "api_key_scope", { Read => "read", ReadWrite => "read_write" });
pg_enum!(AccountType, "account_type", {
    Checking => "checking", Savings => "savings", CreditCard => "credit_card",
    VirtualCard => "virtual_card", Cash => "cash", Loan => "loan", Other => "other",
});
pg_enum!(AccountSource, "account_source", { Plaid => "plaid", Privacy => "privacy", Manual => "manual" });
pg_enum!(TransactionSource, "transaction_source", { Plaid => "plaid", Privacy => "privacy", Csv => "csv", Manual => "manual" });
pg_enum!(ReviewStatus, "review_status", { None => "none", Pending => "pending", Resolved => "resolved" });
pg_enum!(CategorySign, "category_sign", { Income => "income", Spend => "spend" });
pg_enum!(RuleMatchType, "rule_match_type", { Contains => "contains", Regex => "regex" });
pg_enum!(RuleAction, "rule_action", { Categorize => "categorize", FlagCanceled => "flag_canceled", Ignore => "ignore" });
pg_enum!(PlannedStatus, "planned_status", { Planned => "planned", Done => "done", Dropped => "dropped" });
pg_enum!(DebtKind, "debt_kind", { CreditCard => "credit_card", Loan => "loan", Mortgage => "mortgage", Other => "other" });
pg_enum!(AlertType, "alert_type", {
    UnplannedLargeCharge => "unplanned_large_charge", CanceledServiceCharged => "canceled_service_charged",
    CategoryBlown => "category_blown", CategoryPace => "category_pace", FloorBreached => "floor_breached",
    DebtRedraw => "debt_redraw", CardDeclined => "card_declined", PlannedItemMissing => "planned_item_missing",
    BalanceStale => "balance_stale", SyncFailed => "sync_failed", ReviewQueueBacklog => "review_queue_backlog",
});
pg_enum!(AlertSeverity, "alert_severity", { Info => "info", Warn => "warn", Critical => "critical" });
pg_enum!(AlertStatus, "alert_status", { Open => "open", Notified => "notified", Ack => "ack", Done => "done" });
pg_enum!(NotificationChannel, "notification_channel", { Ntfy => "ntfy", Smtp => "smtp" });
pg_enum!(NotificationStatus, "notification_status", { Sent => "sent", Failed => "failed" });
pg_enum!(QuestionStatus, "question_status", { Open => "open", Answered => "answered", Dismissed => "dismissed" });
pg_enum!(CardEventResult, "card_event_result", { Declined => "declined", Approved => "approved" });
pg_enum!(JobRunStatus, "job_run_status", { Running => "running", Ok => "ok", Failed => "failed", Skipped => "skipped" });
pg_enum!(ActorKind, "actor_kind", { User => "user", ApiKey => "api_key", Job => "job", System => "system" });

impl From<CategorySign> for hearth_core::budget::Sign {
    fn from(s: CategorySign) -> Self {
        match s {
            CategorySign::Income => hearth_core::budget::Sign::Income,
            CategorySign::Spend => hearth_core::budget::Sign::Spend,
        }
    }
}

impl From<hearth_core::alerts::AlertType> for AlertType {
    fn from(t: hearth_core::alerts::AlertType) -> Self {
        use hearth_core::alerts::AlertType as C;
        match t {
            C::UnplannedLargeCharge => AlertType::UnplannedLargeCharge,
            C::CanceledServiceCharged => AlertType::CanceledServiceCharged,
            C::CategoryBlown => AlertType::CategoryBlown,
            C::CategoryPace => AlertType::CategoryPace,
            C::FloorBreached => AlertType::FloorBreached,
            C::DebtRedraw => AlertType::DebtRedraw,
            C::CardDeclined => AlertType::CardDeclined,
            C::PlannedItemMissing => AlertType::PlannedItemMissing,
            C::BalanceStale => AlertType::BalanceStale,
            C::SyncFailed => AlertType::SyncFailed,
            C::ReviewQueueBacklog => AlertType::ReviewQueueBacklog,
        }
    }
}

impl From<hearth_core::alerts::Severity> for AlertSeverity {
    fn from(s: hearth_core::alerts::Severity) -> Self {
        use hearth_core::alerts::Severity as C;
        match s {
            C::Info => AlertSeverity::Info,
            C::Warn => AlertSeverity::Warn,
            C::Critical => AlertSeverity::Critical,
        }
    }
}

impl From<RuleMatchType> for hearth_core::rules::MatchType {
    fn from(m: RuleMatchType) -> Self {
        match m {
            RuleMatchType::Contains => hearth_core::rules::MatchType::Contains,
            RuleMatchType::Regex => hearth_core::rules::MatchType::Regex,
        }
    }
}

impl From<RuleAction> for hearth_core::rules::RuleAction {
    fn from(a: RuleAction) -> Self {
        match a {
            RuleAction::Categorize => hearth_core::rules::RuleAction::Categorize,
            RuleAction::FlagCanceled => hearth_core::rules::RuleAction::FlagCanceled,
            RuleAction::Ignore => hearth_core::rules::RuleAction::Ignore,
        }
    }
}

// ───────────────────────────── rows ─────────────────────────────

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Household {
    pub id: Uuid,
    pub name: String,
    pub timezone: String,
    pub currency: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
pub struct User {
    pub id: Uuid,
    pub household_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
pub struct Session {
    pub id: String,
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub household_id: Uuid,
    pub name: String,
    pub key_prefix: String,
    #[serde(skip)]
    pub key_hash: String,
    pub scope: ApiKeyScope,
    pub created_by: Option<Uuid>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
pub struct Setting {
    pub household_id: Uuid,
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct PlaidItem {
    pub id: Uuid,
    pub household_id: Uuid,
    pub item_id: String,
    #[serde(skip)]
    pub access_token_enc: String,
    pub institution_id: Option<String>,
    pub institution_name: Option<String>,
    #[serde(skip)]
    pub sync_cursor: Option<String>,
    pub status: String,
    pub last_error: Option<String>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub consecutive_failures: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Account {
    pub id: Uuid,
    pub household_id: Uuid,
    pub name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub source: AccountSource,
    pub external_id: Option<String>,
    pub plaid_item_id: Option<Uuid>,
    pub institution_label: Option<String>,
    pub mask: Option<String>,
    pub currency: String,
    pub is_active: bool,
    pub include_in_running_balance: bool,
    pub reported_balance_cents: Option<Cents>,
    pub reported_balance_as_of: Option<DateTime<Utc>>,
    pub bank_balance_cents: Option<Cents>,
    pub bank_balance_as_of: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct CategoryGroup {
    pub id: Uuid,
    pub household_id: Uuid,
    pub name: String,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Category {
    pub id: Uuid,
    pub household_id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    pub sign: CategorySign,
    pub note: String,
    pub is_everyday: bool,
    pub due_day: Option<i16>,
    pub sort_order: i32,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct CategoryPlan {
    pub id: Uuid,
    pub category_id: Uuid,
    pub starts_month: String,
    pub amount_cents: Cents,
    pub label: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Rule {
    pub id: Uuid,
    pub household_id: Uuid,
    pub name: String,
    pub priority: i32,
    pub match_type: RuleMatchType,
    pub pattern: String,
    pub account_id: Option<Uuid>,
    pub amount_min_cents: Option<Cents>,
    pub amount_max_cents: Option<Cents>,
    pub action: RuleAction,
    pub category_id: Option<Uuid>,
    pub set_note: Option<String>,
    pub is_active: bool,
    pub hit_count: i32,
    pub last_hit_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct ImportBatch {
    pub id: Uuid,
    pub household_id: Uuid,
    pub account_id: Uuid,
    pub filename: String,
    pub mapping: serde_json::Value,
    pub row_count: i32,
    pub inserted_count: i32,
    pub duplicate_count: i32,
    pub created_by: Option<Uuid>,
    pub rolled_back_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct PlannedItem {
    pub id: Uuid,
    pub household_id: Uuid,
    pub date: NaiveDate,
    pub item: String,
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
    pub status: PlannedStatus,
    pub decided_by: String,
    pub note: String,
    pub matched_transaction_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Transaction {
    pub id: Uuid,
    pub household_id: Uuid,
    pub account_id: Uuid,
    pub date: NaiveDate,
    pub description: String,
    pub original_description: String,
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
    pub source: TransactionSource,
    pub external_id: Option<String>,
    #[serde(skip)]
    pub fingerprint: Option<String>,
    pub pending: bool,
    pub note: String,
    pub planned_item_id: Option<Uuid>,
    pub rule_id: Option<Uuid>,
    pub review_status: ReviewStatus,
    pub import_batch_id: Option<Uuid>,
    pub shadow_of_transaction_id: Option<Uuid>,
    #[serde(skip)]
    pub raw: Option<serde_json::Value>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct SinkingFund {
    pub id: Uuid,
    pub household_id: Uuid,
    pub name: String,
    pub account_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub funded_balance_cents: Cents,
    pub balance_as_of: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct SinkingFundItem {
    pub id: Uuid,
    pub fund_id: Uuid,
    pub name: String,
    pub amount_cents: Cents,
    pub due_month: String,
    pub cycle_months: Option<i16>,
    pub category_id: Option<Uuid>,
    pub note: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Debt {
    pub id: Uuid,
    pub household_id: Uuid,
    pub name: String,
    pub kind: DebtKind,
    pub balance_cents: Cents,
    pub balance_as_of: DateTime<Utc>,
    pub apr_bps: i32,
    pub min_payment_cents: Cents,
    pub planned_payment_cents: Option<Cents>,
    pub due_day: i16,
    pub account_id: Option<Uuid>,
    pub custom_order: Option<i32>,
    pub is_closed: bool,
    pub note: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct BalanceSnapshot {
    pub id: Uuid,
    pub account_id: Option<Uuid>,
    pub debt_id: Option<Uuid>,
    pub kind: String,
    pub balance_cents: Cents,
    pub as_of: DateTime<Utc>,
    pub actor_kind: ActorKind,
    pub actor_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Alert {
    pub id: Uuid,
    pub household_id: Uuid,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub title: String,
    pub detail: String,
    pub amount_cents: Option<Cents>,
    pub source: String,
    pub dedupe_key: String,
    pub status: AlertStatus,
    pub resolution: Option<String>,
    pub transaction_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub debt_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub planned_item_id: Option<Uuid>,
    pub context: Option<serde_json::Value>,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub notified_at: Option<DateTime<Utc>>,
    pub last_nudged_at: Option<DateTime<Utc>>,
    pub nudge_count: i32,
    pub snoozed_until: Option<DateTime<Utc>>,
    pub acked_by: Option<String>,
    pub acked_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Question {
    pub id: Uuid,
    pub household_id: Uuid,
    pub asked_by: String,
    pub question: String,
    pub options: Option<serde_json::Value>,
    pub transaction_id: Option<Uuid>,
    pub alert_id: Option<Uuid>,
    pub planned_item_id: Option<Uuid>,
    pub context: Option<serde_json::Value>,
    pub status: QuestionStatus,
    pub answer: Option<String>,
    pub answered_by: Option<String>,
    pub answered_at: Option<DateTime<Utc>>,
    pub notify: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Notification {
    pub id: Uuid,
    pub alert_id: Option<Uuid>,
    pub question_id: Option<Uuid>,
    pub channel: NotificationChannel,
    pub kind: String,
    pub status: NotificationStatus,
    pub external_id: Option<String>,
    pub error: Option<String>,
    pub sent_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct CardEvent {
    pub id: Uuid,
    pub account_id: Uuid,
    pub external_id: String,
    pub occurred_at: DateTime<Utc>,
    pub amount_cents: Cents,
    pub descriptor: String,
    pub result: CardEventResult,
    pub reason: Option<String>,
    #[serde(skip)]
    pub raw: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct JobRun {
    pub id: Uuid,
    pub job: String,
    pub status: JobRunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub summary: Option<serde_json::Value>,
    pub error: Option<String>,
    pub triggered_by: String,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub household_id: Uuid,
    pub actor_kind: ActorKind,
    pub actor_id: Option<String>,
    pub action: String,
    pub entity: String,
    pub entity_id: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub at: DateTime<Utc>,
}
