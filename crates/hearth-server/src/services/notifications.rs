//! The delivery log and the "what still needs sending" queries used by `notify`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{Ctx, Result};
use crate::db::models::{Alert, Notification, NotificationChannel, Question};

pub struct Record<'a> {
    pub alert_id: Option<Uuid>,
    pub question_id: Option<Uuid>,
    pub channel: NotificationChannel,
    pub kind: &'a str,
    pub ok: bool,
    pub target: Option<String>,
    pub external_id: Option<String>,
    pub error: Option<String>,
}

pub async fn record(ctx: &Ctx, r: Record<'_>) -> Result<Notification> {
    Ok(sqlx::query_as::<_, Notification>(
        "insert into notifications (alert_id, question_id, channel, kind, status, target, external_id, error)
         values ($1, $2, $3, $4, $5::notification_status, $6, $7, $8) returning *",
    )
    .bind(r.alert_id)
    .bind(r.question_id)
    .bind(r.channel)
    .bind(r.kind)
    .bind(if r.ok { "sent" } else { "failed" })
    .bind(r.target)
    .bind(r.external_id)
    .bind(r.error)
    .fetch_one(&ctx.pool)
    .await?)
}

/// Open alerts nobody has been told about.
pub async fn alerts_to_announce(ctx: &Ctx) -> Result<Vec<Alert>> {
    Ok(sqlx::query_as::<_, Alert>(
        "select * from alerts where household_id = $1 and status = 'open' and notified_at is null
         order by case severity when 'critical' then 0 when 'warn' then 1 else 2 end, first_seen_at",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

/// Announced alerts that are still unacknowledged (nudge candidates).
pub async fn alerts_to_nudge(ctx: &Ctx) -> Result<Vec<Alert>> {
    Ok(sqlx::query_as::<_, Alert>(
        "select * from alerts where household_id = $1 and status = 'notified' order by first_seen_at",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn mark_announced(ctx: &Ctx, id: Uuid, at: DateTime<Utc>) -> Result<()> {
    sqlx::query("update alerts set status = 'notified', notified_at = $2, updated_at = now() where id = $1 and status = 'open'")
        .bind(id)
        .bind(at)
        .execute(&ctx.pool)
        .await?;
    Ok(())
}

pub async fn mark_nudged(ctx: &Ctx, id: Uuid, at: DateTime<Utc>) -> Result<()> {
    sqlx::query("update alerts set last_nudged_at = $2, nudge_count = nudge_count + 1, updated_at = now() where id = $1")
        .bind(id)
        .bind(at)
        .execute(&ctx.pool)
        .await?;
    Ok(())
}

/// Open questions and proposals that asked to be pushed and never were.
pub async fn questions_to_announce(ctx: &Ctx) -> Result<Vec<Question>> {
    Ok(sqlx::query_as::<_, Question>(
        "select q.* from questions q where q.household_id = $1 and q.status = 'open' and q.notify
           and not exists (select 1 from notifications n where n.question_id = q.id)
         order by q.created_at",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn recent(ctx: &Ctx, limit: i64) -> Result<Vec<Notification>> {
    Ok(sqlx::query_as::<_, Notification>(
        "select n.* from notifications n
           left join alerts a on a.id = n.alert_id
           left join questions q on q.id = n.question_id
         where coalesce(a.household_id, q.household_id) = $1
         order by n.sent_at desc limit $2",
    )
    .bind(ctx.household_id)
    .bind(super::clamp_limit(Some(limit)))
    .fetch_all(&ctx.pool)
    .await?)
}
