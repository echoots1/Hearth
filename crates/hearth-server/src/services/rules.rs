use hearth_core::money::Cents;
use hearth_core::rules::{self as core, suggest_pattern, validate_pattern};
use serde::Deserialize;
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Rule, RuleAction, RuleMatchType};

pub async fn list(ctx: &Ctx, include_inactive: bool) -> Result<Vec<Rule>> {
    Ok(sqlx::query_as::<_, Rule>(
        "select * from rules where household_id = $1 and ($2 or is_active) order by priority, created_at",
    )
    .bind(ctx.household_id)
    .bind(include_inactive)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<Rule> {
    sqlx::query_as::<_, Rule>("select * from rules where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

/// Active rules as core matching rules, paired with their `set_note`.
pub async fn core_rules(ctx: &Ctx) -> Result<Vec<(core::Rule, Option<String>)>> {
    Ok(list(ctx, false)
        .await?
        .into_iter()
        .map(|r| {
            (
                core::Rule {
                    id: r.id,
                    priority: r.priority,
                    match_type: r.match_type.into(),
                    pattern: r.pattern,
                    account_id: r.account_id,
                    amount_min_cents: r.amount_min_cents,
                    amount_max_cents: r.amount_max_cents,
                    action: r.action.into(),
                    category_id: r.category_id,
                    set_note: r.set_note.clone(),
                    is_active: r.is_active,
                },
                r.set_note,
            )
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRule {
    pub name: Option<String>,
    pub pattern: String,
    pub match_type: Option<RuleMatchType>,
    pub priority: Option<i32>,
    pub account_id: Option<Uuid>,
    pub amount_min_cents: Option<Cents>,
    pub amount_max_cents: Option<Cents>,
    pub action: Option<RuleAction>,
    pub category_id: Option<Uuid>,
    pub set_note: Option<String>,
    /// Apply to existing uncategorized transactions right away (default true).
    pub apply_now: Option<bool>,
}

fn validate(action: RuleAction, category_id: Option<Uuid>, match_type: RuleMatchType, pattern: &str, min: Option<Cents>, max: Option<Cents>) -> Result<()> {
    validate_pattern(match_type.into(), pattern).map_err(invalid)?;
    if action == RuleAction::Categorize && category_id.is_none() {
        return Err(invalid("category_id is required for a categorize rule"));
    }
    if let (Some(a), Some(b)) = (min, max)
        && a > b
    {
        return Err(invalid("amount_min_cents must be ≤ amount_max_cents"));
    }
    Ok(())
}

pub async fn create(ctx: &Ctx, actor: &Actor, c: CreateRule) -> Result<(Rule, super::transactions::ApplyRulesSummary)> {
    let action = c.action.unwrap_or(RuleAction::Categorize);
    let match_type = c.match_type.unwrap_or(RuleMatchType::Contains);
    validate(action, c.category_id, match_type, &c.pattern, c.amount_min_cents, c.amount_max_cents)?;
    if let Some(cat) = c.category_id {
        super::budget::get_category(ctx, cat).await?;
    }
    let name = c.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| c.pattern.clone());
    let r = sqlx::query_as::<_, Rule>(
        "insert into rules (household_id, name, priority, match_type, pattern, account_id, amount_min_cents, amount_max_cents, action, category_id, set_note)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) returning *",
    )
    .bind(ctx.household_id)
    .bind(name.trim())
    .bind(c.priority.unwrap_or(100))
    .bind(match_type)
    .bind(c.pattern.trim())
    .bind(c.account_id)
    .bind(c.amount_min_cents)
    .bind(c.amount_max_cents)
    .bind(action)
    .bind(c.category_id)
    .bind(c.set_note.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "rule.create", "rules", r.id, None::<&()>, Some(&r)).await?;
    let summary = if c.apply_now.unwrap_or(true) {
        super::transactions::apply_rules(ctx, actor, false, None).await?
    } else {
        Default::default()
    };
    Ok((r, summary))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateRule {
    pub name: Option<String>,
    pub pattern: Option<String>,
    pub match_type: Option<RuleMatchType>,
    pub priority: Option<i32>,
    pub account_id: Option<Uuid>,
    pub amount_min_cents: Option<Cents>,
    pub amount_max_cents: Option<Cents>,
    pub action: Option<RuleAction>,
    pub category_id: Option<Uuid>,
    pub set_note: Option<String>,
    pub is_active: Option<bool>,
}

pub async fn update(ctx: &Ctx, actor: &Actor, id: Uuid, u: UpdateRule) -> Result<Rule> {
    let before = get(ctx, id).await?;
    let action = u.action.unwrap_or(before.action);
    let match_type = u.match_type.unwrap_or(before.match_type);
    let pattern = u.pattern.clone().unwrap_or_else(|| before.pattern.clone());
    let category_id = u.category_id.or(before.category_id);
    validate(action, category_id, match_type, &pattern, u.amount_min_cents.or(before.amount_min_cents), u.amount_max_cents.or(before.amount_max_cents))?;
    let after = sqlx::query_as::<_, Rule>(
        "update rules set name = coalesce($3, name), pattern = $4, match_type = $5, priority = coalesce($6, priority),
           account_id = coalesce($7, account_id), amount_min_cents = coalesce($8, amount_min_cents), amount_max_cents = coalesce($9, amount_max_cents),
           action = $10, category_id = $11, set_note = coalesce($12, set_note), is_active = coalesce($13, is_active), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.name.as_deref().map(str::trim))
    .bind(pattern.trim())
    .bind(match_type)
    .bind(u.priority)
    .bind(u.account_id)
    .bind(u.amount_min_cents)
    .bind(u.amount_max_cents)
    .bind(action)
    .bind(category_id)
    .bind(&u.set_note)
    .bind(u.is_active)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "rule.update", "rules", id, Some(&before), Some(&after)).await?;
    Ok(after)
}

pub async fn delete(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<()> {
    let before = get(ctx, id).await?;
    sqlx::query("delete from rules where household_id = $1 and id = $2").bind(ctx.household_id).bind(id).execute(&ctx.pool).await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "rule.delete", "rules", id, Some(&before), None::<&()>).await?;
    Ok(())
}

/// Build a `contains` rule from a transaction's description and categorize it.
pub async fn create_from_transaction(
    ctx: &Ctx,
    actor: &Actor,
    transaction_id: Uuid,
    category_id: Uuid,
    pattern: Option<String>,
) -> Result<(Rule, super::transactions::ApplyRulesSummary)> {
    let t = super::transactions::get(ctx, transaction_id).await?;
    let pattern = pattern.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| suggest_pattern(&t.original_description));
    let (rule, summary) = create(
        ctx,
        actor,
        CreateRule {
            name: Some(pattern.clone()),
            pattern,
            match_type: Some(RuleMatchType::Contains),
            priority: None,
            account_id: None,
            amount_min_cents: None,
            amount_max_cents: None,
            action: Some(RuleAction::Categorize),
            category_id: Some(category_id),
            set_note: None,
            apply_now: Some(true),
        },
    )
    .await?;
    // Make sure the source transaction itself is categorized even if a human had resolved it.
    super::transactions::categorize(ctx, actor, transaction_id, Some(category_id), None).await?;
    Ok((rule, summary))
}

/// Dry-run: which transactions (last 90 days) would this pattern match?
pub async fn preview(ctx: &Ctx, match_type: RuleMatchType, pattern: &str, account_id: Option<Uuid>) -> Result<Vec<crate::db::models::Transaction>> {
    validate_pattern(match_type.into(), pattern).map_err(invalid)?;
    let rule = core::Rule {
        id: Uuid::nil(),
        priority: 0,
        match_type: match_type.into(),
        pattern: pattern.to_string(),
        account_id,
        amount_min_cents: None,
        amount_max_cents: None,
        action: core::RuleAction::Categorize,
        category_id: None,
        set_note: None,
        is_active: true,
    };
    let since = ctx.today() - chrono::Duration::days(90);
    let rows = sqlx::query_as::<_, crate::db::models::Transaction>(
        "select * from transactions where household_id = $1 and deleted_at is null and date >= $2 order by date desc limit 2000",
    )
    .bind(ctx.household_id)
    .bind(since)
    .fetch_all(&ctx.pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|t| core::rule_matches(&rule, &core::TxnForRules { account_id: t.account_id, description: t.description.clone(), amount_cents: t.amount_cents }))
        .collect())
}
