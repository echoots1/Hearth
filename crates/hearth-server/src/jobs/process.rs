//! `process`: shadow links, rules, bill matching, planned-item matching.
//! Never guesses a category.

use chrono::Duration;
use hearth_core::bills::{self as core_bills, TxnForBills};
use serde_json::{Value, json};
use uuid::Uuid;

use super::actor;
use crate::api::AppState;
use crate::db::models::Transaction;
use crate::services::{self, Ctx};

pub async fn run(state: AppState) -> anyhow::Result<Value> {
    let ctx = state.ctx().await?;
    let actor = actor("process");
    let settings = services::settings::load(&ctx).await?;

    // Pair first, so the bank's line for a card charge never reaches the
    // rules or the review queue.
    let shadowed = shadow_links(&ctx).await?;
    let rules = services::transactions::apply_rules(&ctx, &actor, false, None).await?;
    let bills_matched = match_bills(&ctx, &settings).await?;
    let planned_matched = match_planned(&ctx, &settings).await?;

    Ok(json!({
        "rules": { "examined": rules.examined, "categorized": rules.categorized, "ignored": rules.ignored, "unmatched": rules.unmatched },
        "bills_matched": bills_matched,
        "planned_matched": planned_matched,
        "shadow_linked": shadowed,
    }))
}

/// Outflows from the last few weeks that are not yet tied to a bill.
async fn match_bills(
    ctx: &Ctx,
    settings: &hearth_core::settings::Settings,
) -> anyhow::Result<usize> {
    let bills = services::bills::all(ctx).await?;
    if bills.is_empty() {
        return Ok(0);
    }
    let core: Vec<core_bills::Bill> = bills.iter().map(services::bills::to_core).collect();
    let since = ctx.today() - Duration::days(settings.bill_match.days + 14);
    let rows = sqlx::query_as::<_, Transaction>(
        "select * from transactions where household_id = $1 and deleted_at is null and shadow_of_transaction_id is null
           and recurring_bill_id is null and amount_cents < 0 and date >= $2 order by date",
    )
    .bind(ctx.household_id)
    .bind(since)
    .fetch_all(&ctx.pool)
    .await?;
    let mut n = 0;
    for t in rows {
        let txn = TxnForBills {
            date: t.date,
            description: t.description.clone(),
            amount_cents: t.amount_cents,
            account_id: t.account_id,
        };
        let Some(hit) = core_bills::find_match(&txn, &core, &settings.bill_match) else {
            continue;
        };
        let bill_id = hit.bill.id;
        let drift = hit.drift_cents;
        let category = bills
            .iter()
            .find(|b| b.id == bill_id)
            .and_then(|b| b.category_id);
        services::transactions::set_bill_match(ctx, t.id, Some(bill_id), Some(drift), category)
            .await?;
        services::bills::record_match(ctx, bill_id, t.id, t.date).await?;
        n += 1;
    }
    Ok(n)
}

async fn match_planned(
    ctx: &Ctx,
    settings: &hearth_core::settings::Settings,
) -> anyhow::Result<usize> {
    let since = ctx.today() - Duration::days(settings.planned_match.days + 7);
    let rows = sqlx::query_as::<_, Transaction>(
        "select * from transactions where household_id = $1 and deleted_at is null and shadow_of_transaction_id is null
           and planned_item_id is null and recurring_bill_id is null and amount_cents < 0 and date >= $2 order by date",
    )
    .bind(ctx.household_id)
    .bind(since)
    .fetch_all(&ctx.pool)
    .await?;
    let mut n = 0;
    for t in rows {
        if services::planned::try_match(ctx, &t, &settings.planned_match)
            .await?
            .is_some()
        {
            n += 1;
        }
    }
    Ok(n)
}

/// The bank's line for a Privacy.com charge duplicates the card row: Privacy
/// posted the charge to the funding account at once; the bank stream shows it
/// up to a week later as `PwP*MERCHANT` for the same amount. Link the bank row as
/// the card row's shadow so the budget and the cash estimate count the charge
/// once (the card row, which arrived first and carries the category).
async fn shadow_links(ctx: &Ctx) -> anyhow::Result<usize> {
    let pairs: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "select bank.id, card.id from transactions bank
           join transactions card on card.household_id = bank.household_id
             and card.account_id = bank.account_id
             and card.source = 'privacy'
             and card.amount_cents = bank.amount_cents
             and card.date <= bank.date and bank.date - card.date <= 7
             and card.deleted_at is null and card.shadow_of_transaction_id is null
         where bank.household_id = $1 and bank.deleted_at is null and bank.shadow_of_transaction_id is null
           and bank.source <> 'privacy' and bank.amount_cents < 0
           and (bank.original_description ~* '^\\s*pwp' or bank.description ~* '^\\s*pwp')
           and bank.date >= current_date - 30
           and not exists (select 1 from transactions x where x.shadow_of_transaction_id = card.id)
         order by bank.date, bank.id, card.date desc, card.id",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?;
    // One card row per bank line and one bank line per card row.
    let mut used_bank = std::collections::HashSet::new();
    let mut used_card = std::collections::HashSet::new();
    let mut n = 0;
    for (bank, card) in pairs {
        if used_bank.contains(&bank) || used_card.contains(&card) {
            continue;
        }
        services::transactions::set_shadow(ctx, bank, Some(card)).await?;
        used_bank.insert(bank);
        used_card.insert(card);
        n += 1;
    }
    Ok(n)
}
