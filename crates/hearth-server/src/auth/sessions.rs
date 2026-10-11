use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::models::{Session, User};

pub const SESSION_DAYS: i64 = 30;

fn new_id() -> String {
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn create(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Session> {
    let id = new_id();
    let expires = Utc::now() + Duration::days(SESSION_DAYS);
    sqlx::query_as::<_, Session>(
        "insert into sessions (id, user_id, expires_at) values ($1, $2, $3) returning *",
    )
    .bind(&id)
    .bind(user_id)
    .bind(expires)
    .fetch_one(pool)
    .await
}

/// Validate a session id; renews the expiry when past the halfway point.
pub async fn validate(pool: &PgPool, id: &str) -> sqlx::Result<Option<(Session, User)>> {
    let Some(s) =
        sqlx::query_as::<_, Session>("select * from sessions where id = $1 and expires_at > now()")
            .bind(id)
            .fetch_optional(pool)
            .await?
    else {
        return Ok(None);
    };
    let Some(u) = sqlx::query_as::<_, User>("select * from users where id = $1")
        .bind(s.user_id)
        .fetch_optional(pool)
        .await?
    else {
        return Ok(None);
    };
    let halfway: DateTime<Utc> = s.expires_at - Duration::days(SESSION_DAYS / 2);
    if Utc::now() > halfway {
        sqlx::query("update sessions set expires_at = $2 where id = $1")
            .bind(&s.id)
            .bind(Utc::now() + Duration::days(SESSION_DAYS))
            .execute(pool)
            .await?;
    }
    Ok(Some((s, u)))
}

pub async fn delete(pool: &PgPool, id: &str) -> sqlx::Result<()> {
    sqlx::query("delete from sessions where id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_all_for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("delete from sessions where user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}
