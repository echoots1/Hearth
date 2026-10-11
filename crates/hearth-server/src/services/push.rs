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

/// A push endpoint the notify job may POST to. Browsers hand out `https://`
/// URLs on public push services; anything that looks like an internal host
/// (an IP literal, `localhost`, a `.local`/`.lan`/`.internal` name) is refused
/// so a signed-in person cannot point the server at the home network.
pub fn validate_endpoint(endpoint: &str) -> std::result::Result<(), String> {
    let Some(rest) = endpoint.strip_prefix("https://") else {
        return Err("endpoint must be https".into());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err("endpoint must not carry credentials".into());
    }
    let host = authority
        .strip_prefix('[')
        .and_then(|h| h.split(']').next())
        .map(str::to_string)
        .unwrap_or_else(|| {
            authority
                .rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(authority)
                .to_string()
        })
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err("endpoint needs a host name".into());
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Err("endpoint must be a host name, not an address".into());
    }
    const PRIVATE_SUFFIXES: &[&str] = &[
        ".local",
        ".localhost",
        ".internal",
        ".lan",
        ".home",
        ".home.arpa",
        ".corp",
        ".intranet",
        ".localdomain",
    ];
    if host == "localhost"
        || !host.contains('.')
        || PRIVATE_SUFFIXES.iter().any(|s| host.ends_with(s))
    {
        return Err("endpoint must be a public push service".into());
    }
    Ok(())
}

pub async fn subscribe(ctx: &Ctx, user_id: Uuid, s: Subscribe) -> Result<PushSubscription> {
    validate_endpoint(&s.endpoint).map_err(invalid)?;
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

#[cfg(test)]
mod tests {
    use super::validate_endpoint;

    #[test]
    fn public_push_services_pass() {
        for e in [
            "https://fcm.googleapis.com/fcm/send/abc:APA91b",
            "https://web.push.apple.com/QGxyz",
            "https://updates.push.services.mozilla.com/wpush/v2/gAAAA",
            "https://wns2-bl2p.notify.windows.com/w/?token=AwYAAA",
        ] {
            assert!(validate_endpoint(e).is_ok(), "{e}");
        }
    }

    #[test]
    fn internal_targets_are_refused() {
        for e in [
            "http://fcm.googleapis.com/x",
            "https://192.168.1.20/x",
            "https://[::1]:8443/x",
            "https://localhost/x",
            "https://nas/x",
            "https://nas.local/x",
            "https://router.lan/x",
            "https://db.internal:5432/x",
            "https://user:pw@fcm.googleapis.com/x",
            "https:///x",
        ] {
            assert!(validate_endpoint(e).is_err(), "{e}");
        }
    }
}
