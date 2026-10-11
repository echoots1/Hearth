//! Running estimate and projected low point, assembled from the database.

use hearth_core::balance::{
    self, AccountEstimate, AccountForEstimate, BillForLowPoint, CategoryForLowPoint, LowPoint,
    PlannedForLowPoint, TxnForEstimate,
};
use hearth_core::money::Cents;
use hearth_core::timeline::{
    BillForTimeline, BillSplitShare, CategoryForTimeline, PlannedForTimeline, Timeline,
    bills_cover_by_category, cash_timeline,
};
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
    // Only transactions since the oldest anchor can matter; pull those plus
    // pending. Either balance may anchor, so the oldest of both bounds it.
    let oldest = accounts
        .iter()
        .flat_map(|a| [a.reported_balance_as_of, a.bank_balance_as_of])
        .flatten()
        .min()
        .map(|t| hearth_core::calendar::local_date(ctx.tz, t))
        .unwrap_or(ctx.today());
    // A shadow of a row on the same account (the bank's PwP line for a
    // Privacy.com charge) is a duplicate; a shadow of a row on another account
    // is the bank's view of cash and counts.
    let rows: Vec<(uuid::Uuid, chrono::NaiveDate, Cents, bool, bool)> = sqlx::query_as(
        "select t.account_id, t.date, t.amount_cents, t.pending,
                exists (select 1 from transactions p where p.id = t.shadow_of_transaction_id and p.account_id = t.account_id)
         from transactions t where t.household_id = $1 and t.deleted_at is null and (t.date > $2 or t.pending)",
    )
    .bind(ctx.household_id)
    .bind(oldest)
    .fetch_all(&ctx.pool)
    .await?;
    let txns: Vec<TxnForEstimate> = rows
        .into_iter()
        .map(
            |(account_id, date, amount_cents, pending, duplicate)| TxnForEstimate {
                account_id,
                date,
                amount_cents,
                pending,
                deleted: false,
                duplicate,
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
                include_in_cash: a.include_in_cash,
                reported_balance_cents: a.reported_balance_cents,
                reported_balance_as_of: a.reported_balance_as_of,
                bank_balance_cents: a.bank_balance_cents,
                bank_balance_as_of: a.bank_balance_as_of,
            };
            balance::account_estimate(&input, &txns, ctx.tz, today)
        })
        .collect())
}

/// Every active bill with its splits, as the timeline projects them.
async fn active_bills(ctx: &Ctx) -> Result<Vec<BillForTimeline>> {
    let bills = super::bills::list(
        ctx,
        &super::bills::ListQuery {
            status: Some(crate::db::models::BillStatus::Active),
            ..Default::default()
        },
    )
    .await?;
    let mut splits: std::collections::HashMap<uuid::Uuid, Vec<BillSplitShare>> =
        std::collections::HashMap::new();
    for s in super::bills::active_splits(ctx).await? {
        splits.entry(s.bill_id).or_default().push(BillSplitShare {
            category_id: s.category_id,
            amount_cents: s.amount_cents,
        });
    }
    Ok(bills
        .into_iter()
        .map(|b| BillForTimeline {
            splits: splits.remove(&b.id).unwrap_or_default(),
            id: b.id,
            name: b.name,
            next_due: b.next_due,
            cadence_months: b.cadence_months.max(1) as u32,
            per_paycheck: b.per_paycheck,
            amount_cents: b.amount_cents,
            category_id: b.category_id,
        })
        .collect())
}

pub async fn low_point(ctx: &Ctx, estimate: Cents) -> Result<LowPoint> {
    let settings = super::settings::load(ctx).await?;
    let today = ctx.today();
    let payday = settings.pay_schedule.next_payday(today);
    let month = ctx.month();
    let view = super::budget::month_view(ctx, month).await?;
    // Every active bill: the ones due before payday are projected on their
    // dates, including ones past due with no matching charge yet (`next_due`
    // only moves when the charge posts); the rest of the month's occurrences
    // decide what each category's plan still has to cover.
    let active = active_bills(ctx).await?;
    let cover = bills_cover_by_category(&active, month, today, &settings.pay_schedule);
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
            bills_cover_cents: cover.get(&c.id).copied().unwrap_or(Cents::ZERO),
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
    let bills: Vec<BillForLowPoint> = active
        .into_iter()
        .filter(|b| b.next_due < payday)
        .map(|b| BillForLowPoint {
            id: b.id,
            name: b.name,
            due: b.next_due,
            amount_cents: b.amount_cents,
            posted: false,
        })
        .collect();
    Ok(balance::projected_low_point(
        estimate, today, payday, &cats, &bills, &planned,
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineView {
    pub household_estimate_cents: Cents,
    pub cash_floor_cents: Cents,
    /// `timeline.low − cash_floor`: positive = room above the floor.
    pub headroom_cents: Cents,
    pub timeline: Timeline,
}

/// Default and longest horizons for the cash timeline, in days.
pub const TIMELINE_DEFAULT_DAYS: i64 = 60;
pub const TIMELINE_MAX_DAYS: i64 = 366;

/// Expected cash day by day from the current estimate: paychecks, every bill
/// occurrence, planned one-offs and the everyday drain, with the running
/// balance after each, for `days` days from today.
pub async fn timeline(ctx: &Ctx, days: Option<i64>) -> Result<TimelineView> {
    let days = days
        .unwrap_or(TIMELINE_DEFAULT_DAYS)
        .clamp(1, TIMELINE_MAX_DAYS);
    let settings = super::settings::load(ctx).await?;
    let accounts = account_estimates(ctx).await?;
    let estimate = balance::household_estimate(&accounts);
    let today = ctx.today();
    let until = today + chrono::Duration::days(days);
    let mut phases = super::budget::phases_by_category(ctx).await?;
    let actuals = super::budget::actuals_for_month(ctx, ctx.month()).await?;
    let categories: Vec<CategoryForTimeline> = super::budget::list_categories(ctx, false)
        .await?
        .into_iter()
        .map(|c| {
            let sign: hearth_core::budget::Sign = c.sign.into();
            let actual = actuals.get(&Some(c.id)).copied().unwrap_or(Cents::ZERO);
            CategoryForTimeline {
                id: c.id,
                name: c.name,
                sign,
                is_everyday: c.is_everyday,
                plan_per_paycheck: c.plan_per_paycheck,
                pay_schedule: c.pay_schedule.map(|j| j.0),
                phases: phases.remove(&c.id).unwrap_or_default(),
                activity_cents: match sign {
                    hearth_core::budget::Sign::Spend => -actual,
                    hearth_core::budget::Sign::Income => actual,
                },
            }
        })
        .collect();
    let bills = active_bills(ctx).await?;
    let planned: Vec<PlannedForTimeline> = super::planned::outstanding(ctx)
        .await?
        .into_iter()
        .map(|p| PlannedForTimeline {
            id: p.id,
            item: p.item,
            date: p.date,
            amount_cents: p.amount_cents,
            category_id: p.category_id,
        })
        .collect();
    let timeline = cash_timeline(
        estimate,
        today,
        until,
        &settings.pay_schedule,
        &categories,
        &bills,
        &planned,
    );
    Ok(TimelineView {
        household_estimate_cents: estimate,
        cash_floor_cents: settings.cash_floor_cents,
        headroom_cents: timeline.low_cents - settings.cash_floor_cents,
        timeline,
    })
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
