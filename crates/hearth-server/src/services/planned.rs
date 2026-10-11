use chrono::NaiveDate;
use hearth_core::money::Cents;
use hearth_core::planned::{PlannedCandidate, TxnForPlanned, find_match};
use serde::Deserialize;
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{PlannedItem, PlannedStatus, Transaction};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListQuery {
    pub status: Option<PlannedStatus>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub limit: Option<i64>,
}

pub async fn list(ctx: &Ctx, q: &ListQuery) -> Result<Vec<PlannedItem>> {
    Ok(sqlx::query_as::<_, PlannedItem>(
        "select * from planned_items where household_id = $1
           and ($2::planned_status is null or status = $2)
           and ($3::date is null or date >= $3) and ($4::date is null or date <= $4)
         order by date, created_at limit $5",
    )
    .bind(ctx.household_id)
    .bind(q.status)
    .bind(q.from)
    .bind(q.to)
    .bind(super::clamp_limit(q.limit))
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<PlannedItem> {
    sqlx::query_as::<_, PlannedItem>(
        "select * from planned_items where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePlanned {
    pub date: NaiveDate,
    pub item: String,
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
    pub decided_by: Option<String>,
    pub note: Option<String>,
}

pub async fn create(ctx: &Ctx, actor: &Actor, c: CreatePlanned) -> Result<PlannedItem> {
    if c.item.trim().is_empty() {
        return Err(invalid("item is required"));
    }
    if c.amount_cents.0 < 0 {
        return Err(invalid(
            "amount_cents must be ≥ 0 (planned items are spend)",
        ));
    }
    if let Some(cat) = c.category_id {
        super::budget::get_category(ctx, cat).await?;
    }
    let p = sqlx::query_as::<_, PlannedItem>(
        "insert into planned_items (household_id, date, item, amount_cents, category_id, decided_by, note)
         values ($1, $2, $3, $4, $5, $6, $7) returning *",
    )
    .bind(ctx.household_id)
    .bind(c.date)
    .bind(c.item.trim())
    .bind(c.amount_cents)
    .bind(c.category_id)
    .bind(c.decided_by.unwrap_or_else(|| actor.display()))
    .bind(c.note.unwrap_or_default())
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "planned.create",
        "planned_items",
        p.id,
        None::<&()>,
        Some(&p),
    )
    .await?;
    Ok(p)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdatePlanned {
    pub date: Option<NaiveDate>,
    pub item: Option<String>,
    pub amount_cents: Option<Cents>,
    pub category_id: Option<Uuid>,
    pub status: Option<PlannedStatus>,
    /// Paid where the feed cannot see it; lets `done` stand without a match.
    pub paid_outside_feed: Option<bool>,
    pub decided_by: Option<String>,
    pub note: Option<String>,
}

/// Change a planned item. `done` needs a matched transaction or
/// `paid_outside_feed`: a payment that never posted stays planned (and
/// overdue) instead of quietly leaving the projection.
pub async fn update(ctx: &Ctx, actor: &Actor, id: Uuid, u: UpdatePlanned) -> Result<PlannedItem> {
    let before = get(ctx, id).await?;
    if let Some(a) = u.amount_cents
        && a.0 < 0
    {
        return Err(invalid("amount_cents must be ≥ 0"));
    }
    if u.status == Some(PlannedStatus::Done)
        && before.matched_transaction_id.is_none()
        && !u.paid_outside_feed.unwrap_or(before.paid_outside_feed)
    {
        return Err(invalid(
            "no charge in the feed matches this item: link the transaction, or set paid_outside_feed if it was paid in cash or from elsewhere",
        ));
    }
    let after = sqlx::query_as::<_, PlannedItem>(
        "update planned_items set date = coalesce($3, date), item = coalesce($4, item), amount_cents = coalesce($5, amount_cents),
           category_id = coalesce($6, category_id), status = coalesce($7, status), decided_by = coalesce($8, decided_by),
           note = coalesce($9, note), paid_outside_feed = coalesce($10, paid_outside_feed), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.date)
    .bind(u.item.as_deref().map(str::trim))
    .bind(u.amount_cents)
    .bind(u.category_id)
    .bind(u.status)
    .bind(&u.decided_by)
    .bind(&u.note)
    .bind(u.paid_outside_feed)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "planned.update",
        "planned_items",
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
    status: PlannedStatus,
    note: Option<String>,
) -> Result<PlannedItem> {
    update(
        ctx,
        actor,
        id,
        UpdatePlanned {
            status: Some(status),
            note,
            ..Default::default()
        },
    )
    .await
}

pub async fn delete(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<()> {
    let before = get(ctx, id).await?;
    sqlx::query("delete from planned_items where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .execute(&ctx.pool)
        .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "planned.delete",
        "planned_items",
        id,
        Some(&before),
        None::<&()>,
    )
    .await?;
    Ok(())
}

/// Outstanding items (planned, unmatched).
pub async fn outstanding(ctx: &Ctx) -> Result<Vec<PlannedItem>> {
    Ok(sqlx::query_as::<_, PlannedItem>(
        "select * from planned_items where household_id = $1 and status = 'planned' and matched_transaction_id is null order by date",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

/// Try to match a transaction to an outstanding planned item; link on success.
pub async fn try_match(
    ctx: &Ctx,
    t: &Transaction,
    cfg: &hearth_core::settings::PlannedMatch,
) -> Result<Option<PlannedItem>> {
    if t.planned_item_id.is_some() || t.amount_cents.0 >= 0 {
        return Ok(None);
    }
    let items = outstanding(ctx).await?;
    let cands: Vec<PlannedCandidate> = items
        .iter()
        .map(|p| PlannedCandidate {
            id: p.id,
            date: p.date,
            amount_cents: p.amount_cents,
            category_id: p.category_id,
            outstanding: true,
        })
        .collect();
    let txn = TxnForPlanned {
        date: t.date,
        amount_cents: t.amount_cents,
        category_id: t.category_id,
    };
    let Some(hit) = find_match(&txn, &cands, cfg) else {
        return Ok(None);
    };
    let mut tx = ctx.pool.begin().await?;
    let item = sqlx::query_as::<_, PlannedItem>(
        "update planned_items set matched_transaction_id = $2, status = 'done', updated_at = now() where id = $1 returning *",
    )
    .bind(hit.id)
    .bind(t.id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "update transactions set planned_item_id = $2, category_id = coalesce(category_id, $3),
           review_status = case when category_id is null and $3 is null then review_status else 'none' end, updated_at = now()
         where id = $1",
    )
    .bind(t.id)
    .bind(hit.id)
    .bind(item.category_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(item))
}
