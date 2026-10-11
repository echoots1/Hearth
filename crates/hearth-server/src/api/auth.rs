use axum::{Json, extract::State};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::{Deserialize, Serialize};
use time::Duration;

use super::{ApiError, AppState};
use crate::auth::extract::AuthUser;
use crate::auth::{SESSION_COOKIE, passwords, sessions};
use crate::db::models::{User, UserRole};
use crate::services::{self, Actor};

#[derive(Serialize)]
pub struct Status {
    /// True until the first (owner) account exists.
    needs_setup: bool,
    signed_in: bool,
}

pub async fn status(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Status>, ApiError> {
    let n: i64 = sqlx::query_scalar("select count(*) from users")
        .fetch_one(&state.pool)
        .await?;
    let signed_in = match jar.get(SESSION_COOKIE) {
        Some(c) => sessions::validate(&state.pool, c.value()).await?.is_some(),
        None => false,
    };
    Ok(Json(Status {
        needs_setup: n == 0,
        signed_in,
    }))
}

#[derive(Deserialize)]
pub struct SetupBody {
    household_name: String,
    timezone: String,
    email: String,
    display_name: String,
    password: String,
    /// Printed to the server log on every start while no account exists.
    #[serde(default)]
    setup_token: String,
}

#[derive(Serialize)]
pub struct Me {
    id: uuid::Uuid,
    email: String,
    display_name: String,
    role: UserRole,
    household: String,
    timezone: String,
}

fn me_of(u: &User, h: &crate::db::models::Household) -> Me {
    Me {
        id: u.id,
        email: u.email.clone(),
        display_name: u.display_name.clone(),
        role: u.role,
        household: h.name.clone(),
        timezone: h.timezone.clone(),
    }
}

fn session_cookie(state: &AppState, value: String, max_age: Duration) -> Cookie<'static> {
    let mut c = Cookie::new(SESSION_COOKIE, value);
    c.set_http_only(true);
    c.set_same_site(SameSite::Lax);
    c.set_path("/");
    c.set_secure(state.runtime().is_https());
    c.set_max_age(max_age);
    c
}

/// First run: create the owner and name the household. Needs the setup token
/// from the server log; refused once any user exists. The insert runs under
/// an advisory lock so two first visits cannot both become the owner.
pub async fn setup(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(b): Json<SetupBody>,
) -> Result<(CookieJar, Json<Me>), ApiError> {
    let n: i64 = sqlx::query_scalar("select count(*) from users")
        .fetch_one(&state.pool)
        .await?;
    if n > 0 {
        return Err(ApiError::forbidden("setup is already complete"));
    }
    if !state.setup_token.matches(&b.setup_token) {
        return Err(ApiError::forbidden(
            "wrong setup token: it is printed in the server log (`docker compose logs app`, look for \"setup token\")",
        ));
    }
    passwords::validate(&b.password).map_err(ApiError::bad_request)?;
    if !b.email.contains('@') {
        return Err(ApiError::bad_request("a valid email is required"));
    }
    let hash = passwords::hash(&b.password)
        .map_err(|_| ApiError::bad_request("could not hash password"))?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("select pg_advisory_xact_lock(hashtext('hearth:setup'))")
        .execute(&mut *tx)
        .await?;
    let n: i64 = sqlx::query_scalar("select count(*) from users")
        .fetch_one(&mut *tx)
        .await?;
    if n > 0 {
        return Err(ApiError::forbidden("setup is already complete"));
    }
    let u = sqlx::query_as::<_, User>(
        "insert into users (household_id, email, display_name, password_hash, role) values ($1, $2, $3, $4, 'owner') returning *",
    )
    .bind(state.household_id)
    .bind(b.email.trim().to_lowercase())
    .bind(b.display_name.trim())
    .bind(hash)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    state.setup_token.clear();
    let ctx = state.ctx().await?;
    let h = services::household::update(
        &ctx,
        &Actor::System,
        services::household::UpdateHousehold {
            name: Some(b.household_name.trim().to_string()),
            timezone: Some(b.timezone.clone()),
            currency: None,
        },
    )
    .await?;
    services::audit::record(
        &ctx.pool,
        ctx.household_id,
        &Actor::System,
        "user.create",
        "users",
        u.id,
        None::<&()>,
        Some(&serde_json::json!({ "email": u.email, "role": u.role, "setup": true })),
    )
    .await?;
    let s = sessions::create(&state.pool, u.id).await?;
    let jar = jar.add(session_cookie(
        &state,
        s.id,
        Duration::days(sessions::SESSION_DAYS),
    ));
    Ok((jar, Json(me_of(&u, &h))))
}

#[derive(Deserialize)]
pub struct LoginBody {
    email: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(b): Json<LoginBody>,
) -> Result<(CookieJar, Json<Me>), ApiError> {
    let user = sqlx::query_as::<_, User>("select * from users where email = $1")
        .bind(b.email.trim())
        .fetch_optional(&state.pool)
        .await?;
    // Constant-ish time: verify against a dummy hash when the user is unknown.
    let ok = match &user {
        Some(u) => passwords::verify(&b.password, &u.password_hash),
        None => {
            let _ = passwords::verify(
                &b.password,
                "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            );
            false
        }
    };
    let Some(u) = user.filter(|_| ok) else {
        return Err(ApiError {
            status: axum::http::StatusCode::UNAUTHORIZED,
            message: "email or password is wrong".into(),
        });
    };
    let h = services::household::ensure(&state.pool).await?;
    let s = sessions::create(&state.pool, u.id).await?;
    let jar = jar.add(session_cookie(
        &state,
        s.id,
        Duration::days(sessions::SESSION_DAYS),
    ));
    Ok((jar, Json(me_of(&u, &h))))
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Result<CookieJar, ApiError> {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        sessions::delete(&state.pool, c.value()).await?;
    }
    Ok(jar.remove(session_cookie(&state, String::new(), Duration::ZERO)))
}

pub async fn me(
    State(state): State<AppState>,
    AuthUser(u): AuthUser,
) -> Result<Json<Me>, ApiError> {
    let h = services::household::ensure(&state.pool).await?;
    Ok(Json(me_of(&u, &h)))
}
