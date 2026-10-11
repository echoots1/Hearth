//! Signed one-tap links in notifications: acknowledge or snooze an alert
//! without signing in. `HMAC-SHA256(APP_SECRET)` over `alert_id ‖ action ‖ exp`.

use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Ack,
    Snooze,
}

impl Action {
    fn code(self) -> u8 {
        match self {
            Action::Ack => 1,
            Action::Snooze => 2,
        }
    }
    fn from_code(c: u8) -> Option<Action> {
        match c {
            1 => Some(Action::Ack),
            2 => Some(Action::Snooze),
            _ => None,
        }
    }
}

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// A token valid until `expires`.
pub fn sign(secret: &str, alert_id: Uuid, action: Action, expires: DateTime<Utc>) -> String {
    let mut body = Vec::with_capacity(25);
    body.extend_from_slice(alert_id.as_bytes());
    body.push(action.code());
    body.extend_from_slice(&expires.timestamp().to_be_bytes());
    let mut m =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    m.update(&body);
    let tag = m.finalize().into_bytes();
    format!("{}.{}", B64.encode(&body), B64.encode(&tag[..16]))
}

/// The alert and action a token names, if the signature holds and it hasn't expired.
pub fn verify(secret: &str, token: &str, now: DateTime<Utc>) -> Option<(Uuid, Action)> {
    let (body_b64, tag_b64) = token.split_once('.')?;
    let body = B64.decode(body_b64).ok()?;
    let tag = B64.decode(tag_b64).ok()?;
    if body.len() != 25 || tag.len() != 16 {
        return None;
    }
    let mut m = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
    m.update(&body);
    m.verify_truncated_left(&tag).ok()?;
    let alert_id = Uuid::from_slice(&body[..16]).ok()?;
    let action = Action::from_code(body[16])?;
    let exp = i64::from_be_bytes(body[17..25].try_into().ok()?);
    if now.timestamp() > exp {
        return None;
    }
    Some((alert_id, action))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_tamper() {
        let id = Uuid::new_v4();
        let exp = Utc::now() + chrono::Duration::hours(1);
        let t = sign("secret-secret-secret", id, Action::Snooze, exp);
        assert_eq!(
            verify("secret-secret-secret", &t, Utc::now()),
            Some((id, Action::Snooze))
        );
        assert_eq!(verify("other-secret", &t, Utc::now()), None);
        let flipped = if t.starts_with('A') { "B" } else { "A" };
        let bad = format!("{flipped}{}", &t[1..]);
        assert_eq!(verify("secret-secret-secret", &bad, Utc::now()), None);
        let old = sign(
            "secret-secret-secret",
            id,
            Action::Ack,
            Utc::now() - chrono::Duration::minutes(1),
        );
        assert_eq!(verify("secret-secret-secret", &old, Utc::now()), None);
    }
}
