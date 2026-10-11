//! `assistant:pass`: in API mode, Claude works the queue once a day.

use serde_json::{Value, json};

use crate::api::AppState;
use crate::services;

pub async fn run(state: AppState) -> anyhow::Result<Value> {
    let ctx = state.ctx().await?;
    let turn = services::chat::pass(&state, &ctx).await?;
    Ok(json!({
        "cost_cents": turn.cost_cents,
        "tool_calls": turn.tool_calls.len(),
        "tools": turn.tool_calls.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
        "stop_reason": turn.stop_reason,
        "summary": turn.text.chars().take(400).collect::<String>(),
    }))
}

/// The scheduler asks this before running on its own; `jobs.run` ignores it.
pub async fn gate(state: AppState) -> bool {
    match state.ctx().await {
        Ok(ctx) => services::chat::pass_due(&ctx).await.unwrap_or(false),
        Err(_) => false,
    }
}
