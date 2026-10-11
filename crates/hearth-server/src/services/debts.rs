use chrono::{DateTime, Utc};
use hearth_core::debts::{self, DebtInput, DelayCost, Projection};
use hearth_core::money::Cents;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Debt, DebtKind};

pub async fn list(ctx: &Ctx, include_closed: bool) -> Result<Vec<Debt>> {
    Ok(sqlx::query_as::<_, Debt>(
        "select * from debts where household_id = $1 and ($2 or not is_closed) order by name",
    )
    .bind(ctx.household_id)
    .bind(include_closed)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<Debt> {
    sqlx::query_as::<_, Debt>("select * from debts where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertDebt {
    pub id: Option<Uuid>,
    pub name: String,
    pub kind: Option<DebtKind>,
    pub balance_cents: Cents,
    pub apr_bps: i32,
    pub min_payment_cents: Cents,
    pub planned_payment_cents: Option<Cents>,
    pub due_day: i16,
    pub account_id: Option<Uuid>,
    pub custom_order: Option<i32>,
    pub note: Option<String>,
}

pub async fn upsert(ctx: &Ctx, actor: &Actor, u: UpsertDebt) -> Result<Debt> {
    if u.name.trim().is_empty() {
        return Err(invalid("name is required"));
    }
    if u.balance_cents.0 < 0 || u.min_payment_cents.0 < 0 || u.apr_bps < 0 {
        return Err(invalid("balance, minimum payment and APR must be ≥ 0"));
    }
    if !(1..=31).contains(&u.due_day) {
        return Err(invalid("due_day must be 1–31"));
    }
    let before = match u.id {
        Some(id) => Some(get(ctx, id).await?),
        None => None,
    };
    let after = match &before {
        Some(b) => {
            sqlx::query_as::<_, Debt>(
                "update debts set name = $2, kind = coalesce($3, kind), balance_cents = $4, balance_as_of = case when balance_cents <> $4 then now() else balance_as_of end,
                   apr_bps = $5, min_payment_cents = $6, planned_payment_cents = $7, due_day = $8, account_id = $9, custom_order = $10, note = coalesce($11, note), updated_at = now()
                 where id = $1 returning *",
            )
            .bind(b.id)
            .bind(u.name.trim())
            .bind(u.kind)
            .bind(u.balance_cents)
            .bind(u.apr_bps)
            .bind(u.min_payment_cents)
            .bind(u.planned_payment_cents)
            .bind(u.due_day)
            .bind(u.account_id)
            .bind(u.custom_order)
            .bind(&u.note)
            .fetch_one(&ctx.pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, Debt>(
                "insert into debts (household_id, name, kind, balance_cents, apr_bps, min_payment_cents, planned_payment_cents, due_day, account_id, custom_order, note)
                 values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) returning *",
            )
            .bind(ctx.household_id)
            .bind(u.name.trim())
            .bind(u.kind.unwrap_or(DebtKind::Other))
            .bind(u.balance_cents)
            .bind(u.apr_bps)
            .bind(u.min_payment_cents)
            .bind(u.planned_payment_cents)
            .bind(u.due_day)
            .bind(u.account_id)
            .bind(u.custom_order)
            .bind(u.note.unwrap_or_default())
            .fetch_one(&ctx.pool)
            .await?
        }
    };
    if before
        .as_ref()
        .is_none_or(|b| b.balance_cents != after.balance_cents)
    {
        sqlx::query("insert into balance_snapshots (debt_id, kind, balance_cents, as_of, actor_kind, actor_id) values ($1, 'reported', $2, $3, $4, $5)")
            .bind(after.id)
            .bind(after.balance_cents)
            .bind(after.balance_as_of)
            .bind(actor.kind())
            .bind(actor.id())
            .execute(&ctx.pool)
            .await?;
    }
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "debt.upsert",
        "debts",
        after.id,
        before.as_ref(),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn set_balance(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    balance: Cents,
    as_of: Option<DateTime<Utc>>,
) -> Result<Debt> {
    if balance.0 < 0 {
        return Err(invalid("balance must be ≥ 0"));
    }
    let before = get(ctx, id).await?;
    let as_of = as_of.unwrap_or(ctx.now);
    let mut tx = ctx.pool.begin().await?;
    let after = sqlx::query_as::<_, Debt>(
        "update debts set balance_cents = $3, balance_as_of = $4, updated_at = now() where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(balance)
    .bind(as_of)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("insert into balance_snapshots (debt_id, kind, balance_cents, as_of, actor_kind, actor_id) values ($1, 'reported', $2, $3, $4, $5)")
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
        "debt.set_balance",
        "debts",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}

pub async fn close(ctx: &Ctx, actor: &Actor, id: Uuid, closed: bool) -> Result<Debt> {
    let before = get(ctx, id).await?;
    let after = sqlx::query_as::<_, Debt>("update debts set is_closed = $3, updated_at = now() where household_id = $1 and id = $2 returning *")
        .bind(ctx.household_id)
        .bind(id)
        .bind(closed)
        .fetch_one(&ctx.pool)
        .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "debt.close",
        "debts",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

fn to_input(d: &Debt) -> DebtInput {
    DebtInput {
        id: d.id,
        name: d.name.clone(),
        balance_cents: d.balance_cents,
        apr_bps: d.apr_bps as i64,
        min_payment_cents: d.min_payment_cents,
        planned_payment_cents: d.planned_payment_cents,
        due_day: d.due_day as u32,
        custom_order: d.custom_order,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DebtWithMath {
    #[serde(flatten)]
    pub debt: Debt,
    pub interest_per_day_cents: Cents,
    pub monthly_interest_cents: Cents,
}

pub async fn list_with_math(ctx: &Ctx) -> Result<Vec<DebtWithMath>> {
    Ok(list(ctx, false)
        .await?
        .into_iter()
        .map(|d| DebtWithMath {
            interest_per_day_cents: debts::interest_per_day(d.balance_cents, d.apr_bps as i64),
            monthly_interest_cents: debts::monthly_interest(d.balance_cents, d.apr_bps as i64),
            debt: d,
        })
        .collect())
}

pub async fn projection(ctx: &Ctx) -> Result<Projection> {
    let settings = super::settings::load(ctx).await?;
    let inputs: Vec<DebtInput> = list(ctx, false).await?.iter().map(to_input).collect();
    Ok(debts::project(
        &inputs,
        &settings.debt_strategy,
        ctx.month(),
    ))
}

pub async fn delay_cost(ctx: &Ctx, debt_id: Uuid, amount: Cents) -> Result<DelayCost> {
    if amount.0 <= 0 {
        return Err(invalid("amount_cents must be positive"));
    }
    let settings = super::settings::load(ctx).await?;
    let inputs: Vec<DebtInput> = list(ctx, false).await?.iter().map(to_input).collect();
    debts::delay_cost(
        &inputs,
        &settings.debt_strategy,
        ctx.month(),
        debt_id,
        amount,
    )
    .ok_or(Error::NotFound)
}

/// Previous recorded balance for redraw detection.
pub async fn previous_balance(ctx: &Ctx, debt_id: Uuid) -> Result<Option<Cents>> {
    let row: Option<(Cents,)> = sqlx::query_as(
        "select balance_cents from balance_snapshots where debt_id = $1 order by as_of desc offset 1 limit 1",
    )
    .bind(debt_id)
    .fetch_optional(&ctx.pool)
    .await?;
    Ok(row.map(|r| r.0))
}
