//! External systems: Web Push, ntfy, SMTP; later Plaid, Privacy.com, Claude.

pub mod ntfy;
pub mod smtp;
pub mod webpush;

use crate::api::AppState;
use crate::jobs::Job;

/// Sync jobs contributed by the integrations that are configured.
pub fn jobs(_state: &AppState) -> Vec<Job> {
    Vec::new()
}
