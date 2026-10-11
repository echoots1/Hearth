//! The assistant's door into Hearth: the autonomy gate, the record of what it
//! did (with undo), and the work queue a scheduled pass reads.

use chrono::{DateTime, Utc};
use hearth_core::money::Cents;
use hearth_core::settings::{Autonomy, AutonomyArea as CoreArea};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::questions::{Proposal, ProposeInput};
use super::{Actor, Ctx, Error, Result};
use crate::db::models::{
    Alert, AssistantAction, AutonomyArea, Question, RecurringBill, Transaction,
};

/// What a gated write turned into.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Gated<T: Serialize> {
    /// Driver: the change was applied; here is the result and its action record.
    Applied { result: T, action: AssistantAction },
    /// Backseat: a proposal was recorded instead; nothing changed.
    Proposed { question: Question },
}

/// Describe a write before it happens so the gate can propose or record it.
#[derive(Debug, Clone)]
pub struct WriteIntent {
    pub area: CoreArea,
    pub tool: String,
    pub args: serde_json::Value,
    /// One line, e.g. "Groceries plan $800 → $900".
    pub summary: String,
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub transaction_id: Option<Uuid>,
    pub bill_id: Option<Uuid>,
}

/// Tools that remove data outright; the assistant can only propose these.
pub fn is_destructive(tool: &str) -> bool {
    tool.ends_with(".delete") || tool == "budget.delete_plan"
}

pub async fn autonomy_for(ctx: &Ctx, area: CoreArea) -> Result<Autonomy> {
    let s = super::settings::load(ctx).await?;
    Ok(s.assistant.autonomy_for(area))
}

/// Run a write under the autonomy switch. Humans always pass straight through.
///
/// `apply` performs the change and returns `(result, before, after, undo)`.
pub async fn gate<T, F, Fut>(
    ctx: &Ctx,
    actor: &Actor,
    intent: WriteIntent,
    apply: F,
) -> Result<Gated<T>>
where
    T: Serialize,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<
            Output = Result<(
                T,
                Option<serde_json::Value>,
                Option<serde_json::Value>,
                Option<Proposal>,
            )>,
        >,
{
    // Destructive tools always go through a person, whatever the autonomy
    // position: text the assistant reads (descriptions, notes, imports) is
    // not trusted enough to delete on.
    let must_propose = actor.is_assistant()
        && (is_destructive(&intent.tool)
            || autonomy_for(ctx, intent.area).await? == Autonomy::Backseat);
    if must_propose {
        let q = super::questions::propose(
            ctx,
            actor,
            ProposeInput {
                area: intent.area.into(),
                proposal: Proposal {
                    tool: intent.tool.clone(),
                    args: intent.args.clone(),
                    summary: intent.summary.clone(),
                },
                question: format!("Approve: {}", intent.summary),
                transaction_id: intent.transaction_id,
                bill_id: intent.bill_id,
                context: None,
            },
        )
        .await?;
        return Ok(Gated::Proposed { question: q });
    }
    let (result, before, after, undo) = apply().await?;
    let key_id = match actor {
        Actor::Assistant { key_id, .. } => *key_id,
        _ => None,
    };
    let action = if actor.is_assistant() {
        record_action(ctx, key_id, &intent, before, after, undo).await?
    } else {
        // Humans' writes are audited by the services; no assistant action row.
        AssistantAction {
            id: Uuid::nil(),
            household_id: ctx.household_id,
            api_key_id: None,
            area: intent.area.into(),
            tool: intent.tool,
            args: intent.args,
            summary: intent.summary,
            entity: intent.entity,
            entity_id: intent.entity_id,
            before,
            after,
            undo: None,
            undone_at: None,
            undone_by: None,
            created_at: ctx.now,
        }
    };
    Ok(Gated::Applied { result, action })
}

async fn record_action(
    ctx: &Ctx,
    key_id: Option<Uuid>,
    intent: &WriteIntent,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
    undo: Option<Proposal>,
) -> Result<AssistantAction> {
    let undo_v = undo.map(|u| serde_json::to_value(u).unwrap_or_default());
    Ok(sqlx::query_as::<_, AssistantAction>(
        "insert into assistant_actions (household_id, api_key_id, area, tool, args, summary, entity, entity_id, before, after, undo)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) returning *",
    )
    .bind(ctx.household_id)
    .bind(key_id)
    .bind(AutonomyArea::from(intent.area))
    .bind(&intent.tool)
    .bind(&intent.args)
    .bind(&intent.summary)
    .bind(&intent.entity)
    .bind(&intent.entity_id)
    .bind(before)
    .bind(after)
    .bind(undo_v)
    .fetch_one(&ctx.pool)
    .await?)
}

