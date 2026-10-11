//! Local auth: argon2id passwords, DB-backed sessions, hashed API keys, and
//! the Axum extractors that turn a request into an actor.

pub mod actions;
pub mod api_keys;
pub mod extract;
pub mod passwords;
pub mod sessions;

/// Cookie that carries the session id.
pub const SESSION_COOKIE: &str = "hearth_session";
