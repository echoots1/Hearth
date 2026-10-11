//! Savings goals and their projections.

use chrono::{DateTime, NaiveDate, Utc};
use hearth_core::calendar::Month;
use hearth_core::goals::{self, GoalInput, GoalProjection};
use hearth_core::money::Cents;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Goal, GoalStatus};

pub async fn list(ctx: &Ctx, include_inactive: bool) -> Result<Vec<Goal>> {
    Ok(sqlx::query_as::<_, Goal>(
        "select * from goals where household_id = $1 and ($2 or status in ('active', 'reached')) order by created_at",
    )
    .bind(ctx.household_id)
    .bind(include_inactive)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<Goal> {
    sqlx::query_as::<_, Goal>("select * from goals where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGoal {
    pub name: String,
    pub target_cents: Cents,
    pub target_date: Option<NaiveDate>,
    pub account_id: Option<Uuid>,
    /// The category whose plan is the monthly contribution.
    pub category_id: Option<Uuid>,
    pub saved_cents: Option<Cents>,
    pub note: Option<String>,
}

pub async fn create(ctx: &Ctx, actor: &Actor, c: CreateGoal) -> Result<Goal> {
    if c.name.trim().is_empty() {
        return Err(invalid("name is required"));
    }
    if c.target_cents.0 <= 0 {
        return Err(invalid("target_cents must be positive"));
    }
    let g = sqlx::query_as::<_, Goal>(
        "insert into goals (household_id, name, target_cents, target_date, account_id, category_id, saved_cents, saved_as_of, note)
         values ($1, $2, $3, $4, $5, $6, coalesce($7, 0), case when $7 is null then null else now() end, $8) returning *",
    )
    .bind(ctx.household_id)
    .bind(c.name.trim())
    .bind(c.target_cents)
    .bind(c.target_date)
    .bind(c.account_id)
    .bind(c.category_id)
    .bind(c.saved_cents)
    .bind(c.note.unwrap_or_default())
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "goal.create",
        "goals",
        g.id,
        None::<&()>,
        Some(&g),
    )
    .await?;
    Ok(g)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateGoal {
    pub name: Option<String>,
    pub target_cents: Option<Cents>,
    pub target_date: Option<NaiveDate>,
    pub account_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub status: Option<GoalStatus>,
    pub note: Option<String>,
}

pub async fn update(ctx: &Ctx, actor: &Actor, id: Uuid, u: UpdateGoal) -> Result<Goal> {
    let before = get(ctx, id).await?;
    if let Some(t) = u.target_cents
        && t.0 <= 0
    {
        return Err(invalid("target_cents must be positive"));
    }
    let after = sqlx::query_as::<_, Goal>(
        "update goals set name = coalesce($3, name), target_cents = coalesce($4, target_cents), target_date = coalesce($5, target_date),
           account_id = coalesce($6, account_id), category_id = coalesce($7, category_id), status = coalesce($8, status),
           note = coalesce($9, note), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.name.as_deref().map(str::trim))
    .bind(u.target_cents)
    .bind(u.target_date)
    .bind(u.account_id)
    .bind(u.category_id)
    .bind(u.status)
    .bind(&u.note)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "goal.update",
        "goals",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

/// Record what's saved toward the goal; marks reached when the target is met.
pub async fn set_saved(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    saved: Cents,
    as_of: Option<DateTime<Utc>>,
) -> Result<Goal> {
    if saved.0 < 0 {
        return Err(invalid("saved_cents must be ≥ 0"));
    }
    let before = get(ctx, id).await?;
    let as_of = as_of.unwrap_or(ctx.now);
    let mut tx = ctx.pool.begin().await?;
    let after = sqlx::query_as::<_, Goal>(
        "update goals set saved_cents = $3, saved_as_of = $4,
           status = case when $3 >= target_cents and status = 'active' then 'reached'::goal_status else status end,
           reached_at = case when $3 >= target_cents and reached_at is null then now() else reached_at end,
           updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(saved)
    .bind(as_of)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("insert into balance_snapshots (goal_id, kind, balance_cents, as_of, actor_kind, actor_id) values ($1, 'reported', $2, $3, $4, $5)")
        .bind(id)
        .bind(saved)
        .bind(as_of)
        .bind(actor.kind())
        .bind(actor.id())
        .execute(&mut *tx)
        .await?;
    super::audit::record(
        &mut *tx,
        ctx.household_id,
        actor,
        "goal.set_saved",
        "goals",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}

#[derive(Debug, Clone, Serialize)]
pub struct GoalWithProjection {
    #[serde(flatten)]
    pub goal: Goal,
    pub projection: GoalProjection,
}

/// Projection using the goal's category plan (this month) as the contribution.
pub async fn projections(ctx: &Ctx) -> Result<Vec<GoalWithProjection>> {
    let goals_rows = list(ctx, false).await?;
    let month: Month = ctx.month();
    let plans = super::budget::list_plans(ctx, None).await?;
    let today = ctx.today();
    Ok(goals_rows
        .into_iter()
        .map(|g| {
            let monthly = g
                .category_id
                .map(|c| {
                    let phases: Vec<hearth_core::budget::PlanPhase> = plans
                        .iter()
                        .filter(|p| p.category_id == c)
                        .filter_map(|p| {
                            Some(hearth_core::budget::PlanPhase {
                                starts_month: p.starts_month.parse().ok()?,
                                amount_cents: p.amount_cents,
                                label: p.label.clone(),
                            })
                        })
                        .collect();
                    hearth_core::budget::effective_plan(&phases, month)
                })
                .unwrap_or(Cents::ZERO);
            let input = GoalInput {
                id: g.id,
                name: g.name.clone(),
                target_cents: g.target_cents,
                saved_cents: g.saved_cents,
                target_date: g.target_date,
                monthly_cents: monthly,
            };
            GoalWithProjection {
                projection: goals::project(&input, today),
                goal: g,
            }
        })
        .collect())
}
