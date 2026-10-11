//! Axum extractors.
//!
//! - [`AuthUser`]: a signed-in person (session cookie). UI routes.
//! - [`ApiCaller`]: a person **or** an API key (bearer). Routes the assistant may also call.

use axum::extract::FromRequestParts;
use axum::http::{StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;

use super::{SESSION_COOKIE, api_keys, sessions};
use crate::api::AppState;
use crate::db::models::{ApiKey, ApiKeyScope, User, UserRole};
use crate::services::Actor;

#[derive(Clone, Debug)]
pub struct AuthUser(pub User);

impl AuthUser {
    pub fn actor(&self) -> Actor {
        Actor::User {
            id: self.0.id,
            name: self.0.display_name.clone(),
        }
    }

    pub fn is_owner(&self) -> bool {
        self.0.role == UserRole::Owner
    }
}

#[derive(Clone, Debug)]
pub enum ApiCaller {
    User(User),
    Key(ApiKey),
}

impl ApiCaller {
    pub fn actor(&self) -> Actor {
        match self {
            ApiCaller::User(u) => Actor::User {
                id: u.id,
                name: u.display_name.clone(),
            },
            ApiCaller::Key(k) => Actor::Assistant {
                key_id: Some(k.id),
                name: k.name.clone(),
            },
        }
    }

    pub fn can_write(&self) -> bool {
        match self {
            ApiCaller::User(_) => true,
            ApiCaller::Key(k) => k.scope == ApiKeyScope::ReadWrite,
        }
    }

}

pub struct AuthRejection(StatusCode, &'static str);

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (self.0, axum::Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

async fn user_from_cookie(
    state: &AppState,
    parts: &mut Parts,
) -> Result<Option<User>, AuthRejection> {
    let jar = CookieJar::from_request_parts(parts, state)
        .await
        .map_err(|_| AuthRejection(StatusCode::UNAUTHORIZED, "no session"))?;
    let Some(c) = jar.get(SESSION_COOKIE) else {
        return Ok(None);
    };
    sessions::validate(&state.pool, c.value())
        .await
        .map_err(|_| AuthRejection(StatusCode::INTERNAL_SERVER_ERROR, "session lookup failed"))
        .map(|r| r.map(|(_, u)| u))
}

fn bearer(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match user_from_cookie(state, parts).await? {
            Some(u) => Ok(AuthUser(u)),
            None => Err(AuthRejection(StatusCode::UNAUTHORIZED, "sign in required")),
        }
    }
}

impl FromRequestParts<AppState> for ApiCaller {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = bearer(parts) {
            return match api_keys::verify(&state.pool, token).await {
                Ok(Some(k)) => Ok(ApiCaller::Key(k)),
                Ok(None) => Err(AuthRejection(StatusCode::UNAUTHORIZED, "invalid API key")),
                Err(_) => Err(AuthRejection(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "key lookup failed",
                )),
            };
        }
        match user_from_cookie(state, parts).await? {
            Some(u) => Ok(ApiCaller::User(u)),
            None => Err(AuthRejection(
                StatusCode::UNAUTHORIZED,
                "sign in or API key required",
            )),
        }
    }
}
