//! Running estimate and projected low point, assembled from the database.

use hearth_core::balance::{
    self, AccountEstimate, AccountForEstimate, CategoryForLowPoint, LowPoint, PlannedForLowPoint,
    TxnForEstimate,
};
use hearth_core::money::Cents;
use serde::Serialize;

use super::{Ctx, Result};

#[derive(Debug, Clone, Serialize)]
pub struct BalanceOverview {
    pub accounts: Vec<AccountEstimate>,
    pub household_estimate_cents: Cents,
    pub low_point: LowPoint,
    pub cash_floor_cents: Cents,
    /// `low_point − cash_floor`: positive = room above the floor.
    pub headroom_cents: Cents,
}

pub async fn account_estimates(ctx: &Ctx) -> Result<Vec<AccountEstimate>> {
    let accounts = super::accounts::list(ctx, false).await?;
    // Only transactions since the oldest as-of can matter; pull those plus pending.
    let oldest = accounts
        .iter()
        .filter_map(|a| a.reported_balance_as_of)
        .min()
        .map(|t| hearth_core::calendar::local_date(ctx.tz, t))
        .unwrap_or(ctx.today());
    let rows: Vec<(uuid::Uuid, chrono::NaiveDate, Cents, bool, bool)> = sqlx::query_as(
        "select account_id, date, amount_cents, pending, shadow_of_transaction_id is not null
         from transactions where household_id = $1 and deleted_at is null and (date > $2 or pending)",
    )
    .bind(ctx.household_id)
    .bind(oldest)
    .fetch_all(&ctx.pool)
    .await?;
    let txns: Vec<TxnForEstimate> = rows
        .into_iter()
        .map(
            |(account_id, date, amount_cents, pending, shadow)| TxnForEstimate {
                account_id,
                date,
                amount_cents,
                pending,
                deleted: false,
                shadow,
            },
        )
        .collect();
    let today = ctx.today();
    Ok(accounts
        .iter()
        .map(|a| {
            let input = AccountForEstimate {
                id: a.id,
                name: a.name.clone(),
                include_in_running_balance: a.include_in_running_balance,
                reported_balance_cents: a.reported_balance_cents,
                reported_balance_as_of: a.reported_balance_as_of,
                bank_balance_cents: a.bank_balance_cents,
                bank_balance_as_of: a.bank_balance_as_of,
            };
            balance::account_estimate(&input, &txns, ctx.tz, today)
        })
        .collect())
}

pub async fn low_point(ctx: &Ctx, estimate: Cents) -> Result<LowPoint> {
    let settings = super::settings::load(ctx).await?;
    let today = ctx.today();
    let payday = settings.pay_schedule.next_payday(today);
    let view = super::budget::month_view(ctx, ctx.month()).await?;
    let cats: Vec<CategoryForLowPoint> = view
        .groups
        .iter()
        .flat_map(|g| g.categories.iter())
        .map(|c| CategoryForLowPoint {
            id: c.id,
            name: c.name.clone(),
            sign: c.sign,
            plan_cents: c.plan_cents,
            spent_cents: c.activity_cents,
            is_everyday: c.is_everyday,
            due_day: c.due_day,
        })
        .collect();
    let planned: Vec<PlannedForLowPoint> = super::planned::outstanding(ctx)
        .await?
        .into_iter()
        .map(|p| PlannedForLowPoint {
            id: p.id,
            item: p.item,
            date: p.date,
            amount_cents: p.amount_cents,
            outstanding: true,
        })
        .collect();
    Ok(balance::projected_low_point(
        estimate, today, payday, &cats, &planned,
    ))
}

pub async fn overview(ctx: &Ctx) -> Result<BalanceOverview> {
    let accounts = account_estimates(ctx).await?;
    let household = balance::household_estimate(&accounts);
    let lp = low_point(ctx, household).await?;
    let settings = super::settings::load(ctx).await?;
    Ok(BalanceOverview {
        accounts,
        household_estimate_cents: household,
        headroom_cents: lp.low_point_cents - settings.cash_floor_cents,
        low_point: lp,
        cash_floor_cents: settings.cash_floor_cents,
    })
}
