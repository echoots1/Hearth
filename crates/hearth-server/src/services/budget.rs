//! Categories, groups, plan phases, and the month view.

use std::collections::HashMap;

use hearth_core::budget::{self, CategoryInput, GroupInput, MonthView, PlanPhase};
use hearth_core::calendar::Month;
use hearth_core::money::Cents;
use serde::Deserialize;

use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Category, CategoryGroup, CategoryPlan, CategorySign};

pub async fn list_groups(ctx: &Ctx) -> Result<Vec<CategoryGroup>> {
    Ok(sqlx::query_as::<_, CategoryGroup>(
        "select * from category_groups where household_id = $1 order by sort_order, name",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn list_categories(ctx: &Ctx, include_archived: bool) -> Result<Vec<Category>> {
    Ok(sqlx::query_as::<_, Category>(
        "select * from categories where household_id = $1 and ($2 or not is_archived) order by sort_order, name",
    )
    .bind(ctx.household_id)
    .bind(include_archived)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get_category(ctx: &Ctx, id: Uuid) -> Result<Category> {
    sqlx::query_as::<_, Category>("select * from categories where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

pub async fn find_category_by_name(ctx: &Ctx, name: &str) -> Result<Option<Category>> {
    Ok(sqlx::query_as::<_, Category>(
        "select * from categories where household_id = $1 and lower(name) = lower($2) and not is_archived",
    )
    .bind(ctx.household_id)
    .bind(name.trim())
    .fetch_optional(&ctx.pool)
    .await?)
}

pub async fn list_plans(ctx: &Ctx, category_id: Option<Uuid>) -> Result<Vec<CategoryPlan>> {
    Ok(sqlx::query_as::<_, CategoryPlan>(
        "select p.* from category_plans p join categories c on c.id = p.category_id
         where c.household_id = $1 and ($2::uuid is null or p.category_id = $2)
         order by p.category_id, p.starts_month",
    )
    .bind(ctx.household_id)
    .bind(category_id)
    .fetch_all(&ctx.pool)
    .await?)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGroup {
    pub name: String,
    pub sort_order: Option<i32>,
}

pub async fn create_group(ctx: &Ctx, actor: &Actor, c: CreateGroup) -> Result<CategoryGroup> {
    if c.name.trim().is_empty() {
        return Err(invalid("name is required"));
    }
    let g = sqlx::query_as::<_, CategoryGroup>(
        "insert into category_groups (household_id, name, sort_order) values ($1, $2, coalesce($3, (select coalesce(max(sort_order), 0) + 10 from category_groups where household_id = $1))) returning *",
    )
    .bind(ctx.household_id)
    .bind(c.name.trim())
    .bind(c.sort_order)
    .fetch_one(&ctx.pool)
    .await
    .map_err(unique_to_conflict)?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "group.create",
        "category_groups",
        g.id,
        None::<&()>,
        Some(&g),
    )
    .await?;
    Ok(g)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateGroup {
    pub name: Option<String>,
    pub sort_order: Option<i32>,
}

pub async fn update_group(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    u: UpdateGroup,
) -> Result<CategoryGroup> {
    let before = sqlx::query_as::<_, CategoryGroup>(
        "select * from category_groups where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)?;
    let after = sqlx::query_as::<_, CategoryGroup>(
        "update category_groups set name = coalesce($3, name), sort_order = coalesce($4, sort_order), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.name.as_deref().map(str::trim))
    .bind(u.sort_order)
    .fetch_one(&ctx.pool)
    .await
    .map_err(unique_to_conflict)?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "group.update",
        "category_groups",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateCategory {
    pub group_id: Uuid,
    pub name: String,
    pub sign: Option<CategorySign>,
    pub note: Option<String>,
    pub is_everyday: Option<bool>,
    pub sort_order: Option<i32>,
    /// Optional initial plan for the current month.
    pub plan_cents: Option<Cents>,
}

pub async fn create_category(ctx: &Ctx, actor: &Actor, c: CreateCategory) -> Result<Category> {
    if c.name.trim().is_empty() {
        return Err(invalid("name is required"));
    }
    let mut tx = ctx.pool.begin().await?;
    let cat = sqlx::query_as::<_, Category>(
        "insert into categories (household_id, group_id, name, sign, note, is_everyday, sort_order)
         values ($1, $2, $3, $4, $5, $6, coalesce($7, (select coalesce(max(sort_order), 0) + 10 from categories where group_id = $2)))
         returning *",
    )
    .bind(ctx.household_id)
    .bind(c.group_id)
    .bind(c.name.trim())
    .bind(c.sign.unwrap_or(CategorySign::Spend))
    .bind(c.note.unwrap_or_default())
    .bind(c.is_everyday.unwrap_or(false))
    .bind(c.sort_order)
    .fetch_one(&mut *tx)
    .await
    .map_err(unique_to_conflict)?;
    if let Some(plan) = c.plan_cents {
        if plan.0 < 0 {
            return Err(invalid("plan_cents must be ≥ 0"));
        }
        sqlx::query("insert into category_plans (category_id, starts_month, amount_cents) values ($1, $2, $3)")
            .bind(cat.id)
            .bind(ctx.month().to_string())
            .bind(plan)
            .execute(&mut *tx)
            .await?;
    }
    super::audit::record(
        &mut *tx,
        ctx.household_id,
        actor,
        "category.create",
        "categories",
        cat.id,
        None::<&()>,
        Some(&cat),
    )
    .await?;
    tx.commit().await?;
    Ok(cat)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateCategory {
    pub group_id: Option<Uuid>,
    pub name: Option<String>,
    pub sign: Option<CategorySign>,
    pub note: Option<String>,
    pub is_everyday: Option<bool>,
    pub sort_order: Option<i32>,
    pub is_archived: Option<bool>,
}

pub async fn update_category(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    u: UpdateCategory,
) -> Result<Category> {
    let before = get_category(ctx, id).await?;
    let after = sqlx::query_as::<_, Category>(
        "update categories set group_id = coalesce($3, group_id), name = coalesce($4, name), sign = coalesce($5, sign),
         note = coalesce($6, note), is_everyday = coalesce($7, is_everyday),
         sort_order = coalesce($8, sort_order), is_archived = coalesce($9, is_archived), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.group_id)
    .bind(u.name.as_deref().map(str::trim))
    .bind(u.sign)
    .bind(&u.note)
    .bind(u.is_everyday)
    .bind(u.sort_order)
    .bind(u.is_archived)
    .fetch_one(&ctx.pool)
    .await
    .map_err(unique_to_conflict)?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "category.update",
        "categories",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn set_note(ctx: &Ctx, actor: &Actor, id: Uuid, note: String) -> Result<Category> {
    update_category(
        ctx,
        actor,
        id,
        UpdateCategory {
            note: Some(note),
            ..Default::default()
        },
    )
    .await
}

/// Upsert a plan phase for `category_id` starting at `starts_month`.
pub async fn set_plan(
    ctx: &Ctx,
    actor: &Actor,
    category_id: Uuid,
    starts_month: Month,
    amount: Cents,
    label: Option<String>,
    note: Option<String>,
) -> Result<CategoryPlan> {
    if amount.0 < 0 {
        return Err(invalid("amount_cents must be ≥ 0"));
    }
    get_category(ctx, category_id).await?;
    let before = sqlx::query_as::<_, CategoryPlan>(
        "select * from category_plans where category_id = $1 and starts_month = $2",
    )
    .bind(category_id)
    .bind(starts_month.to_string())
    .fetch_optional(&ctx.pool)
    .await?;
    let after = sqlx::query_as::<_, CategoryPlan>(
        "insert into category_plans (category_id, starts_month, amount_cents, label, note) values ($1, $2, $3, $4, $5)
         on conflict (category_id, starts_month) do update set amount_cents = excluded.amount_cents,
           label = coalesce(excluded.label, category_plans.label), note = coalesce(excluded.note, category_plans.note), updated_at = now()
         returning *",
    )
    .bind(category_id)
    .bind(starts_month.to_string())
    .bind(amount)
    .bind(&label)
    .bind(&note)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "budget.set_plan",
        "category_plans",
        after.id,
        before.as_ref(),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn delete_plan(ctx: &Ctx, actor: &Actor, plan_id: Uuid) -> Result<()> {
    let before = sqlx::query_as::<_, CategoryPlan>(
        "select p.* from category_plans p join categories c on c.id = p.category_id where c.household_id = $1 and p.id = $2",
    )
    .bind(ctx.household_id)
    .bind(plan_id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)?;
    sqlx::query("delete from category_plans where id = $1")
        .bind(plan_id)
        .execute(&ctx.pool)
        .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "budget.delete_plan",
        "category_plans",
        plan_id,
        Some(&before),
        None::<&()>,
    )
    .await?;
    Ok(())
}

/// Signed sum per category for a month (`None` key = uncategorized).
/// Excludes deleted and shadow rows.
pub async fn actuals_for_month(ctx: &Ctx, month: Month) -> Result<HashMap<Option<Uuid>, Cents>> {
    let rows: Vec<(Option<Uuid>, Cents)> = sqlx::query_as(
        "select category_id, coalesce(sum(amount_cents), 0)::bigint from transactions
         where household_id = $1 and deleted_at is null and shadow_of_transaction_id is null
           and date >= $2 and date <= $3
         group by category_id",
    )
    .bind(ctx.household_id)
    .bind(month.first_day())
    .bind(month.last_day())
    .fetch_all(&ctx.pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Build the inputs the pure month view needs.
pub async fn category_inputs(
    ctx: &Ctx,
    month: Month,
) -> Result<(Vec<GroupInput>, Vec<CategoryInput>, Cents)> {
    let groups = list_groups(ctx).await?;
    let cats = list_categories(ctx, false).await?;
    let plans = list_plans(ctx, None).await?;
    let actuals = actuals_for_month(ctx, month).await?;
    let bill_totals = super::bills::monthly_totals_by_category(ctx).await?;

    let mut phases: HashMap<Uuid, Vec<PlanPhase>> = HashMap::new();
    for p in plans {
        let Ok(m) = p.starts_month.parse::<Month>() else {
            continue;
        };
        phases.entry(p.category_id).or_default().push(PlanPhase {
            starts_month: m,
            amount_cents: p.amount_cents,
            label: p.label,
        });
    }
    let group_inputs = groups
        .iter()
        .map(|g| GroupInput {
            id: g.id,
            name: g.name.clone(),
            sort_order: g.sort_order,
        })
        .collect();
    let cat_inputs = cats
        .iter()
        .map(|c| CategoryInput {
            id: c.id,
            group_id: c.group_id,
            name: c.name.clone(),
            sign: c.sign.into(),
            note: c.note.clone(),
            is_everyday: c.is_everyday,
            sort_order: c.sort_order,
            bills_total_cents: bill_totals.get(&c.id).copied().unwrap_or(Cents::ZERO),
            phases: phases.remove(&c.id).unwrap_or_default(),
            actual_cents: actuals.get(&Some(c.id)).copied().unwrap_or(Cents::ZERO),
        })
        .collect();
    let uncategorized = actuals.get(&None).copied().unwrap_or(Cents::ZERO);
    Ok((group_inputs, cat_inputs, uncategorized))
}

pub async fn month_view(ctx: &Ctx, month: Month) -> Result<MonthView> {
    let (groups, cats, uncategorized) = category_inputs(ctx, month).await?;
    Ok(budget::month_view(month, &groups, &cats, uncategorized))
}

/// Map a Postgres unique violation to a 409-style error.
pub(crate) fn unique_to_conflict(e: sqlx::Error) -> Error {
    if let sqlx::Error::Database(db) = &e
        && db.is_unique_violation()
    {
        return Error::Conflict("a row with that name or key already exists".into());
    }
    Error::Db(e)
}
