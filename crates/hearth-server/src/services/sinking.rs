use chrono::{DateTime, Datelike, Utc};
use hearth_core::calendar::Month;
use hearth_core::money::Cents;
use hearth_core::sinking::{self, SinkingFundView, SinkingItemInput};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid, parse_month};
use crate::db::models::{SinkingFund, SinkingFundItem};

pub async fn list_funds(ctx: &Ctx) -> Result<Vec<SinkingFund>> {
    Ok(sqlx::query_as::<_, SinkingFund>("select * from sinking_funds where household_id = $1 order by created_at")
        .bind(ctx.household_id)
        .fetch_all(&ctx.pool)
        .await?)
}

pub async fn get_fund(ctx: &Ctx, id: Uuid) -> Result<SinkingFund> {
    sqlx::query_as::<_, SinkingFund>("select * from sinking_funds where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

pub async fn list_items(ctx: &Ctx, fund_id: Uuid) -> Result<Vec<SinkingFundItem>> {
    get_fund(ctx, fund_id).await?;
    Ok(sqlx::query_as::<_, SinkingFundItem>("select * from sinking_fund_items where fund_id = $1 order by due_month, name")
        .bind(fund_id)
        .fetch_all(&ctx.pool)
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
         values ($1, $2, $3, $4, $5, case when $5 is null then null else now() end) returning *",
    )
    .bind(ctx.household_id)
    .bind(c.name.trim())
    .bind(c.account_id)
    .bind(c.category_id)
    .bind(c.funded_balance_cents)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "sinking.create_fund", "sinking_funds", f.id, None::<&()>, Some(&f)).await?;
    Ok(f)
}

pub async fn set_balance(ctx: &Ctx, actor: &Actor, fund_id: Uuid, balance: Cents, as_of: Option<DateTime<Utc>>) -> Result<SinkingFund> {
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
    super::audit::record(&ctx.pool, ctx.household_id, actor, "sinking.set_balance", "sinking_funds", fund_id, Some(&before), Some(&after)).await?;
    Ok(after)
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertItem {
    pub id: Option<Uuid>,
    pub name: String,
    pub amount_cents: Cents,
    /// `YYYY-MM`
    pub due_month: String,
    pub cycle_months: Option<i16>,
    pub category_id: Option<Uuid>,
    pub note: Option<String>,
}

pub async fn upsert_item(ctx: &Ctx, actor: &Actor, fund_id: Uuid, u: UpsertItem) -> Result<SinkingFundItem> {
    get_fund(ctx, fund_id).await?;
    if u.name.trim().is_empty() {
        return Err(invalid("name is required"));
    }
    if u.amount_cents.0 < 0 {
        return Err(invalid("amount_cents must be ≥ 0"));
    }
    if let Some(c) = u.cycle_months
        && c <= 0
    {
        return Err(invalid("cycle_months must be positive or null"));
    }
    let due = parse_month(&u.due_month)?;
    let before = match u.id {
        Some(id) => Some(
            sqlx::query_as::<_, SinkingFundItem>("select * from sinking_fund_items where id = $1 and fund_id = $2")
                .bind(id)
                .bind(fund_id)
                .fetch_optional(&ctx.pool)
                .await?
                .ok_or(Error::NotFound)?,
        ),
        None => None,
    };
    let after = match &before {
        Some(b) => {
            sqlx::query_as::<_, SinkingFundItem>(
                "update sinking_fund_items set name = $2, amount_cents = $3, due_month = $4, cycle_months = $5, category_id = $6, note = coalesce($7, note), updated_at = now()
                 where id = $1 returning *",
            )
            .bind(b.id)
            .bind(u.name.trim())
            .bind(u.amount_cents)
            .bind(due.to_string())
            .bind(u.cycle_months)
            .bind(u.category_id)
            .bind(&u.note)
            .fetch_one(&ctx.pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, SinkingFundItem>(
                "insert into sinking_fund_items (fund_id, name, amount_cents, due_month, cycle_months, category_id, note)
                 values ($1, $2, $3, $4, $5, $6, $7) returning *",
            )
            .bind(fund_id)
            .bind(u.name.trim())
            .bind(u.amount_cents)
            .bind(due.to_string())
            .bind(u.cycle_months)
            .bind(u.category_id)
            .bind(u.note.unwrap_or_default())
            .fetch_one(&ctx.pool)
            .await?
        }
    };
    super::audit::record(&ctx.pool, ctx.household_id, actor, "sinking.upsert_item", "sinking_fund_items", after.id, before.as_ref(), Some(&after)).await?;
    Ok(after)
}

pub async fn delete_item(ctx: &Ctx, actor: &Actor, fund_id: Uuid, item_id: Uuid) -> Result<()> {
    get_fund(ctx, fund_id).await?;
    let before = sqlx::query_as::<_, SinkingFundItem>("select * from sinking_fund_items where id = $1 and fund_id = $2")
        .bind(item_id)
        .bind(fund_id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)?;
    sqlx::query("delete from sinking_fund_items where id = $1").bind(item_id).execute(&ctx.pool).await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "sinking.delete_item", "sinking_fund_items", item_id, Some(&before), None::<&()>).await?;
    Ok(())
}

/// Advance a recurring item's due month by one cycle (after the bill posts).
pub async fn advance_item(ctx: &Ctx, actor: &Actor, fund_id: Uuid, item_id: Uuid) -> Result<SinkingFundItem> {
    get_fund(ctx, fund_id).await?;
    let before = sqlx::query_as::<_, SinkingFundItem>("select * from sinking_fund_items where id = $1 and fund_id = $2")
        .bind(item_id)
        .bind(fund_id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)?;
    let input = to_input(&before)?;
    let next = sinking::advance_due_month(&input).ok_or_else(|| invalid("one-off items have no next due month"))?;
    let after = sqlx::query_as::<_, SinkingFundItem>("update sinking_fund_items set due_month = $2, updated_at = now() where id = $1 returning *")
        .bind(item_id)
        .bind(next.to_string())
        .fetch_one(&ctx.pool)
        .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "sinking.advance_item", "sinking_fund_items", item_id, Some(&before), Some(&after)).await?;
    Ok(after)
}

fn to_input(i: &SinkingFundItem) -> Result<SinkingItemInput> {
    Ok(SinkingItemInput {
        id: i.id,
        name: i.name.clone(),
        amount_cents: i.amount_cents,
        due_month: parse_month(&i.due_month)?,
        cycle_months: i.cycle_months.map(|c| c as u32),
        created_month: Month::new(i.created_at.year(), i.created_at.month()).expect("valid"),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct FundWithView {
    #[serde(flatten)]
    pub fund: SinkingFund,
    pub view: SinkingFundView,
}

pub async fn view(ctx: &Ctx, fund_id: Uuid, month: Option<Month>) -> Result<FundWithView> {
    let fund = get_fund(ctx, fund_id).await?;
    let items = list_items(ctx, fund_id).await?;
    let inputs: Vec<SinkingItemInput> = items.iter().map(to_input).collect::<Result<_>>()?;
    let view = sinking::fund_view(&inputs, fund.funded_balance_cents, month.unwrap_or_else(|| ctx.month()));
    Ok(FundWithView { fund, view })
}

pub async fn views(ctx: &Ctx, month: Option<Month>) -> Result<Vec<FundWithView>> {
    let mut out = vec![];
    for f in list_funds(ctx).await? {
        out.push(view(ctx, f.id, month).await?);
    }
    Ok(out)
}
