//! Transactions: listing, manual entry, categorization, dedup, review queue,
//! and upserts from synced sources.

use chrono::{DateTime, NaiveDate, Utc};
use hearth_core::money::Cents;
use hearth_core::rules::{self as core_rules, fingerprint};
use serde::{Deserialize, Serialize};
use sqlx::{PgExecutor, QueryBuilder};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, clamp_limit, invalid};
use crate::db::models::{ReviewStatus, Transaction, TransactionSource};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListQuery {
    /// `YYYY-MM`; mutually exclusive with from/to.
    pub month: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub account_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub uncategorized: Option<bool>,
    pub review: Option<bool>,
    pub pending: Option<bool>,
    /// Case-insensitive substring on description / original description / note.
    pub q: Option<String>,
    pub include_deleted: Option<bool>,
    pub include_shadow: Option<bool>,
    pub limit: Option<i64>,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

fn encode_cursor(date: NaiveDate, id: Uuid) -> String {
    format!("{date}|{id}")
}

fn decode_cursor(s: &str) -> Result<(NaiveDate, Uuid)> {
    let (d, i) = s.split_once('|').ok_or_else(|| invalid("bad cursor"))?;
    Ok((
        d.parse().map_err(|_| invalid("bad cursor"))?,
        i.parse().map_err(|_| invalid("bad cursor"))?,
    ))
}

pub async fn list(ctx: &Ctx, q: &ListQuery) -> Result<Page<Transaction>> {
    let limit = clamp_limit(q.limit);
    let (from, to) = match (&q.month, q.from, q.to) {
        (Some(m), _, _) => {
            let m = super::parse_month(m)?;
            (Some(m.first_day()), Some(m.last_day()))
        }
        (None, f, t) => (f, t),
    };
    let mut qb: QueryBuilder<sqlx::Postgres> =
        QueryBuilder::new("select * from transactions where household_id = ");
    qb.push_bind(ctx.household_id);
    if !q.include_deleted.unwrap_or(false) {
        qb.push(" and deleted_at is null");
    }
    if !q.include_shadow.unwrap_or(false) {
        qb.push(" and shadow_of_transaction_id is null");
    }
    if let Some(f) = from {
        qb.push(" and date >= ").push_bind(f);
    }
    if let Some(t) = to {
        qb.push(" and date <= ").push_bind(t);
    }
    if let Some(a) = q.account_id {
        qb.push(" and account_id = ").push_bind(a);
    }
    if let Some(c) = q.category_id {
        qb.push(" and category_id = ").push_bind(c);
    }
    if q.uncategorized == Some(true) {
        qb.push(" and category_id is null");
    }
    if q.review == Some(true) {
        qb.push(" and review_status = 'pending'");
    }
    if let Some(p) = q.pending {
        qb.push(" and pending = ").push_bind(p);
    }
    if let Some(text) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pat = format!("%{}%", text.replace('%', "\\%"));
        qb.push(" and (description ilike ").push_bind(pat.clone());
        qb.push(" or original_description ilike ")
            .push_bind(pat.clone());
        qb.push(" or note ilike ").push_bind(pat).push(")");
    }
    if let Some(c) = &q.cursor {
        let (d, id) = decode_cursor(c)?;
        qb.push(" and (date, id) < (")
            .push_bind(d)
            .push(", ")
            .push_bind(id)
            .push(")");
    }
    qb.push(" order by date desc, id desc limit ")
        .push_bind(limit + 1);
    let mut rows: Vec<Transaction> = qb.build_query_as().fetch_all(&ctx.pool).await?;
    let next = if rows.len() as i64 > limit {
        rows.truncate(limit as usize);
        rows.last().map(|t| encode_cursor(t.date, t.id))
    } else {
        None
    };
    Ok(Page {
        items: rows,
        next_cursor: next,
    })
}

