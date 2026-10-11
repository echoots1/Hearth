//! Web Push subscriptions.

use serde::Deserialize;
use uuid::Uuid;

use super::{Ctx, Result, invalid};
use crate::db::models::PushSubscription;

#[derive(Debug, Clone, Deserialize)]
pub struct Subscribe {
    pub endpoint: String,
    pub keys: PushKeys,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushKeys {
    pub p256dh: String,
    pub auth: String,
}

pub async fn subscribe(ctx: &Ctx, user_id: Uuid, s: Subscribe) -> Result<PushSubscription> {
    if !s.endpoint.starts_with("https://") {
        return Err(invalid("endpoint must be https"));
    }
    Ok(sqlx::query_as::<_, PushSubscription>(
        "insert into push_subscriptions (user_id, endpoint, p256dh, auth, user_agent)
         values ($1, $2, $3, $4, $5)
         on conflict (endpoint) do update set user_id = excluded.user_id, p256dh = excluded.p256dh, auth = excluded.auth,
           user_agent = excluded.user_agent, failures = 0
         returning *",
    )
    .bind(user_id)
    .bind(&s.endpoint)
    .bind(&s.keys.p256dh)
    .bind(&s.keys.auth)
    .bind(&s.user_agent)
    .fetch_one(&ctx.pool)
    .await?)
}

pub async fn unsubscribe(ctx: &Ctx, user_id: Uuid, endpoint: &str) -> Result<bool> {
    let n = sqlx::query("delete from push_subscriptions where user_id = $1 and endpoint = $2")
        .bind(user_id)
        .bind(endpoint)
        .execute(&ctx.pool)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// Every subscription in the household (all users).
pub async fn all(ctx: &Ctx) -> Result<Vec<PushSubscription>> {
    Ok(sqlx::query_as::<_, PushSubscription>(
        "select p.* from push_subscriptions p join users u on u.id = p.user_id where u.household_id = $1 order by p.created_at",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn list_for_user(ctx: &Ctx, user_id: Uuid) -> Result<Vec<PushSubscription>> {
    Ok(sqlx::query_as::<_, PushSubscription>(
        "select * from push_subscriptions where user_id = $1 order by created_at",
    )
    .bind(user_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn mark_success(ctx: &Ctx, id: Uuid) -> Result<()> {
    sqlx::query("update push_subscriptions set last_success = now(), failures = 0 where id = $1")
        .bind(id)
        .execute(&ctx.pool)
        .await?;
    Ok(())
}

/// Count a failure; `gone` (404/410 from the push service) removes the subscription.
pub async fn mark_failure(ctx: &Ctx, id: Uuid, gone: bool) -> Result<()> {
    if gone {
        sqlx::query("delete from push_subscriptions where id = $1")
            .bind(id)
            .execute(&ctx.pool)
            .await?;
    } else {
        sqlx::query("update push_subscriptions set failures = failures + 1 where id = $1")
            .bind(id)
            .execute(&ctx.pool)
            .await?;
    }
    Ok(())
}
