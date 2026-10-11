//! Privacy.com: each card is an account; charges are transactions; declines
//! are card events (alerts, not transactions).

use chrono::Duration;
use serde::Serialize;

use super::{Actor, Ctx, Error, Result};
use crate::db::models::{Account, AccountSource, AccountType, TransactionSource};
use crate::integrations::privacy::{self, Privacy};

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncSummary {
    pub cards: usize,
    pub inserted: usize,
    pub updated: usize,
    pub declines: usize,
    pub voided: usize,
    pub skipped: usize,
}

async fn card_account(ctx: &Ctx, actor: &Actor, c: &privacy::Card) -> Result<Account> {
    if let Some(a) = sqlx::query_as::<_, Account>(
        "select * from accounts where household_id = $1 and source = 'privacy' and external_id = $2",
    )
    .bind(ctx.household_id)
    .bind(&c.token)
    .fetch_optional(&ctx.pool)
    .await?
    {
        let active = c.state != "CLOSED";
        if a.is_active != active {
            sqlx::query("update accounts set is_active = $2, updated_at = now() where id = $1")
                .bind(a.id)
                .bind(active)
                .execute(&ctx.pool)
                .await?;
        }
        return Ok(a);
    }
    let name = if c.memo.trim().is_empty() {
        format!("Privacy card ····{}", c.last_four)
    } else {
        format!("{} (Privacy ····{})", c.memo.trim(), c.last_four)
    };
    super::accounts::create(
        ctx,
        actor,
        super::accounts::CreateAccount {
            name,
            account_type: AccountType::VirtualCard,
            source: Some(AccountSource::Privacy),
            external_id: Some(c.token.clone()),
            plaid_item_id: None,
            institution_label: Some("Privacy.com".into()),
            mask: Some(c.last_four.clone()),
            include_in_cash: Some(false),
        },
    )
    .await
}

pub async fn sync(ctx: &Ctx, actor: &Actor, client: &Privacy) -> Result<SyncSummary> {
    let mut s = SyncSummary::default();
    let cards = client
        .cards()
        .await
        .map_err(|e| Error::Other(anyhow::anyhow!(e)))?;
    let mut by_token = std::collections::HashMap::new();
    for c in &cards {
        let a = card_account(ctx, actor, c).await?;
        by_token.insert(c.token.clone(), a.id);
    }
    s.cards = cards.len();

    // Overlap the previous window so late settlements are picked up.
    let last_ok: Option<(chrono::DateTime<chrono::Utc>,)> = sqlx::query_as(
        "select started_at from job_runs where job = 'sync:privacy' and status = 'ok' order by started_at desc limit 1",
    )
    .fetch_optional(&ctx.pool)
    .await?;
    let begin = match last_ok {
        Some((t,)) => (t - Duration::days(5)).date_naive(),
        None => ctx.today() - Duration::days(45),
    };
    let txns = client
        .transactions(&begin.to_string())
        .await
        .map_err(|e| Error::Other(anyhow::anyhow!(e)))?;
    for t in txns {
        let Some(&account_id) = by_token.get(&t.card.token) else {
            s.skipped += 1;
            continue;
        };
        let declined = t.status == "DECLINED" || (t.result != "APPROVED" && !t.result.is_empty());
        if declined {
            let n = sqlx::query(
                "insert into card_events (account_id, external_id, occurred_at, amount_cents, descriptor, result, reason)
                 values ($1, $2, $3, $4, $5, 'declined', $6) on conflict (external_id) do nothing",
            )
            .bind(account_id)
            .bind(&t.token)
            .bind(t.created)
            .bind(t.amount)
            .bind(&t.merchant.descriptor)
            .bind(if t.result == "DECLINED" { None } else { Some(t.result.clone()) })
            .execute(&ctx.pool)
            .await?
            .rows_affected();
            s.declines += n as usize;
            continue;
        }
        if t.status == "VOIDED" || t.status == "BOUNCED" {
            if super::transactions::remove_external(ctx, TransactionSource::Privacy, &t.token)
                .await?
            {
                s.voided += 1;
            }
            continue;
        }
        let amount = t
            .settled_amount
            .filter(|_| t.status == "SETTLED")
            .unwrap_or(t.amount);
        let inc = super::transactions::Incoming {
            account_id,
            source: TransactionSource::Privacy,
            external_id: t.token.clone(),
            replaces_external_id: None,
            date: hearth_core::calendar::local_date(ctx.tz, t.created),
            description: t.merchant.descriptor.clone(),
            original_description: t.merchant.descriptor.clone(),
            amount_cents: hearth_core::money::Cents(-amount),
            pending: t.status != "SETTLED",
            raw: None,
        };
        let (_, outcome) = super::transactions::upsert_incoming(ctx, &inc).await?;
        match outcome {
            super::transactions::UpsertOutcome::Inserted => s.inserted += 1,
            super::transactions::UpsertOutcome::Updated => s.updated += 1,
            super::transactions::UpsertOutcome::Unchanged => {}
        }
    }
    Ok(s)
}
