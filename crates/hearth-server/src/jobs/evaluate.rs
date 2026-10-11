//! `evaluate`: assemble the snapshot, run the deterministic evaluator, upsert
//! alerts by dedupe key, and note milestones. Never acks or resolves.

use chrono::{DateTime, Duration, Utc};
use hearth_core::alerts::{
    self, AccountSnapshot, DebtSnapshot, DeclineSnapshot, GoalSnapshot, PlannedSnapshot, Snapshot,
    SourceHealth, TxnSnapshot,
};
use hearth_core::calendar::Month;
use hearth_core::money::Cents;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api::AppState;
use crate::services;

/// One transaction row as the evaluator wants it.
type TxnRow = (
    Uuid,
    chrono::NaiveDate,
    String,
    Cents,
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
    Option<Cents>,
    bool,
);
/// One decline event joined to its account.
type DeclineRow = (
    String,
    Uuid,
    String,
    DateTime<Utc>,
    Cents,
    String,
    Option<String>,
);

pub async fn run(state: AppState) -> anyhow::Result<Value> {
    let ctx = state.ctx().await?;
    let settings = services::settings::load(&ctx).await?;
    let today = ctx.today();
    let month = ctx.month();

    let view = services::budget::month_view(&ctx, month).await?;
    let categories = view
        .groups
        .iter()
        .flat_map(|g| g.categories.iter().cloned())
        .collect();

    let bills = services::bills::all(&ctx)
        .await?
        .iter()
        .map(services::bills::to_core)
        .collect();

    let since = today - Duration::days(45);
    let txns: Vec<TxnRow> = sqlx::query_as(
        "select id, date, description, amount_cents, category_id, planned_item_id, recurring_bill_id, bill_drift_cents, pending
         from transactions where household_id = $1 and deleted_at is null and shadow_of_transaction_id is null and date >= $2",
    )
    .bind(ctx.household_id)
    .bind(since)
    .fetch_all(&ctx.pool)
    .await?;
    let transactions = txns
        .into_iter()
        .map(
            |(
                id,
                date,
                description,
                amount_cents,
                category_id,
                planned_item_id,
                bill_id,
                bill_drift_cents,
                pending,
            )| TxnSnapshot {
                id,
                date,
                description,
                amount_cents,
                category_id,
                planned_item_id,
                bill_id,
                bill_drift_cents,
                pending,
            },
        )
        .collect();

    let planned = services::planned::list(
        &ctx,
        &services::planned::ListQuery {
            status: Some(crate::db::models::PlannedStatus::Planned),
            limit: Some(500),
            ..Default::default()
        },
    )
    .await?
    .into_iter()
    .map(|p| PlannedSnapshot {
        id: p.id,
        item: p.item,
        date: p.date,
        amount_cents: p.amount_cents,
        outstanding: p.matched_transaction_id.is_none(),
    })
    .collect();

    let projection = services::debts::projection(&ctx).await?;
    let mut debts = Vec::new();
    let mut payoff_updates: Vec<(Uuid, Option<Month>)> = Vec::new();
    for d in services::debts::list(&ctx, false).await? {
        let payoff = projection
            .debts
            .iter()
            .find(|p| p.debt_id == d.id)
            .and_then(|p| p.payoff_month);
        let previous_payoff = d
            .last_payoff_month
            .as_deref()
            .and_then(|m| m.parse::<Month>().ok());
        payoff_updates.push((d.id, payoff));
        debts.push(DebtSnapshot {
            id: d.id,
            name: d.name,
            balance_cents: d.balance_cents,
            balance_as_of: d.balance_as_of,
            previous_balance_cents: services::debts::previous_balance(&ctx, d.id).await?,
            min_payment_cents: d.min_payment_cents,
            payoff_month: payoff,
            previous_payoff_month: previous_payoff,
        });
    }

    let goal_rows = services::goals::list(&ctx, false).await?;
    let goals = goal_rows
        .iter()
        .map(|g| GoalSnapshot {
            id: g.id,
            name: g.name.clone(),
            target_cents: g.target_cents,
            saved_cents: g.saved_cents,
            previously_reached: g.reached_at.is_some(),
        })
        .collect();

    let accounts = services::accounts::list(&ctx, false)
        .await?
        .into_iter()
        .map(|a| AccountSnapshot {
            id: a.id,
            name: a.name,
            include_in_cash: a.include_in_cash,
            reported_balance_as_of: a.reported_balance_as_of,
            bank_balance_as_of: a.bank_balance_as_of,
        })
        .collect();

    let decl: Vec<DeclineRow> = sqlx::query_as(
        "select e.external_id, e.account_id, coalesce(e.card_label, a.name), e.occurred_at, e.amount_cents, e.descriptor, e.reason
         from card_events e join accounts a on a.id = e.account_id
         where a.household_id = $1 and e.result = 'declined' and e.occurred_at > now() - interval '7 days'",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?;
    let declines = decl
        .into_iter()
        .map(
            |(
                external_id,
                account_id,
                account_name,
                occurred_at,
                amount_cents,
                descriptor,
                reason,
            )| DeclineSnapshot {
                external_id,
                account_id,
                account_name,
                occurred_at,
                amount_cents,
                descriptor,
                reason,
            },
        )
        .collect();

    let mut sources: Vec<SourceHealth> = sqlx::query_as::<_, (Option<String>, i32, Option<String>)>(
        "select institution_name, consecutive_failures, last_error from plaid_items where household_id = $1",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?
    .into_iter()
    .map(|(name, consecutive_failures, last_error)| SourceHealth {
        source: format!("plaid:{}", name.unwrap_or_else(|| "bank".into())),
        consecutive_failures,
        last_error,
    })
    .collect();
    if services::integrations::privacy(&ctx, &state.config)
        .await?
        .is_some()
    {
        let runs: Vec<(String, Option<String>)> = sqlx::query_as(
            "select status::text, error from job_runs where job = 'sync:privacy' and status in ('ok','failed') order by started_at desc limit 10",
        )
        .fetch_all(&ctx.pool)
        .await?;
        let failures = runs.iter().take_while(|(s, _)| s == "failed").count() as i32;
        sources.push(SourceHealth {
            source: "privacy".into(),
            consecutive_failures: failures,
            last_error: runs.first().and_then(|(_, e)| e.clone()),
        });
    }

    let overview = services::balance::overview(&ctx).await?;
    let (review_pending_count, review_pending_oldest) =
        services::transactions::review_stats(&ctx).await?;

    let snapshot = Snapshot {
        now: ctx.now,
        today,
        month,
        settings,
        categories,
        bills,
        transactions,
        planned,
        debts,
        goals,
        accounts,
        declines,
        sources,
        low_point: Some(overview.low_point),
        review_pending_count,
        review_pending_oldest,
    };
    let candidates = alerts::evaluate(&snapshot);
    let summary = services::alerts::upsert_candidates(&ctx, "evaluate", &candidates).await?;

    // Bookkeeping the evaluator relies on next time (not decisions).
    for (id, payoff) in payoff_updates {
        sqlx::query("update debts set last_payoff_month = $2 where id = $1")
            .bind(id)
            .bind(payoff.map(|m| m.to_string()))
            .execute(&ctx.pool)
            .await?;
    }
    let mut reached = 0;
    for g in goal_rows.iter().filter(|g| {
        g.reached_at.is_none() && g.target_cents.0 > 0 && g.saved_cents >= g.target_cents
    }) {
        sqlx::query("update goals set reached_at = now(), status = case when status = 'active' then 'reached' else status end, updated_at = now() where id = $1")
            .bind(g.id)
            .execute(&ctx.pool)
            .await?;
        reached += 1;
    }

    Ok(json!({
        "candidates": candidates.len(),
        "created": summary.created,
        "refreshed": summary.refreshed,
        "goals_reached": reached,
        "low_point_cents": snapshot.low_point.as_ref().map(|l| l.low_point_cents),
    }))
}
