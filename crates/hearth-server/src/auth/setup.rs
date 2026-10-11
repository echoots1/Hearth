//! The first-run setup token.
//!
//! Until the first account exists, `/api/auth/setup` would otherwise be open
//! to whoever reaches the public URL first. So on every start with an empty
//! `users` table the server draws a random token, prints it to its log, and
//! setup requires it. Anyone who can read the container log owns the box
//! anyway; a stranger on the internet does not.

use std::sync::{Arc, RwLock};

use base64::Engine;
use sha2::{Digest, Sha256};

#[derive(Clone, Default)]
pub struct SetupToken(Arc<RwLock<Option<String>>>);

impl SetupToken {
    /// Draw a fresh token and remember it.
    pub fn issue(&self) -> String {
        let bytes: [u8; 24] = rand::random();
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        self.set(Some(token.clone()));
        token
    }

    /// Forget the token (setup is done).
    pub fn clear(&self) {
        self.set(None);
    }

    /// Constant-time comparison against the issued token, if any.
    pub fn matches(&self, given: &str) -> bool {
        let issued = self
            .0
            .read()
            .map(|t| t.clone())
            .unwrap_or_else(|e| e.into_inner().clone());
        let Some(issued) = issued else {
            return false;
        };
        let a = Sha256::digest(issued.as_bytes());
        let b = Sha256::digest(given.trim().as_bytes());
        a.iter()
            .zip(b.iter())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
    }

    fn set(&self, v: Option<String>) {
        match self.0.write() {
            Ok(mut w) => *w = v,
            Err(e) => *e.into_inner() = v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_match_clear() {
        let t = SetupToken::default();
        assert!(!t.matches(""));
        let token = t.issue();
        assert!(t.matches(&token));
        assert!(t.matches(&format!("  {token} ")));
        assert!(!t.matches("nope"));
        assert!(!t.matches(&token[1..]));
        t.clear();
        assert!(!t.matches(&token));
    }
}