pub async fn get(ctx: &Ctx, id: Uuid) -> Result<Transaction> {
    sqlx::query_as::<_, Transaction>(
        "select * from transactions where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)
}

pub async fn review_queue(ctx: &Ctx, limit: Option<i64>) -> Result<Vec<Transaction>> {
    Ok(sqlx::query_as::<_, Transaction>(
        "select * from transactions where household_id = $1 and review_status = 'pending' and deleted_at is null
         order by date desc, created_at desc limit $2",
    )
    .bind(ctx.household_id)
    .bind(clamp_limit(limit))
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn review_stats(ctx: &Ctx) -> Result<(i64, Option<DateTime<Utc>>)> {
    let row: (i64, Option<DateTime<Utc>>) = sqlx::query_as(
        "select count(*)::bigint, min(created_at) from transactions where household_id = $1 and review_status = 'pending' and deleted_at is null",
    )
    .bind(ctx.household_id)
    .fetch_one(&ctx.pool)
    .await?;
    Ok(row)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateManual {
    pub account_id: Uuid,
    pub date: NaiveDate,
    pub description: String,
    /// Signed: negative = money out.
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
    pub note: Option<String>,
    pub pending: Option<bool>,
    pub planned_item_id: Option<Uuid>,
}

/// Insert a manual transaction. Duplicate (same account/date/amount/description) → `Conflict`.
pub async fn create_manual(ctx: &Ctx, actor: &Actor, c: CreateManual) -> Result<Transaction> {
    if c.description.trim().is_empty() {
        return Err(invalid("description is required"));
    }
    if c.amount_cents.0 == 0 {
        return Err(invalid("amount_cents cannot be zero"));
    }
    super::accounts::get(ctx, c.account_id).await?;
    if let Some(cat) = c.category_id {
        super::budget::get_category(ctx, cat).await?;
    }
    let fp = fingerprint(c.account_id, c.date, c.amount_cents, &c.description);
    let review = if c.category_id.is_some() {
        ReviewStatus::None
    } else {
        ReviewStatus::Pending
    };
    let t = sqlx::query_as::<_, Transaction>(
        "insert into transactions (household_id, account_id, date, description, original_description, amount_cents,
           category_id, source, fingerprint, pending, note, planned_item_id, review_status)
         values ($1, $2, $3, $4, $4, $5, $6, 'manual', $7, $8, $9, $10, $11) returning *",
    )
    .bind(ctx.household_id)
    .bind(c.account_id)
    .bind(c.date)
    .bind(c.description.trim())
    .bind(c.amount_cents)
    .bind(c.category_id)
    .bind(&fp)
    .bind(c.pending.unwrap_or(false))
    .bind(c.note.unwrap_or_default())
    .bind(c.planned_item_id)
    .bind(review)
    .fetch_one(&ctx.pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            Error::Conflict("an identical transaction already exists on that account/date".into())
        }
        _ => Error::Db(e),
    })?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "transaction.create",
        "transactions",
        t.id,
        None::<&()>,
        Some(&t),
    )
    .await?;
    Ok(t)
}

