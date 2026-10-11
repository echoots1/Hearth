//! `/api/tools` — the registry over HTTP, plus proposal approval and undo.

use axum::extract::{Path, State};
use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ApiError, AppState};
use crate::auth::extract::{ApiCaller, AuthUser};
use crate::services;
use crate::tools::{self, ToolCtx};

#[derive(Serialize)]
pub struct ToolInfo {
    name: &'static str,
    description: &'static str,
    writes: bool,
    area: Option<hearth_core::settings::AutonomyArea>,
    input_schema: Value,
}

pub async fn list(_caller: ApiCaller) -> Json<Vec<ToolInfo>> {
    Json(
        tools::all()
            .map(|t| ToolInfo {
                name: t.name,
                description: t.description,
                writes: t.writes,
                area: t.area,
                input_schema: t.schema(),
            })
            .collect(),
    )
}

/// Tools a signed-in member may not call; the owner and API keys may.
pub fn owner_only(name: &str) -> bool {
    services::assistant::is_destructive(name)
        || matches!(name, "jobs.run" | "household.update_settings")
}

pub async fn call(
    State(state): State<AppState>,
    caller: ApiCaller,
    Path(name): Path<String>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    if let ApiCaller::User(u) = &caller
        && u.role != crate::db::models::UserRole::Owner
        && owner_only(&name)
    {
        return Err(ApiError::forbidden(format!(
            "only the owner can call {name}"
        )));
    }
    let ctx = state.ctx().await?;
    let tc = ToolCtx {
        state: state.clone(),
        ctx,
        actor: caller.actor(),
        can_write: caller.can_write(),
    };
    let args = body.map(|b| b.0).unwrap_or(Value::Null);
    Ok(Json(tools::call(tc, &name, args).await?))
}

#[derive(Deserialize, Default)]
pub struct ApproveBody {
    /// Edited arguments, if the person changed the proposal before approving.
    args: Option<Value>,
}

/// Approve a proposal: run its tool as the approving person, then mark it applied.
pub async fn approve_proposal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    body: Option<Json<ApproveBody>>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let actor = user.actor();
    let (q, proposal) =
        services::questions::approve(&ctx, &actor, id, body.and_then(|b| b.0.args)).await?;
    let tc = ToolCtx {
        state: state.clone(),
        ctx: ctx.clone(),
        actor,
        can_write: true,
    };
    let result = tools::call(tc, &proposal.tool, proposal.args.clone()).await?;
    let q = services::questions::mark_applied(&ctx, q.id).await?;
    Ok(Json(json!({ "question": q, "result": result })))
}

#[derive(Deserialize, Default)]
pub struct DeclineBody {
    reason: Option<String>,
}

pub async fn decline_proposal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    body: Option<Json<DeclineBody>>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let q = services::questions::decline(&ctx, &user.actor(), id, body.and_then(|b| b.0.reason))
        .await?;
    Ok(Json(serde_json::to_value(q)?))
}

/// Undo an assistant action: run its inverse as the person, then mark it undone.
pub async fn undo_action(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ctx = state.ctx().await?;
    let actor = user.actor();
    let spec = services::assistant::undo_spec(&ctx, &actor, id).await?;
    let tc = ToolCtx {
        state: state.clone(),
        ctx: ctx.clone(),
        actor: actor.clone(),
        can_write: true,
    };
    let result = tools::call(tc, &spec.tool, spec.args.clone()).await?;
    let a = services::assistant::mark_undone(&ctx, &actor, id).await?;
    Ok(Json(json!({ "action": a, "result": result })))
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::owner_only;

    #[test]
    fn destructive_and_admin_tools_are_owner_only() {
        for t in [
            "transactions.delete",
            "rules.delete",
            "bills.delete",
            "budget.delete_plan",
            "jobs.run",
            "household.update_settings",
        ] {
            assert!(owner_only(t), "{t}");
        }
        for t in [
            "transactions.categorize",
            "bills.cancel",
            "alerts.ack",
            "budget.set_plan",
        ] {
            assert!(!owner_only(t), "{t}");
        }
    }
}
