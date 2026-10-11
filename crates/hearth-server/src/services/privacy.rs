//! Privacy.com: card charges are transactions on the **funding account** (the
//! bank account Privacy draws from), pending until settled; declines are card
//! events (alerts, not transactions). Cards are not accounts: the bank's own
//! `PwP*…` line for each charge arrives a few days later and the process job
//! pairs it with the card row (see `jobs::process::shadow_links`).

use chrono::Duration;
use serde::Serialize;
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Account, TransactionSource};
use crate::integrations::privacy::{self, Privacy};

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncSummary {
    pub cards: usize,
    pub inserted: usize,
    pub updated: usize,
    pub declines: usize,
    pub voided: usize,
    /// $0 charges (card verifications), not stored.
    pub ignored: usize,
    /// Rows and declines moved here from card accounts of earlier versions.
    pub rehomed: usize,
}

/// The account card charges land on: the configured one, else the household's
/// only active checking account. Anything else is a setup error.
pub async fn funding_account(ctx: &Ctx, configured: Option<Uuid>) -> Result<Account> {
    if let Some(id) = configured {
        let a = super::accounts::get(ctx, id).await.map_err(|e| match e {
            Error::NotFound => invalid(
                "the Privacy.com funding account no longer exists: pick another under More → Integrations",
            ),
            other => other,
        })?;
        if a.source == crate::db::models::AccountSource::Privacy {
            return Err(invalid(
                "the Privacy.com funding account must be a bank account, not a card",
            ));
        }
        return Ok(a);
    }
    let candidates: Vec<Account> = sqlx::query_as(
        "select * from accounts where household_id = $1 and is_active and type = 'checking' and source <> 'privacy' order by name",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?;
    match candidates.len() {
        1 => Ok(candidates.into_iter().next().expect("one")),
        0 => Err(invalid(
            "Privacy.com needs a funding account: add the checking account the cards draw from, then pick it under More → Integrations",
        )),
        _ => Err(invalid(
            "Privacy.com needs a funding account: pick the checking account the cards draw from under More → Integrations",
        )),
    }
}

fn card_label(c: &privacy::Card) -> String {
    let closed = if c.state == "CLOSED" { ", closed" } else { "" };
    if c.memo.trim().is_empty() {
        format!("Privacy card ····{}{closed}", c.last_four)
    } else {
        format!("{} (····{}{closed})", c.memo.trim(), c.last_four)
    }
}

/// Earlier versions made every card an account. Move their rows to the
/// funding account (also when the funding account changed) and remove the
/// empty card accounts, so nothing asks for a "balance" of a virtual card.
async fn rehome(ctx: &Ctx, actor: &Actor, funding: &Account) -> Result<usize> {
    let moved_txns = sqlx::query(
        "update transactions set account_id = $2, updated_at = now()
         where household_id = $1 and source = 'privacy' and account_id <> $2",
    )
    .bind(ctx.household_id)
    .bind(funding.id)
    .execute(&ctx.pool)
    .await?
    .rows_affected();
    let moved_events = sqlx::query(
        "update card_events e set account_id = $2, card_label = coalesce(e.card_label, a.name)
         from accounts a where a.id = e.account_id and a.household_id = $1 and e.account_id <> $2",
    )
    .bind(ctx.household_id)
    .bind(funding.id)
    .execute(&ctx.pool)
    .await?
    .rows_affected();
    // Anything else that pointed at a card account points at the funding
    // account now, so removing the card account loses nothing.
    for sql in [
        "update rules x set account_id = $2, updated_at = now() from accounts a
          where a.id = x.account_id and a.household_id = $1 and a.source = 'privacy' and x.account_id <> $2",
        "update import_batches x set account_id = $2 from accounts a
          where a.id = x.account_id and a.household_id = $1 and a.source = 'privacy' and x.account_id <> $2",
        "update recurring_bills x set account_id = $2, updated_at = now() from accounts a
          where a.id = x.account_id and a.household_id = $1 and a.source = 'privacy' and x.account_id <> $2",
        "update alerts x set account_id = $2, updated_at = now() from accounts a
          where a.id = x.account_id and a.household_id = $1 and a.source = 'privacy' and x.account_id <> $2",
    ] {
        sqlx::query(sql)
            .bind(ctx.household_id)
            .bind(funding.id)
            .execute(&ctx.pool)
            .await?;
    }
    let stale: Vec<Account> = sqlx::query_as(
        "select * from accounts where household_id = $1 and source = 'privacy'
           and not exists (select 1 from transactions t where t.account_id = accounts.id)
           and not exists (select 1 from card_events e where e.account_id = accounts.id)",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?;
    for a in stale {
        sqlx::query("delete from accounts where id = $1")
            .bind(a.id)
            .execute(&ctx.pool)
            .await?;
        super::audit::record(
            &ctx.pool,
            ctx.household_id,
            actor,
            "account.delete",
            "accounts",
            a.id,
            Some(&a),
            None::<&()>,
        )
        .await?;
    }
    Ok((moved_txns + moved_events) as usize)
}

pub async fn sync(
    ctx: &Ctx,
    actor: &Actor,
    client: &Privacy,
    funding_account_id: Option<Uuid>,
) -> Result<SyncSummary> {
    let mut s = SyncSummary::default();
    let funding = funding_account(ctx, funding_account_id).await?;
    s.rehomed = rehome(ctx, actor, &funding).await?;

    let cards = client
        .cards()
        .await
        .map_err(|e| Error::Other(anyhow::anyhow!(e)))?;
    let labels: std::collections::HashMap<&str, String> = cards
        .iter()
        .map(|c| (c.token.as_str(), card_label(c)))
        .collect();
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
        let label = labels
            .get(t.card.token.as_str())
            .cloned()
            .unwrap_or_else(|| "Privacy card".to_string());
        let declined = t.status == "DECLINED" || (t.result != "APPROVED" && !t.result.is_empty());
        if declined {
            let n = sqlx::query(
                "insert into card_events (account_id, external_id, occurred_at, amount_cents, descriptor, result, reason, card_token, card_label)
                 values ($1, $2, $3, $4, $5, 'declined', $6, $7, $8) on conflict (external_id) do nothing",
            )
            .bind(funding.id)
            .bind(&t.token)
            .bind(t.created)
            .bind(t.amount)
            .bind(&t.merchant.descriptor)
            .bind(if t.result == "DECLINED" { None } else { Some(t.result.clone()) })
            .bind(&t.card.token)
            .bind(&label)
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
            account_id: funding.id,
            source: TransactionSource::Privacy,
            external_id: t.token.clone(),
            replaces_external_id: None,
            date: hearth_core::calendar::local_date(ctx.tz, t.created),
            description: t.merchant.descriptor.clone(),
            original_description: t.merchant.descriptor.clone(),
            amount_cents: hearth_core::money::Cents(-amount),
            pending: t.status != "SETTLED",
            // Which card, for people and the assistant; never shown in the API.
            raw: Some(serde_json::json!({
                "card_token": t.card.token,
                "card_label": label,
                "status": t.status,
            })),
        };
        match super::transactions::upsert_incoming(ctx, &inc).await? {
            super::transactions::UpsertOutcome::Inserted => s.inserted += 1,
            super::transactions::UpsertOutcome::Updated => s.updated += 1,
            super::transactions::UpsertOutcome::Ignored => s.ignored += 1,
            super::transactions::UpsertOutcome::Unchanged => {}
        }
    }
    Ok(s)
}
