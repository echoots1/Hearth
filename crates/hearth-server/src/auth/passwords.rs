use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};

#[derive(Debug, thiserror::Error)]
#[error("password hashing failed")]
pub struct HashError;

pub fn hash(password: &str) -> Result<String, HashError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|_| HashError)
}

pub fn verify(password: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Minimum policy: 10+ characters. (Length beats complexity rules.)
pub fn validate(password: &str) -> Result<(), String> {
    if password.chars().count() < 10 {
        return Err("password must be at least 10 characters".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let h = hash("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("correct horse battery", &h));
        assert!(!verify("wrong", &h));
        assert!(!verify("x", "not-a-hash"));
        assert!(validate("short").is_err());
        assert!(validate("long enough password").is_ok());
    }
}
