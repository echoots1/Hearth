use chrono::{DateTime, Utc};
use hearth_core::money::Cents;
use serde::Deserialize;
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Account, AccountSource, AccountType, BalanceSnapshot};

pub async fn list(ctx: &Ctx, include_inactive: bool) -> Result<Vec<Account>> {
    Ok(sqlx::query_as::<_, Account>(
        "select * from accounts where household_id = $1 and ($2 or is_active) order by name",
    )
    .bind(ctx.household_id)
    .bind(include_inactive)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<Account> {
    sqlx::query_as::<_, Account>("select * from accounts where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAccount {
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    #[serde(default)]
    pub source: Option<AccountSource>,
    pub external_id: Option<String>,
    pub plaid_item_id: Option<Uuid>,
    pub institution_label: Option<String>,
    pub mask: Option<String>,
    pub include_in_running_balance: Option<bool>,
}

fn default_include(t: AccountType) -> bool {
    matches!(
        t,
        AccountType::Checking | AccountType::Savings | AccountType::Cash
    )
}

pub async fn create(ctx: &Ctx, actor: &Actor, c: CreateAccount) -> Result<Account> {
    if c.name.trim().is_empty() {
        return Err(invalid("name is required"));
    }
    let a = sqlx::query_as::<_, Account>(
        "insert into accounts (household_id, name, type, source, external_id, plaid_item_id, institution_label, mask, include_in_running_balance)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9) returning *",
    )
    .bind(ctx.household_id)
    .bind(c.name.trim())
    .bind(c.account_type)
    .bind(c.source.unwrap_or(AccountSource::Manual))
    .bind(&c.external_id)
    .bind(c.plaid_item_id)
    .bind(&c.institution_label)
    .bind(&c.mask)
    .bind(c.include_in_running_balance.unwrap_or_else(|| default_include(c.account_type)))
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "account.create",
        "accounts",
        a.id,
        None::<&()>,
        Some(&a),
    )
    .await?;
    Ok(a)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateAccount {
    pub name: Option<String>,
    pub include_in_running_balance: Option<bool>,
    pub is_active: Option<bool>,
    pub institution_label: Option<String>,
}

pub async fn update(ctx: &Ctx, actor: &Actor, id: Uuid, u: UpdateAccount) -> Result<Account> {
    let before = get(ctx, id).await?;
    let after = sqlx::query_as::<_, Account>(
        "update accounts set name = coalesce($3, name), include_in_running_balance = coalesce($4, include_in_running_balance),
         is_active = coalesce($5, is_active), institution_label = coalesce($6, institution_label), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.name.as_deref().map(str::trim))
    .bind(u.include_in_running_balance)
    .bind(u.is_active)
    .bind(&u.institution_label)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "account.update",
        "accounts",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

/// Record a user-reported balance. `as_of` defaults to now.
pub async fn set_reported_balance(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    balance: Cents,
    as_of: Option<DateTime<Utc>>,
) -> Result<Account> {
    let before = get(ctx, id).await?;
    let as_of = as_of.unwrap_or(ctx.now);
    if as_of > ctx.now + chrono::Duration::minutes(5) {
        return Err(invalid("as_of cannot be in the future"));
    }
    let mut tx = ctx.pool.begin().await?;
    let after = sqlx::query_as::<_, Account>(
        "update accounts set reported_balance_cents = $3, reported_balance_as_of = $4, updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(balance)
    .bind(as_of)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "insert into balance_snapshots (account_id, kind, balance_cents, as_of, actor_kind, actor_id) values ($1, 'reported', $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(balance)
    .bind(as_of)
    .bind(actor.kind())
    .bind(actor.id())
    .execute(&mut *tx)
    .await?;
    super::audit::record(
        &mut *tx,
        ctx.household_id,
        actor,
        "account.set_reported_balance",
        "accounts",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}

/// Record a bank-reported balance (from a sync). No audit row: it's data, not a decision.
pub async fn set_bank_balance(
    ctx: &Ctx,
    id: Uuid,
    balance: Cents,
    as_of: DateTime<Utc>,
) -> Result<()> {
    let mut tx = ctx.pool.begin().await?;
    sqlx::query(
        "update accounts set bank_balance_cents = $3, bank_balance_as_of = $4, updated_at = now() where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(balance)
    .bind(as_of)
    .execute(&mut *tx)
    .await?;
    sqlx::query("insert into balance_snapshots (account_id, kind, balance_cents, as_of, actor_kind) values ($1, 'bank', $2, $3, 'system')")
        .bind(id)
        .bind(balance)
        .bind(as_of)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn snapshots(ctx: &Ctx, id: Uuid, limit: Option<i64>) -> Result<Vec<BalanceSnapshot>> {
    get(ctx, id).await?;
    Ok(sqlx::query_as::<_, BalanceSnapshot>(
        "select * from balance_snapshots where account_id = $1 order by as_of desc limit $2",
    )
    .bind(id)
    .bind(super::clamp_limit(limit))
    .fetch_all(&ctx.pool)
    .await?)
}