/// Set (or clear with `None`) the category. Resolves the review queue entry.
pub async fn categorize(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    category_id: Option<Uuid>,
    note: Option<String>,
) -> Result<Transaction> {
    let before = get(ctx, id).await?;
    if let Some(cat) = category_id {
        super::budget::get_category(ctx, cat).await?;
    }
    let after = sqlx::query_as::<_, Transaction>(
        "update transactions set category_id = $3, review_status = 'resolved', rule_id = null,
           note = coalesce($4, note), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(category_id)
    .bind(&note)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "transaction.categorize",
        "transactions",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateTransaction {
    pub description: Option<String>,
    pub note: Option<String>,
    pub date: Option<NaiveDate>,
    pub pending: Option<bool>,
}

pub async fn update(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    u: UpdateTransaction,
) -> Result<Transaction> {
    let before = get(ctx, id).await?;
    let after = sqlx::query_as::<_, Transaction>(
        "update transactions set description = coalesce($3, description), note = coalesce($4, note),
           date = coalesce($5, date), pending = coalesce($6, pending), updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(u.description.as_deref().map(str::trim))
    .bind(&u.note)
    .bind(u.date)
    .bind(u.pending)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "transaction.update",
        "transactions",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn set_note(ctx: &Ctx, actor: &Actor, id: Uuid, note: String) -> Result<Transaction> {
    update(
        ctx,
        actor,
        id,
        UpdateTransaction {
            note: Some(note),
            ..Default::default()
        },
    )
    .await
}

/// Link to a planned item (marks it done) or unlink with `None`.
pub async fn link_planned(
    ctx: &Ctx,
    actor: &Actor,
    id: Uuid,
    planned_item_id: Option<Uuid>,
) -> Result<Transaction> {
    let before = get(ctx, id).await?;
    let mut tx = ctx.pool.begin().await?;
    if let Some(old) = before.planned_item_id
        && old != planned_item_id.unwrap_or_default()
    {
        sqlx::query("update planned_items set matched_transaction_id = null, status = 'planned', updated_at = now() where id = $1 and matched_transaction_id = $2")
            .bind(old)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(p) = planned_item_id {
        let n = sqlx::query("update planned_items set matched_transaction_id = $2, status = 'done', updated_at = now() where id = $1 and household_id = $3")
            .bind(p)
            .bind(id)
            .bind(ctx.household_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if n == 0 {
            return Err(Error::NotFound);
        }
    }
    let after = sqlx::query_as::<_, Transaction>(
        "update transactions set planned_item_id = $3, updated_at = now() where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(planned_item_id)
    .fetch_one(&mut *tx)
    .await?;
    super::audit::record(
        &mut *tx,
        ctx.household_id,
        actor,
        "transaction.link_planned",
        "transactions",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}

pub async fn soft_delete(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<Transaction> {
    let before = get(ctx, id).await?;
    let after = sqlx::query_as::<_, Transaction>(
        "update transactions set deleted_at = now(), updated_at = now() where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "transaction.delete",
        "transactions",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

/// Outcome of applying rules to one transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleOutcome {
    Categorized,
    Ignored,
    Unmatched,
    Skipped,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplyRulesSummary {
    pub examined: usize,
    pub categorized: usize,
    pub ignored: usize,
    pub unmatched: usize,
}

/// Apply the household's rules to uncategorized, non-deleted transactions
/// (or to every transaction when `force`). Unmatched rows enter the review queue.
pub async fn apply_rules(
    ctx: &Ctx,
    actor: &Actor,
    force: bool,
    only_ids: Option<&[Uuid]>,
) -> Result<ApplyRulesSummary> {
    let rules = super::rules::core_rules(ctx).await?;
    let targets: Vec<Transaction> = match only_ids {
        Some(ids) => {
            sqlx::query_as::<_, Transaction>("select * from transactions where household_id = $1 and id = any($2) and deleted_at is null")
                .bind(ctx.household_id)
                .bind(ids)
                .fetch_all(&ctx.pool)
                .await?
        }
        None => {
            sqlx::query_as::<_, Transaction>(
                "select * from transactions where household_id = $1 and deleted_at is null and ($2 or (category_id is null and review_status <> 'resolved'))",
            )
            .bind(ctx.household_id)
            .bind(force)
            .fetch_all(&ctx.pool)
            .await?
        }
    };
    let mut summary = ApplyRulesSummary::default();
    for t in &targets {
        // Never override a human/AI decision unless forced.
        if !force
            && (t.review_status == ReviewStatus::Resolved
                || (t.category_id.is_some() && t.rule_id.is_none()))
        {
            continue;
        }
        summary.examined += 1;
        match apply_rules_to(&ctx.pool, &rules, t).await? {
            RuleOutcome::Categorized => summary.categorized += 1,
            RuleOutcome::Ignored => summary.ignored += 1,
            RuleOutcome::Unmatched => summary.unmatched += 1,
            RuleOutcome::Skipped => {}
        }
    }
    if summary.examined > 0 {
        super::audit::record(
            &ctx.pool,
            ctx.household_id,
            actor,
            "transactions.apply_rules",
            "transactions",
            "*",
            None::<&()>,
            Some(&summary),
        )
        .await?;
    }
    Ok(summary)
}

/// Apply rules to one row. Used by `apply_rules` and by the sync upsert path.
pub async fn apply_rules_to<'e>(
    exec: impl PgExecutor<'e> + Copy,
    rules: &[(core_rules::Rule, Option<String>)],
    t: &Transaction,
) -> Result<RuleOutcome> {
    let txn = core_rules::TxnForRules {
        account_id: t.account_id,
        description: t.description.clone(),
        amount_cents: t.amount_cents,
    };
    let core_only: Vec<core_rules::Rule> = rules.iter().map(|(r, _)| r.clone()).collect();
    let Some(hit) = core_rules::first_match(&core_only, &txn) else {
        sqlx::query("update transactions set review_status = 'pending', updated_at = now() where id = $1 and category_id is null")
            .bind(t.id)
            .execute(exec)
            .await?;
        return Ok(RuleOutcome::Unmatched);
    };
    let set_note = rules
        .iter()
        .find(|(r, _)| r.id == hit.id)
        .and_then(|(_, n)| n.clone());
    let outcome = match hit.action {
        core_rules::RuleAction::Categorize => RuleOutcome::Categorized,
        core_rules::RuleAction::Ignore => RuleOutcome::Ignored,
    };
    sqlx::query(
        "update transactions set category_id = $2, rule_id = $3, review_status = 'none',
           note = case when $4::text is null or $4 = '' or position($4 in note) > 0 then note
                       when note = '' then $4 else note || ' ' || $4 end,
           updated_at = now()
         where id = $1",
    )
    .bind(t.id)
    .bind(hit.category_id)
    .bind(hit.id)
    .bind(&set_note)
    .execute(exec)
    .await?;
    sqlx::query("update rules set hit_count = hit_count + 1, last_hit_at = now() where id = $1")
        .bind(hit.id)
        .execute(exec)
        .await?;
    Ok(outcome)
}

/// A transaction arriving from a synced source.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub account_id: Uuid,
    pub source: TransactionSource,
    pub external_id: String,
    /// For Plaid: the pending transaction this posted row replaces.
    pub replaces_external_id: Option<String>,
    pub date: NaiveDate,
    pub description: String,
    pub original_description: String,
    pub amount_cents: Cents,
    pub pending: bool,
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpsertOutcome {
    Inserted,
    Updated,
    Unchanged,
}

/// Insert or update a synced transaction by `(source, external_id)`.
/// Category, note, planned link and review state survive updates.
pub async fn upsert_incoming(ctx: &Ctx, inc: &Incoming) -> Result<(Transaction, UpsertOutcome)> {
    // 1. Pending → posted replacement (Plaid): move the old row to the new id.
    if let Some(old_ext) = &inc.replaces_external_id
        && let Some(existing) = sqlx::query_as::<_, Transaction>(
            "select * from transactions where source = $1 and external_id = $2",
        )
        .bind(inc.source)
        .bind(old_ext)
        .fetch_optional(&ctx.pool)
        .await?
    {
        let t = sqlx::query_as::<_, Transaction>(
            "update transactions set external_id = $2, date = $3, description = case when description = original_description then $4 else description end,
               original_description = $5, amount_cents = $6, pending = $7, raw = $8, deleted_at = null, updated_at = now()
             where id = $1 returning *",
        )
        .bind(existing.id)
        .bind(&inc.external_id)
        .bind(inc.date)
        .bind(&inc.description)
        .bind(&inc.original_description)
        .bind(inc.amount_cents)
        .bind(inc.pending)
        .bind(&inc.raw)
        .fetch_one(&ctx.pool)
        .await?;
        return Ok((t, UpsertOutcome::Updated));
    }
    // 2. Existing row with the same id: update the provider-owned fields.
    if let Some(existing) = sqlx::query_as::<_, Transaction>(
        "select * from transactions where source = $1 and external_id = $2",
    )
    .bind(inc.source)
    .bind(&inc.external_id)
    .fetch_optional(&ctx.pool)
    .await?
    {
        let unchanged = existing.date == inc.date
            && existing.amount_cents == inc.amount_cents
            && existing.pending == inc.pending
            && existing.original_description == inc.original_description
            && existing.deleted_at.is_none();
        if unchanged {
            return Ok((existing, UpsertOutcome::Unchanged));
        }
        let t = sqlx::query_as::<_, Transaction>(
            "update transactions set date = $2, description = case when description = original_description then $3 else description end,
               original_description = $4, amount_cents = $5, pending = $6, raw = $7, deleted_at = null, updated_at = now()
             where id = $1 returning *",
        )
        .bind(existing.id)
        .bind(inc.date)
        .bind(&inc.description)
        .bind(&inc.original_description)
        .bind(inc.amount_cents)
        .bind(inc.pending)
        .bind(&inc.raw)
        .fetch_one(&ctx.pool)
        .await?;
        return Ok((t, UpsertOutcome::Updated));
    }
    // 3. New row.
    let t = sqlx::query_as::<_, Transaction>(
        "insert into transactions (household_id, account_id, date, description, original_description, amount_cents, source, external_id, pending, raw, review_status)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'pending') returning *",
    )
    .bind(ctx.household_id)
    .bind(inc.account_id)
    .bind(inc.date)
    .bind(&inc.description)
    .bind(&inc.original_description)
    .bind(inc.amount_cents)
    .bind(inc.source)
    .bind(&inc.external_id)
    .bind(inc.pending)
    .bind(&inc.raw)
    .fetch_one(&ctx.pool)
    .await?;
    Ok((t, UpsertOutcome::Inserted))
}

/// Soft-delete a synced row the provider removed.
pub async fn remove_external(
    ctx: &Ctx,
    source: TransactionSource,
    external_id: &str,
) -> Result<bool> {
    let n = sqlx::query("update transactions set deleted_at = now(), updated_at = now() where household_id = $1 and source = $2 and external_id = $3 and deleted_at is null")
        .bind(ctx.household_id)
        .bind(source)
        .bind(external_id)
        .execute(&ctx.pool)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// Insert a CSV row; returns `None` when the fingerprint already exists.
pub async fn insert_csv_row(
    ctx: &Ctx,
    account_id: Uuid,
    batch_id: Uuid,
    date: NaiveDate,
    description: &str,
    amount: Cents,
    external_id: Option<&str>,
) -> Result<Option<Transaction>> {
    let fp = fingerprint(account_id, date, amount, description);
    let t = sqlx::query_as::<_, Transaction>(
        "insert into transactions (household_id, account_id, date, description, original_description, amount_cents, source, external_id, fingerprint, import_batch_id, review_status)
         values ($1, $2, $3, $4, $4, $5, 'csv', $6, $7, $8, 'pending')
         on conflict do nothing returning *",
    )
    .bind(ctx.household_id)
    .bind(account_id)
    .bind(date)
    .bind(description.trim())
    .bind(amount)
    .bind(external_id)
    .bind(&fp)
    .bind(batch_id)
    .fetch_optional(&ctx.pool)
    .await?;
    Ok(t)
}

/// Link a bank-feed row as the shadow of a card row (OPEN-QUESTIONS Q3).
pub async fn set_shadow(ctx: &Ctx, bank_row: Uuid, card_row: Option<Uuid>) -> Result<()> {
    sqlx::query("update transactions set shadow_of_transaction_id = $3, review_status = case when $3 is null then review_status else 'none' end, updated_at = now() where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(bank_row)
        .bind(card_row)
        .execute(&ctx.pool)
        .await?;
    Ok(())
}

/// Record that a transaction pays a recurring bill (set by the process job or a person).
pub async fn set_bill_match(
    ctx: &Ctx,
    id: Uuid,
    bill_id: Option<Uuid>,
    drift: Option<Cents>,
    category_id: Option<Uuid>,
) -> Result<Transaction> {
    Ok(sqlx::query_as::<_, Transaction>(
        "update transactions set recurring_bill_id = $3, bill_drift_cents = $4,
           category_id = coalesce(category_id, $5),
           review_status = case when $3 is null then review_status else 'none' end, updated_at = now()
         where household_id = $1 and id = $2 returning *",
    )
    .bind(ctx.household_id)
    .bind(id)
    .bind(bill_id)
    .bind(drift)
    .bind(category_id)
    .fetch_one(&ctx.pool)
    .await?)
}
