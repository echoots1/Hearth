//! `housekeeping`: prune what nobody needs any more. Never touches money data.

use serde_json::{Value, json};

use crate::api::AppState;

pub async fn run(state: AppState) -> anyhow::Result<Value> {
    let pool = &state.pool;
    let runs = sqlx::query("delete from job_runs where started_at < now() - interval '30 days'")
        .execute(pool)
        .await?
        .rows_affected();
    let sessions = sqlx::query("delete from sessions where expires_at < now()")
        .execute(pool)
        .await?
        .rows_affected();
    let subs = sqlx::query("delete from push_subscriptions where failures >= 8")
        .execute(pool)
        .await?
        .rows_affected();
    let notifications =
        sqlx::query("delete from notifications where sent_at < now() - interval '180 days'")
            .execute(pool)
            .await?
            .rows_affected();
    let snapshots =
        sqlx::query("delete from balance_snapshots where created_at < now() - interval '3 years'")
            .execute(pool)
            .await?
            .rows_affected();
    Ok(json!({
        "job_runs": runs,
        "sessions": sessions,
        "push_subscriptions": subs,
        "notifications": notifications,
        "balance_snapshots": snapshots,
    }))
}
