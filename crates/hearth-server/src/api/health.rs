//! `/api/health`: unauthenticated, so it says only whether the database answers.

use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;

use super::AppState;

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    database: &'static str,
}

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let db_ok = sqlx::query_scalar::<_, i32>("select 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(Health {
            status: if db_ok { "ok" } else { "degraded" },
            database: if db_ok { "ok" } else { "unreachable" },
        }),
    )
}
