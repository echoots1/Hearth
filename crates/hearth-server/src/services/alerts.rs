use chrono::{DateTime, Utc};
use hearth_core::alerts::AlertCandidate;
use serde::Deserialize;
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Alert, AlertSeverity, AlertStatus, AlertType};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListQuery {
    pub status: Option<AlertStatus>,
    /// Convenience: `open`, `notified`, `ack` (everything not done).
    pub live: Option<bool>,
    pub severity: Option<AlertSeverity>,
    #[serde(rename = "type")]
    pub alert_type: Option<AlertType>,
    pub limit: Option<i64>,
}

pub async fn list(ctx: &Ctx, q: &ListQuery) -> Result<Vec<Alert>> {
    Ok(sqlx::query_as::<_, Alert>(
        "select * from alerts where household_id = $1
           and ($2::alert_status is null or status = $2)
           and (not $3 or status <> 'done')
           and ($4::alert_severity is null or severity = $4)
           and ($5::alert_type is null or type = $5)
         order by case severity when 'critical' then 0 when 'warn' then 1 else 2 end, first_seen_at desc
         limit $6",
    )
    .bind(ctx.household_id)
    .bind(q.status)
    .bind(q.live.unwrap_or(false))
    .bind(q.severity)
    .bind(q.alert_type)
    .bind(super::clamp_limit(q.limit))
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<Alert> {
    sqlx::query_as::<_, Alert>("select * from alerts where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

pub async fn ack(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<Alert> {
    let before = get(ctx, id).await?;
    if before.status == AlertStatus::Done {
        return Err(invalid("alert is already done"));
    }
    let after = sqlx::query_as::<_, Alert>(
        "update alerts set status = 'ack', acked_by = $3, acked_at = now(), updated_at = now() where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(actor.display())
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "alert.ack", "alerts", id, Some(&before), Some(&after)).await?;
    Ok(after)
}

pub async fn resolve(ctx: &Ctx, actor: &Actor, id: Uuid, resolution: String) -> Result<Alert> {
    let before = get(ctx, id).await?;
    let after = sqlx::query_as::<_, Alert>(
        "update alerts set status = 'done', resolution = $3, resolved_at = now(),
           acked_by = coalesce(acked_by, $4), acked_at = coalesce(acked_at, now()), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(resolution.trim())
    .bind(actor.display())
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "alert.resolve", "alerts", id, Some(&before), Some(&after)).await?;
    Ok(after)
}

pub async fn snooze(ctx: &Ctx, actor: &Actor, id: Uuid, until: DateTime<Utc>) -> Result<Alert> {
    let before = get(ctx, id).await?;
    if until <= ctx.now {
        return Err(invalid("snooze time must be in the future"));
    }
    let after = sqlx::query_as::<_, Alert>("update alerts set snoozed_until = $3, updated_at = now() where household_id = $1 and id = $2 returning *")
        .bind(ctx.household_id)
        .bind(id)
        .bind(until)
        .fetch_one(&ctx.pool)
        .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "alert.snooze", "alerts", id, Some(&before), Some(&after)).await?;
    Ok(after)
}

/// Reopen a done alert (rare; e.g. acknowledged by mistake).
pub async fn reopen(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<Alert> {
    let before = get(ctx, id).await?;
    let after = sqlx::query_as::<_, Alert>(
        "update alerts set status = 'open', resolution = null, resolved_at = null, acked_by = null, acked_at = null, updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_one(&ctx.pool)
    .await
    .map_err(super::budget::unique_to_conflict)?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "alert.reopen", "alerts", id, Some(&before), Some(&after)).await?;
    Ok(after)
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct UpsertSummary {
    pub created: usize,
    pub refreshed: usize,
}

/// Materialize evaluator output: insert new live alerts by dedupe key, refresh
/// `last_seen_at`/detail/amount on existing ones. Never touches `status`.
pub async fn upsert_candidates(ctx: &Ctx, source: &str, candidates: &[AlertCandidate]) -> Result<UpsertSummary> {
    let mut summary = UpsertSummary::default();
    for c in candidates {
        let existing: Option<(Uuid,)> = sqlx::query_as("select id from alerts where household_id = $1 and dedupe_key = $2 and status <> 'done'")
            .bind(ctx.household_id)
            .bind(&c.dedupe_key)
            .fetch_optional(&ctx.pool)
            .await?;
        match existing {
            Some((id,)) => {
                sqlx::query(
                    "update alerts set last_seen_at = $2, detail = $3, amount_cents = $4, context = $5, severity = $6, updated_at = now() where id = $1",
                )
                .bind(id)
                .bind(ctx.now)
                .bind(&c.detail)
                .bind(c.amount_cents)
                .bind(&c.context)
                .bind(AlertSeverity::from(c.severity))
                .execute(&ctx.pool)
                .await?;
                summary.refreshed += 1;
            }
            None => {
                sqlx::query(
                    "insert into alerts (household_id, type, severity, title, detail, amount_cents, source, dedupe_key, transaction_id, category_id, debt_id, account_id, planned_item_id, context, first_seen_at, last_seen_at)
                     values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $15)",
                )
                .bind(ctx.household_id)
                .bind(AlertType::from(c.alert_type))
                .bind(AlertSeverity::from(c.severity))
                .bind(&c.title)
                .bind(&c.detail)
                .bind(c.amount_cents)
                .bind(source)
                .bind(&c.dedupe_key)
                .bind(c.transaction_id)
                .bind(c.category_id)
                .bind(c.debt_id)
                .bind(c.account_id)
                .bind(c.planned_item_id)
                .bind(&c.context)
                .bind(ctx.now)
                .execute(&ctx.pool)
                .await?;
                summary.created += 1;
            }
        }
    }
    Ok(summary)
}

pub async fn counts(ctx: &Ctx) -> Result<(i64, i64, i64)> {
    let row: (i64, i64, i64) = sqlx::query_as(
        "select count(*) filter (where status in ('open','notified'))::bigint, count(*) filter (where status = 'ack')::bigint,
                count(*) filter (where status <> 'done' and severity = 'critical')::bigint
         from alerts where household_id = $1",
    )
    .bind(ctx.household_id)
    .fetch_one(&ctx.pool)
    .await?;
    Ok(row)
}
