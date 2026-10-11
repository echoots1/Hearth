//! Questions the assistant asks, and proposals it makes in Backseat mode.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{AutonomyArea, Question, QuestionKind, QuestionStatus};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListQuery {
    pub status: Option<QuestionStatus>,
    pub kind: Option<QuestionKind>,
    pub limit: Option<i64>,
}

pub async fn list(ctx: &Ctx, q: &ListQuery) -> Result<Vec<Question>> {
    Ok(sqlx::query_as::<_, Question>(
        "select * from questions where household_id = $1
           and ($2::question_status is null or status = $2)
           and ($3::question_kind is null or kind = $3)
         order by created_at desc limit $4",
    )
    .bind(ctx.household_id)
    .bind(q.status)
    .bind(q.kind)
    .bind(super::clamp_limit(q.limit))
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<Question> {
    sqlx::query_as::<_, Question>("select * from questions where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

#[derive(Debug, Clone, Deserialize)]
pub struct AskQuestion {
    pub question: String,
    /// `[{ "key": "groceries", "label": "Groceries" }]`
    pub options: Option<serde_json::Value>,
    pub transaction_id: Option<Uuid>,
    pub alert_id: Option<Uuid>,
    pub planned_item_id: Option<Uuid>,
    pub bill_id: Option<Uuid>,
    pub context: Option<serde_json::Value>,
    pub notify: Option<bool>,
}

pub async fn ask(ctx: &Ctx, actor: &Actor, a: AskQuestion) -> Result<Question> {
    if a.question.trim().is_empty() {
        return Err(invalid("question is required"));
    }
    if let Some(o) = &a.options
        && !o.is_array()
    {
        return Err(invalid("options must be an array"));
    }
    let q = sqlx::query_as::<_, Question>(
        "insert into questions (household_id, kind, asked_by, question, options, transaction_id, alert_id, planned_item_id, bill_id, context, notify)
         values ($1, 'ask', $2, $3, $4, $5, $6, $7, $8, $9, $10) returning *",
    )
    .bind(ctx.household_id)
    .bind(actor.display())
    .bind(a.question.trim())
    .bind(&a.options)
    .bind(a.transaction_id)
    .bind(a.alert_id)
    .bind(a.planned_item_id)
    .bind(a.bill_id)
    .bind(&a.context)
    .bind(a.notify.unwrap_or(true))
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "question.ask",
        "questions",
        q.id,
        None::<&()>,
        Some(&q),
    )
    .await?;
    Ok(q)
}

/// A structured change the assistant wants to make, awaiting approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub tool: String,
    pub args: serde_json::Value,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ProposeInput {
    pub area: AutonomyArea,
    pub proposal: Proposal,
    /// Human-readable question, e.g. "Raise the Groceries plan to $900?"
    pub question: String,
    pub transaction_id: Option<Uuid>,
    pub bill_id: Option<Uuid>,
    pub context: Option<serde_json::Value>,
}

pub async fn propose(ctx: &Ctx, actor: &Actor, p: ProposeInput) -> Result<Question> {
    let proposal = serde_json::to_value(&p.proposal).map_err(anyhow::Error::from)?;
    let q = sqlx::query_as::<_, Question>(
        "insert into questions (household_id, kind, asked_by, question, proposal, area, transaction_id, bill_id, context)
         values ($1, 'proposal', $2, $3, $4, $5, $6, $7, $8) returning *",
    )
    .bind(ctx.household_id)
    .bind(actor.display())
    .bind(p.question.trim())
    .bind(&proposal)
    .bind(p.area)
    .bind(p.transaction_id)
    .bind(p.bill_id)
    .bind(&p.context)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "question.propose",
        "questions",
        q.id,
        None::<&()>,
        Some(&q),
    )
    .await?;
    Ok(q)
}

pub async fn answer(ctx: &Ctx, actor: &Actor, id: Uuid, answer: String) -> Result<Question> {
    if answer.trim().is_empty() {
        return Err(invalid("answer is required"));
    }
    let before = get(ctx, id).await?;
    if before.kind != QuestionKind::Ask {
        return Err(invalid("proposals are approved or declined, not answered"));
    }
    let after = sqlx::query_as::<_, Question>(
        "update questions set status = 'answered', answer = $3, answered_by = $4, answered_at = now(), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(answer.trim())
    .bind(actor.display())
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "question.answer",
        "questions",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

/// The proposal a person is approving, possibly with edited arguments.
/// The caller executes `proposal.tool` with `proposal.args` and then calls `mark_applied`.
pub async fn approve(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    edited_args: Option<serde_json::Value>,
) -> Result<(Question, Proposal)> {
    if actor.is_assistant() {
        return Err(Error::Forbidden);
    }
    let before = get(ctx, id).await?;
    if before.kind != QuestionKind::Proposal || before.status != QuestionStatus::Open {
        return Err(invalid("not an open proposal"));
    }
    let mut proposal: Proposal =
        serde_json::from_value(before.proposal.clone().unwrap_or_default())
            .map_err(|e| invalid(format!("bad proposal: {e}")))?;
    if let Some(args) = edited_args {
        proposal.args = args;
    }
    let after = sqlx::query_as::<_, Question>(
        "update questions set status = 'approved', answered_by = $3, answered_at = now(), proposal = $4, updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(actor.display())
    .bind(serde_json::to_value(&proposal).map_err(anyhow::Error::from)?)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "question.approve",
        "questions",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok((after, proposal))
}

pub async fn mark_applied(ctx: &Ctx, id: Uuid) -> Result<Question> {
    Ok(sqlx::query_as::<_, Question>("update questions set applied_at = now(), updated_at = now() where household_id = $1 and id = $2 returning *")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_one(&ctx.pool)
        .await?)
}

pub async fn decline(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    reason: Option<String>,
) -> Result<Question> {
    if actor.is_assistant() {
        return Err(Error::Forbidden);
    }
    let before = get(ctx, id).await?;
    if before.kind != QuestionKind::Proposal {
        return Err(invalid(
            "only proposals can be declined; dismiss a question instead",
        ));
    }
    let after = sqlx::query_as::<_, Question>(
        "update questions set status = 'declined', answer = $3, answered_by = $4, answered_at = now(), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(reason)
    .bind(actor.display())
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "question.decline",
        "questions",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn dismiss(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<Question> {
    let before = get(ctx, id).await?;
    let after = sqlx::query_as::<_, Question>("update questions set status = 'dismissed', updated_at = now() where household_id = $1 and id = $2 returning *")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_one(&ctx.pool)
        .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "question.dismiss",
        "questions",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn open_count(ctx: &Ctx) -> Result<(i64, i64)> {
    let row: (i64, i64) = sqlx::query_as(
        "select count(*) filter (where kind = 'ask')::bigint, count(*) filter (where kind = 'proposal')::bigint
         from questions where household_id = $1 and status = 'open'",
    )
    .bind(ctx.household_id)
    .fetch_one(&ctx.pool)
    .await?;
    Ok(row)
}
