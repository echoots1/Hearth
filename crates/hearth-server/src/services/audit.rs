use serde::Serialize;
use sqlx::PgExecutor;

use super::{Actor, Result};

/// Record a write. `before`/`after` are serialized snapshots (or `None`).
#[allow(clippy::too_many_arguments)]
pub async fn record<'e, B: Serialize, A: Serialize>(
    exec: impl PgExecutor<'e>,
    household_id: uuid::Uuid,
    actor: &Actor,
    action: &str,
    entity: &str,
    entity_id: impl ToString,
    before: Option<&B>,
    after: Option<&A>,
) -> Result<()> {
    let before = before
        .map(serde_json::to_value)
        .transpose()
        .map_err(anyhow::Error::from)?;
    let after = after
        .map(serde_json::to_value)
        .transpose()
        .map_err(anyhow::Error::from)?;
    sqlx::query(
        "insert into audit_log (household_id, actor_kind, actor_id, action, entity, entity_id, before, after)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(household_id)
    .bind(actor.kind())
    .bind(actor.id())
    .bind(action)
    .bind(entity)
    .bind(entity_id.to_string())
    .bind(before)
    .bind(after)
    .execute(exec)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AuditQuery {
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub limit: Option<i64>,
}

pub async fn list(ctx: &super::Ctx, q: &AuditQuery) -> Result<Vec<crate::db::models::AuditEntry>> {
    let rows = sqlx::query_as::<_, crate::db::models::AuditEntry>(
        "select * from audit_log
         where household_id = $1
           and ($2::text is null or entity = $2)
           and ($3::text is null or entity_id = $3)
         order by at desc limit $4",
    )
    .bind(ctx.household_id)
    .bind(&q.entity)
    .bind(&q.entity_id)
    .bind(super::clamp_limit(q.limit))
    .fetch_all(&ctx.pool)
    .await?;
    Ok(rows)
}
