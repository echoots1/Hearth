//! Sinking fund: the money set aside for non-monthly recurring bills.

use chrono::{DateTime, Utc};
use hearth_core::calendar::Month;
use hearth_core::money::Cents;
use hearth_core::sinking::{self, SinkingFundView};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::SinkingFund;

pub async fn list_funds(ctx: &Ctx) -> Result<Vec<SinkingFund>> {
    Ok(sqlx::query_as::<_, SinkingFund>(
        "select * from sinking_funds where household_id = $1 order by created_at",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get_fund(ctx: &Ctx, id: Uuid) -> Result<SinkingFund> {
    sqlx::query_as::<_, SinkingFund>(
        "select * from sinking_funds where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)
}

/// The household's fund, created on first use.
pub async fn ensure_fund(ctx: &Ctx) -> Result<SinkingFund> {
    if let Some(f) = list_funds(ctx).await?.into_iter().next() {
        return Ok(f);
    }
    Ok(sqlx::query_as::<_, SinkingFund>(
        "insert into sinking_funds (household_id, name) values ($1, 'Sinking fund') returning *",
    )
    .bind(ctx.household_id)
    .fetch_one(&ctx.pool)
    .await?)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateFund {
    pub name: String,
    pub account_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub funded_balance_cents: Option<Cents>,
}

pub async fn create_fund(ctx: &Ctx, actor: &Actor, c: CreateFund) -> Result<SinkingFund> {
    if c.name.trim().is_empty() {
        return Err(invalid("name is required"));
    }
    let f = sqlx::query_as::<_, SinkingFund>(
        "insert into sinking_funds (household_id, name, account_id, category_id, funded_balance_cents, balance_as_of)
         values ($1, $2, $3, $4, coalesce($5, 0), case when $5 is null then null else now() end) returning *",
    )
    .bind(ctx.household_id)
    .bind(c.name.trim())
    .bind(c.account_id)
    .bind(c.category_id)
    .bind(c.funded_balance_cents)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "sinking.create_fund",
        "sinking_funds",
        f.id,
        None::<&()>,
        Some(&f),
    )
    .await?;
    Ok(f)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateFund {
    pub name: Option<String>,
    pub account_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
}

pub async fn update_fund(ctx: &Ctx, actor: &Actor, id: Uuid, u: UpdateFund) -> Result<SinkingFund> {
    let before = get_fund(ctx, id).await?;
    let after = sqlx::query_as::<_, SinkingFund>(
        "update sinking_funds set name = coalesce($3, name), account_id = coalesce($4, account_id), category_id = coalesce($5, category_id), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.name.as_deref().map(str::trim))
    .bind(u.account_id)
    .bind(u.category_id)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "sinking.update_fund",
        "sinking_funds",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn set_balance(
    ctx: &Ctx,
    actor: &Actor,
    fund_id: Uuid,
    balance: Cents,
    as_of: Option<DateTime<Utc>>,
) -> Result<SinkingFund> {
    if balance.0 < 0 {
        return Err(invalid("balance must be ≥ 0"));
    }
    let before = get_fund(ctx, fund_id).await?;
    let after = sqlx::query_as::<_, SinkingFund>(
        "update sinking_funds set funded_balance_cents = $3, balance_as_of = $4, updated_at = now() where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(fund_id)
    .bind(balance)
    .bind(as_of.unwrap_or(ctx.now))
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "sinking.set_balance",
        "sinking_funds",
        fund_id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

#[derive(Debug, Clone, Serialize)]
pub struct FundWithView {
    #[serde(flatten)]
    pub fund: SinkingFund,
    pub view: SinkingFundView,
}

/// The fund's math over every active non-monthly bill.
pub async fn overview(ctx: &Ctx, month: Option<Month>) -> Result<FundWithView> {
    let fund = ensure_fund(ctx).await?;
    let bills = super::bills::all(ctx).await?;
    let items = super::bills::sinking_items(&bills);
    let view = sinking::fund_view(
        &items,
        fund.funded_balance_cents,
        month.unwrap_or_else(|| ctx.month()),
    );
    Ok(FundWithView { fund, view })
}
