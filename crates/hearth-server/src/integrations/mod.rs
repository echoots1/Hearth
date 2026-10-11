//! External systems: Web Push, ntfy, SMTP, Plaid, Privacy.com, Claude.

pub mod claude;
pub mod ntfy;
pub mod plaid;
pub mod privacy;
pub mod smtp;
pub mod webpush;

use crate::api::AppState;
use crate::jobs::Job;

/// Sync jobs for the feeds. Both are always registered; the scheduler runs
/// them only while the integration is configured (in the app or the
/// environment), and `jobs.run` can always force one.
pub fn jobs(state: &AppState) -> Vec<Job> {
    vec![
        Job {
            name: "sync:plaid",
            description: "pull transactions and balances for every linked bank, then process",
            every: std::time::Duration::from_secs(state.config.jobs.plaid_sync_minutes.max(5) * 60),
            handler: |s| Box::pin(sync_plaid(s)),
            gate: Some(|s| Box::pin(plaid_configured(s))),
        },
        Job {
            name: "sync:privacy",
            description: "pull virtual-card charges and declines, then process",
            every: std::time::Duration::from_secs(
                state.config.jobs.privacy_sync_minutes.max(2) * 60,
            ),
            handler: |s| Box::pin(sync_privacy(s)),
            gate: Some(|s| Box::pin(privacy_configured(s))),
        },
    ]
}

async fn plaid_configured(state: AppState) -> bool {
    match state.ctx().await {
        Ok(ctx) => matches!(
            crate::services::integrations::plaid(&ctx, &state.config).await,
            Ok(Some(_))
        ),
        Err(_) => false,
    }
}

async fn privacy_configured(state: AppState) -> bool {
    match state.ctx().await {
        Ok(ctx) => matches!(
            crate::services::integrations::privacy(&ctx, &state.config).await,
            Ok(Some(_))
        ),
        Err(_) => false,
    }
}

async fn sync_plaid(state: AppState) -> anyhow::Result<serde_json::Value> {
    let ctx = state.ctx().await?;
    let (cfg, _) = crate::services::integrations::plaid(&ctx, &state.config)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Plaid is not configured"))?;
    let client = plaid::Plaid::new(&cfg)?;
    let actor = crate::jobs::actor("sync:plaid");
    let items = crate::services::plaid::items(&ctx).await?;
    let mut results = serde_json::Map::new();
    let mut failures = 0;
    for item in &items {
        let name = item
            .institution_name
            .clone()
            .unwrap_or_else(|| item.item_id.clone());
        match crate::services::plaid::sync_item(&ctx, &actor, &client, item).await {
            Ok(s) => {
                results.insert(name, serde_json::to_value(s)?);
            }
            Err(e) => {
                failures += 1;
                results.insert(name, serde_json::json!({ "error": e.to_string() }));
            }
        }
    }
    if !items.is_empty() {
        let _ = crate::jobs::run_named(state.clone(), "process", "sync:plaid").await;
    }
    if failures > 0 && failures == items.len() {
        anyhow::bail!("every item failed: {}", serde_json::Value::Object(results));
    }
    Ok(serde_json::json!({ "items": items.len(), "failed": failures, "results": results }))
}

async fn sync_privacy(state: AppState) -> anyhow::Result<serde_json::Value> {
    let ctx = state.ctx().await?;
    let (cfg, _) = crate::services::integrations::privacy(&ctx, &state.config)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Privacy.com is not configured"))?;
    let client = privacy::Privacy::new(&cfg)?;
    let actor = crate::jobs::actor("sync:privacy");
    let s = crate::services::privacy::sync(&ctx, &actor, &client).await?;
    let _ = crate::jobs::run_named(state.clone(), "process", "sync:privacy").await;
    Ok(serde_json::to_value(s)?)
}
