//! Recurring bills: the commitments that drive plans, the low point, the
//! sinking fund, and the canceled-bill alert.

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use hearth_core::bills::{self as core, Bill as CoreBill};
use hearth_core::money::Cents;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{BillSplit, BillStatus, RecurringBill};

pub fn to_core(b: &RecurringBill) -> CoreBill {
    CoreBill {
        id: b.id,
        name: b.name.clone(),
        amount_cents: b.amount_cents,
        cadence_months: b.cadence_months.max(1) as u32,
        per_paycheck: b.per_paycheck,
        next_due: b.next_due,
        category_id: b.category_id,
        account_id: b.account_id,
        match_pattern: b.match_pattern.clone(),
        status: b.status.into(),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListQuery {
    pub status: Option<BillStatus>,
    /// Only bills due before this date (upcoming).
    pub due_before: Option<NaiveDate>,
    pub category_id: Option<Uuid>,
    pub include_canceled: Option<bool>,
}

pub async fn list(ctx: &Ctx, q: &ListQuery) -> Result<Vec<RecurringBill>> {
    Ok(sqlx::query_as::<_, RecurringBill>(
        "select * from recurring_bills where household_id = $1
           and ($2::bill_status is null or status = $2)
           and ($3::date is null or next_due < $3)
           and ($4::uuid is null or category_id = $4)
           and ($5 or status <> 'canceled')
         order by next_due, name",
    )
    .bind(ctx.household_id)
    .bind(q.status)
    .bind(q.due_before)
    .bind(q.category_id)
    .bind(q.include_canceled.unwrap_or(false) || q.status == Some(BillStatus::Canceled))
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn all(ctx: &Ctx) -> Result<Vec<RecurringBill>> {
    Ok(sqlx::query_as::<_, RecurringBill>(
        "select * from recurring_bills where household_id = $1 order by next_due, name",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<RecurringBill> {
    sqlx::query_as::<_, RecurringBill>(
        "select * from recurring_bills where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)
}

pub async fn find_by_name(ctx: &Ctx, name: &str) -> Result<Option<RecurringBill>> {
    Ok(sqlx::query_as::<_, RecurringBill>(
        "select * from recurring_bills where household_id = $1 and lower(name) = lower($2) and status <> 'canceled' limit 1",
    )
    .bind(ctx.household_id)
    .bind(name.trim())
    .fetch_optional(&ctx.pool)
    .await?)
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddBill {
    pub name: String,
    pub amount_cents: Cents,
    /// 1 = monthly, 3 = quarterly, 12 = yearly. Default 1. 0 = every paycheck
    /// (the same as `per_paycheck`).
    pub cadence_months: Option<i16>,
    /// Due on every payday of the household pay schedule.
    pub per_paycheck: Option<bool>,
    pub next_due: NaiveDate,
    pub category_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub match_pattern: Option<String>,
    pub note: Option<String>,
}

/// The cadence as stored: `(cadence_months, per_paycheck)`. `cadence_months`
/// 0 is the API's spelling of per paycheck; a per-paycheck bill keeps
/// `cadence_months` 1 so the column check holds.
fn cadence_of(cadence_months: Option<i16>, per_paycheck: Option<bool>) -> Result<(i16, bool)> {
    let per_paycheck = per_paycheck.unwrap_or(false) || cadence_months == Some(0);
    let cadence = match cadence_months {
        Some(0) | None => 1,
        Some(n) if n < 0 => return Err(invalid("cadence_months must be ≥ 1 (0 = every paycheck)")),
        Some(n) => n,
    };
    Ok((cadence, per_paycheck))
}

fn validate_common(name: &str, amount: Cents) -> Result<()> {
    if name.trim().is_empty() {
        return Err(invalid("name is required"));
    }
    if amount.0 < 0 {
        return Err(invalid("amount_cents must be ≥ 0"));
    }
    Ok(())
}

/// A per-paycheck bill is due on paydays: the first payday on or after `due`.
async fn snap_to_payday(ctx: &Ctx, due: NaiveDate) -> Result<NaiveDate> {
    let settings = super::settings::load(ctx).await?;
    Ok(settings
        .pay_schedule
        .next_payday(due.pred_opt().unwrap_or(due)))
}

pub async fn add(ctx: &Ctx, actor: &Actor, a: AddBill) -> Result<RecurringBill> {
    validate_common(&a.name, a.amount_cents)?;
    let (cadence, per_paycheck) = cadence_of(a.cadence_months, a.per_paycheck)?;
    if let Some(c) = a.category_id {
        super::budget::get_category(ctx, c).await?;
    }
    if let Some(acct) = a.account_id {
        super::accounts::get(ctx, acct).await?;
    }
    let next_due = if per_paycheck {
        snap_to_payday(ctx, a.next_due).await?
    } else {
        a.next_due
    };
    let b = sqlx::query_as::<_, RecurringBill>(
        "insert into recurring_bills (household_id, name, amount_cents, cadence_months, next_due, category_id, account_id, match_pattern, note, per_paycheck)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) returning *",
    )
    .bind(ctx.household_id)
    .bind(a.name.trim())
    .bind(a.amount_cents)
    .bind(cadence)
    .bind(next_due)
    .bind(a.category_id)
    .bind(a.account_id)
    .bind(a.match_pattern.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(a.note.unwrap_or_default())
    .bind(per_paycheck)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "bill.add",
        "recurring_bills",
        b.id,
        None::<&()>,
        Some(&b),
    )
    .await?;
    Ok(b)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateBill {
    pub name: Option<String>,
    pub amount_cents: Option<Cents>,
    /// 0 = every paycheck.
    pub cadence_months: Option<i16>,
    pub per_paycheck: Option<bool>,
    pub next_due: Option<NaiveDate>,
    pub category_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub match_pattern: Option<String>,
    pub note: Option<String>,
    pub status: Option<BillStatus>,
}

pub async fn update(ctx: &Ctx, actor: &Actor, id: Uuid, u: UpdateBill) -> Result<RecurringBill> {
    let before = get(ctx, id).await?;
    validate_common(
        u.name.as_deref().unwrap_or(&before.name),
        u.amount_cents.unwrap_or(before.amount_cents),
    )?;
    // Cadence: untouched unless given; `cadence_months` ≥ 1 also turns per
    // paycheck off, 0 or `per_paycheck` turns it on.
    let (cadence, per_paycheck) = match (u.cadence_months, u.per_paycheck) {
        (None, p) => (None, p),
        (Some(c), p) => {
            let (cadence, per) = cadence_of(Some(c), p)?;
            (Some(cadence), Some(per))
        }
    };
    if let Some(c) = u.category_id {
        super::budget::get_category(ctx, c).await?;
    }
    let next_due = match (per_paycheck.unwrap_or(before.per_paycheck), u.next_due) {
        (true, Some(d)) => Some(snap_to_payday(ctx, d).await?),
        (true, None) if !before.per_paycheck => Some(snap_to_payday(ctx, before.next_due).await?),
        (_, d) => d,
    };
    let after = sqlx::query_as::<_, RecurringBill>(
        "update recurring_bills set name = coalesce($3, name), amount_cents = coalesce($4, amount_cents),
           cadence_months = coalesce($5, cadence_months), next_due = coalesce($6, next_due),
           category_id = coalesce($7, category_id), account_id = coalesce($8, account_id),
           match_pattern = coalesce($9, match_pattern), note = coalesce($10, note), status = coalesce($11, status),
           per_paycheck = coalesce($12, per_paycheck), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.name.as_deref().map(str::trim))
    .bind(u.amount_cents)
    .bind(cadence)
    .bind(next_due)
    .bind(u.category_id)
    .bind(u.account_id)
    .bind(u.match_pattern.as_deref().map(str::trim))
    .bind(&u.note)
    .bind(u.status)
    .bind(per_paycheck)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "bill.update",
        "recurring_bills",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn set_status(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    status: BillStatus,
) -> Result<RecurringBill> {
    update(
        ctx,
        actor,
        id,
        UpdateBill {
            status: Some(status),
            ..Default::default()
        },
    )
    .await
}

pub async fn delete(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<()> {
    let before = get(ctx, id).await?;
    sqlx::query("delete from recurring_bills where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .execute(&ctx.pool)
        .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "bill.delete",
        "recurring_bills",
        id,
        Some(&before),
        None::<&()>,
    )
    .await?;
    Ok(())
}

/// Replace the split for a bill. Only on a person's request (enforced by the caller).
pub async fn set_splits(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    splits: Vec<(Uuid, Cents)>,
) -> Result<Vec<BillSplit>> {
    let bill = get(ctx, id).await?;
    let total: Cents = splits.iter().map(|(_, c)| *c).sum();
    if !splits.is_empty() && total != bill.amount_cents {
        return Err(invalid(format!(
            "splits must total the bill amount ({})",
            bill.amount_cents
        )));
    }
    let mut tx = ctx.pool.begin().await?;
    sqlx::query("delete from bill_splits where bill_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for (cat, amt) in &splits {
        sqlx::query(
            "insert into bill_splits (bill_id, category_id, amount_cents) values ($1, $2, $3)",
        )
        .bind(id)
        .bind(cat)
        .bind(amt)
        .execute(&mut *tx)
        .await?;
    }
    let rows = sqlx::query_as::<_, BillSplit>("select * from bill_splits where bill_id = $1")
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    super::audit::record(
        &mut *tx,
        ctx.household_id,
        actor,
        "bill.set_splits",
        "recurring_bills",
        id,
        None::<&()>,
        Some(&rows),
    )
    .await?;
    tx.commit().await?;
    Ok(rows)
}

pub async fn splits(ctx: &Ctx, id: Uuid) -> Result<Vec<BillSplit>> {
    Ok(
        sqlx::query_as::<_, BillSplit>("select * from bill_splits where bill_id = $1")
            .bind(id)
            .fetch_all(&ctx.pool)
            .await?,
    )
}

/// Paydays in the current month under the household schedule: what a
/// per-paycheck bill costs this month is its amount times this.
pub async fn paychecks_this_month(ctx: &Ctx) -> Result<u32> {
    let settings = super::settings::load(ctx).await?;
    Ok(settings.pay_schedule.paydays_in_month(ctx.month()).len() as u32)
}

/// The splits of every active bill, in one query.
pub async fn active_splits(ctx: &Ctx) -> Result<Vec<BillSplit>> {
    Ok(sqlx::query_as::<_, BillSplit>(
        "select s.* from bill_splits s join recurring_bills b on b.id = s.bill_id where b.household_id = $1 and b.status = 'active'",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

/// Monthly-equivalent total of active bills per category for the current
/// month (splits honored).
pub async fn monthly_totals_by_category(ctx: &Ctx) -> Result<HashMap<Uuid, Cents>> {
    let bills = all(ctx).await?;
    let n = paychecks_this_month(ctx).await?;
    let core_bills: Vec<CoreBill> = bills.iter().map(to_core).collect();
    let mut totals = core::monthly_total_by_category(&core_bills, n);
    // Splits: move the bill's amount from its category to the split categories.
    for s in active_splits(ctx).await? {
        if let Some(b) = bills.iter().find(|b| b.id == s.bill_id) {
            let share = if b.per_paycheck {
                Cents(s.amount_cents.0 * n as i64)
            } else {
                s.amount_cents.div_ceil(b.cadence_months.max(1) as i64)
            };
            if let Some(c) = b.category_id
                && let Some(t) = totals.get_mut(&c)
            {
                *t -= share;
            }
            *totals.entry(s.category_id).or_insert(Cents::ZERO) += share;
        }
    }
    Ok(totals)
}

/// After a transaction pays the bill: record the match and advance `next_due`
/// past the transaction date. Canceled bills are never advanced.
pub async fn record_match(
    ctx: &Ctx,
    id: Uuid,
    transaction_id: Uuid,
    txn_date: NaiveDate,
) -> Result<RecurringBill> {
    let b = get(ctx, id).await?;
    let next = if b.status == BillStatus::Canceled {
        b.next_due
    } else {
        let schedule = super::settings::load(ctx).await?.pay_schedule;
        let core_bill = to_core(&b);
        let (due, _) = core_bill.catch_up(
            core_bill.next_occurrence(b.next_due, &schedule),
            &schedule,
            txn_date.succ_opt().unwrap_or(txn_date),
        );
        due
    };
    Ok(sqlx::query_as::<_, RecurringBill>(
        "update recurring_bills set next_due = $3, last_matched_transaction = $4, last_matched_at = now(), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(next)
    .bind(transaction_id)
    .fetch_one(&ctx.pool)
    .await?)
}

/// Bills as the sinking-fund math wants them: every active non-monthly bill.
pub fn sinking_items(bills: &[RecurringBill]) -> Vec<hearth_core::sinking::SinkingItemInput> {
    bills
        .iter()
        .filter(|b| b.status == BillStatus::Active && b.cadence_months > 1 && !b.per_paycheck)
        .map(|b| hearth_core::sinking::SinkingItemInput {
            id: b.id,
            name: b.name.clone(),
            amount_cents: b.amount_cents,
            due_month: hearth_core::calendar::Month::of(b.next_due),
            cycle_months: Some(b.cadence_months as u32),
            created_month: hearth_core::calendar::Month::new(
                b.created_at.year(),
                b.created_at.month(),
            )
            .expect("valid"),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct BillView {
    #[serde(flatten)]
    pub bill: RecurringBill,
    pub monthly_equivalent_cents: Cents,
    pub category_name: Option<String>,
    pub splits: Vec<BillSplit>,
}

pub async fn views(ctx: &Ctx, q: &ListQuery) -> Result<Vec<BillView>> {
    let bills = list(ctx, q).await?;
    let n = paychecks_this_month(ctx).await?;
    let cats = super::budget::list_categories(ctx, true).await?;
    let all_splits = sqlx::query_as::<_, BillSplit>(
        "select s.* from bill_splits s join recurring_bills b on b.id = s.bill_id where b.household_id = $1",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?;
    Ok(bills
        .into_iter()
        .map(|b| BillView {
            monthly_equivalent_cents: to_core(&b).monthly_equivalent(n),
            category_name: b
                .category_id
                .and_then(|c| cats.iter().find(|x| x.id == c))
                .map(|c| c.name.clone()),
            splits: all_splits
                .iter()
                .filter(|s| s.bill_id == b.id)
                .cloned()
                .collect(),
            bill: b,
        })
        .collect())
}