pub async fn actions(
    ctx: &Ctx,
    limit: Option<i64>,
    undoable_only: bool,
) -> Result<Vec<AssistantAction>> {
    Ok(sqlx::query_as::<_, AssistantAction>(
        "select * from assistant_actions where household_id = $1 and (not $3 or (undo is not null and undone_at is null))
         order by created_at desc limit $2",
    )
    .bind(ctx.household_id)
    .bind(super::clamp_limit(limit))
    .bind(undoable_only)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get_action(ctx: &Ctx, id: Uuid) -> Result<AssistantAction> {
    sqlx::query_as::<_, AssistantAction>(
        "select * from assistant_actions where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)
}

/// The inverse call for an action, for the caller to execute; then `mark_undone`.
pub async fn undo_spec(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<Proposal> {
    if actor.is_assistant() {
        return Err(Error::Forbidden);
    }
    let a = get_action(ctx, id).await?;
    if a.undone_at.is_some() {
        return Err(super::invalid("already undone"));
    }
    let Some(u) = a.undo else {
        return Err(super::invalid("this action cannot be undone"));
    };
    serde_json::from_value(u).map_err(|e| super::invalid(format!("bad undo spec: {e}")))
}

pub async fn mark_undone(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<AssistantAction> {
    let a = sqlx::query_as::<_, AssistantAction>(
        "update assistant_actions set undone_at = now(), undone_by = $3 where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(actor.display())
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "assistant.undo",
        "assistant_actions",
        id,
        None::<&()>,
        Some(&a),
    )
    .await?;
    Ok(a)
}

/// A pass summary the assistant leaves behind; shows in Activity.
pub async fn write_summary(ctx: &Ctx, actor: &Actor, text: String) -> Result<AssistantAction> {
    let key_id = match actor {
        Actor::Assistant { key_id, .. } => *key_id,
        _ => None,
    };
    Ok(sqlx::query_as::<_, AssistantAction>(
        "insert into assistant_actions (household_id, api_key_id, area, tool, args, summary, entity)
         values ($1, $2, 'categorizing_notes', 'assistant.summary', '{}'::jsonb, $3, 'summary') returning *",
    )
    .bind(ctx.household_id)
    .bind(key_id)
    .bind(text.trim())
    .fetch_one(&ctx.pool)
    .await?)
}

// ───────────────────────────── the work queue ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ReviewItem {
    #[serde(flatten)]
    pub transaction: Transaction,
    pub account_name: String,
    /// Categories used before for a similar description, most frequent first.
    pub suggested_category_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryShortfall {
    pub category_id: Uuid,
    pub name: String,
    pub plan_cents: Cents,
    pub bills_total_cents: Cents,
    pub short_cents: Cents,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkQueue {
    pub today: chrono::NaiveDate,
    pub month: String,
    pub autonomy: serde_json::Value,
    pub to_categorize: Vec<ReviewItem>,
    pub answered_questions: Vec<Question>,
    pub pending_proposals: Vec<Question>,
    pub open_alerts: Vec<Alert>,
    pub overdue_bills: Vec<RecurringBill>,
    pub plan_shortfalls: Vec<CategoryShortfall>,
    pub stale_accounts: Vec<String>,
    pub last_summary_at: Option<DateTime<Utc>>,
}

/// Everything waiting for the assistant, in one call.
pub async fn work(ctx: &Ctx) -> Result<WorkQueue> {
    let settings = super::settings::load(ctx).await?;
    let accounts = super::accounts::list(ctx, false).await?;
    let review = super::transactions::review_queue(ctx, Some(50)).await?;
    let mut to_categorize = Vec::with_capacity(review.len());
    for t in review {
        let suggested: Vec<(Uuid,)> = sqlx::query_as(
            "select category_id from transactions
             where household_id = $1 and category_id is not null and deleted_at is null
               and lower(description) = lower($2)
             group by category_id order by count(*) desc limit 3",
        )
        .bind(ctx.household_id)
        .bind(&t.description)
        .fetch_all(&ctx.pool)
        .await?;
        let account_name = accounts
            .iter()
            .find(|a| a.id == t.account_id)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        to_categorize.push(ReviewItem {
            account_name,
            suggested_category_ids: suggested.into_iter().map(|s| s.0).collect(),
            transaction: t,
        });
    }
    let last_summary: Option<(DateTime<Utc>,)> = sqlx::query_as(
        "select created_at from assistant_actions where household_id = $1 and tool = 'assistant.summary' order by created_at desc limit 1",
    )
    .bind(ctx.household_id)
    .fetch_optional(&ctx.pool)
    .await?;
    let since = last_summary
        .map(|s| s.0)
        .unwrap_or(ctx.now - chrono::Duration::days(7));
    let answered = sqlx::query_as::<_, Question>(
        "select * from questions where household_id = $1 and status = 'answered' and answered_at > $2 order by answered_at",
    )
    .bind(ctx.household_id)
    .bind(since)
    .fetch_all(&ctx.pool)
    .await?;
    let pending = super::questions::list(
        ctx,
        &super::questions::ListQuery {
            status: Some(crate::db::models::QuestionStatus::Open),
            kind: Some(crate::db::models::QuestionKind::Proposal),
            limit: Some(50),
        },
    )
    .await?;
    let open_alerts = super::alerts::list(
        ctx,
        &super::alerts::ListQuery {
            live: Some(true),
            limit: Some(50),
            ..Default::default()
        },
    )
    .await?;
    let bills = super::bills::all(ctx).await?;
    let core_bills: Vec<hearth_core::bills::Bill> =
        bills.iter().map(super::bills::to_core).collect();
    let overdue_ids: Vec<Uuid> =
        hearth_core::bills::overdue(&core_bills, ctx.today(), &settings.bill_match)
            .iter()
            .map(|b| b.id)
            .collect();
    let overdue_bills: Vec<RecurringBill> = bills
        .into_iter()
        .filter(|b| overdue_ids.contains(&b.id))
        .collect();
    let view = super::budget::month_view(ctx, ctx.month()).await?;
    let plan_shortfalls: Vec<CategoryShortfall> = view
        .groups
        .iter()
        .flat_map(|g| g.categories.iter())
        .filter(|c| c.bills_over_plan_cents.0 > 0)
        .map(|c| CategoryShortfall {
            category_id: c.id,
            name: c.name.clone(),
            plan_cents: c.plan_cents,
            bills_total_cents: c.bills_total_cents,
            short_cents: c.bills_over_plan_cents,
        })
        .collect();
    let stale_accounts: Vec<String> = accounts
        .iter()
        .filter(|a| a.include_in_cash)
        .filter(|a| {
            a.reported_balance_as_of
                .is_none_or(|t| (ctx.now - t).num_days() >= settings.stale_balance_days)
        })
        .map(|a| a.name.clone())
        .collect();
    Ok(WorkQueue {
        today: ctx.today(),
        month: ctx.month().to_string(),
        autonomy: serde_json::to_value(&settings.assistant).map_err(anyhow::Error::from)?,
        to_categorize,
        answered_questions: answered,
        pending_proposals: pending,
        open_alerts,
        overdue_bills,
        plan_shortfalls,
        stale_accounts,
        last_summary_at: last_summary.map(|s| s.0),
    })
}

/// Monthly API-mode spend so far.
pub async fn usage_this_month(ctx: &Ctx) -> Result<(Cents, i64)> {
    let row: (Option<Cents>, i64) = sqlx::query_as(
        "select sum(cost_cents)::bigint, count(*)::bigint from assistant_usage where household_id = $1 and at >= $2",
    )
    .bind(ctx.household_id)
    .bind(hearth_core::calendar::local_datetime_to_utc(ctx.tz, ctx.month().first_day(), 0, 0))
    .fetch_one(&ctx.pool)
    .await?;
    Ok((row.0.unwrap_or(Cents::ZERO), row.1))
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsageRecord {
    pub purpose: String,
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_cents: Cents,
}

pub async fn record_usage(ctx: &Ctx, u: UsageRecord) -> Result<()> {
    sqlx::query("insert into assistant_usage (household_id, purpose, model, input_tokens, output_tokens, cost_cents) values ($1, $2, $3, $4, $5, $6)")
        .bind(ctx.household_id)
        .bind(&u.purpose)
        .bind(&u.model)
        .bind(u.input_tokens)
        .bind(u.output_tokens)
        .bind(u.cost_cents)
        .execute(&ctx.pool)
        .await?;
    Ok(())
}
