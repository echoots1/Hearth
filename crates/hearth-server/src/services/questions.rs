use serde::Deserialize;
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Question, QuestionStatus};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListQuery {
    pub status: Option<QuestionStatus>,
    pub limit: Option<i64>,
}

pub async fn list(ctx: &Ctx, q: &ListQuery) -> Result<Vec<Question>> {
    Ok(sqlx::query_as::<_, Question>(
        "select * from questions where household_id = $1 and ($2::question_status is null or status = $2) order by created_at desc limit $3",
    )
    .bind(ctx.household_id)
    .bind(q.status)
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
        "insert into questions (household_id, asked_by, question, options, transaction_id, alert_id, planned_item_id, context, notify)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9) returning *",
    )
    .bind(ctx.household_id)
    .bind(actor.display())
    .bind(a.question.trim())
    .bind(&a.options)
    .bind(a.transaction_id)
    .bind(a.alert_id)
    .bind(a.planned_item_id)
    .bind(&a.context)
    .bind(a.notify.unwrap_or(true))
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "question.ask", "questions", q.id, None::<&()>, Some(&q)).await?;
    Ok(q)
}

pub async fn answer(ctx: &Ctx, actor: &Actor, id: Uuid, answer: String) -> Result<Question> {
    if answer.trim().is_empty() {
        return Err(invalid("answer is required"));
    }
    let before = get(ctx, id).await?;
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
    super::audit::record(&ctx.pool, ctx.household_id, actor, "question.answer", "questions", id, Some(&before), Some(&after)).await?;
    Ok(after)
}

pub async fn dismiss(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<Question> {
    let before = get(ctx, id).await?;
    let after = sqlx::query_as::<_, Question>("update questions set status = 'dismissed', updated_at = now() where household_id = $1 and id = $2 returning *")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_one(&ctx.pool)
        .await?;
    super::audit::record(&ctx.pool, ctx.household_id, actor, "question.dismiss", "questions", id, Some(&before), Some(&after)).await?;
    Ok(after)
}

pub async fn open_count(ctx: &Ctx) -> Result<i64> {
    let (n,): (i64,) = sqlx::query_as("select count(*)::bigint from questions where household_id = $1 and status = 'open'")
        .bind(ctx.household_id)
        .fetch_one(&ctx.pool)
        .await?;
    Ok(n)
}
