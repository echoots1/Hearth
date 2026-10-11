//! Encrypted secrets at rest (Anthropic API key, VAPID keys, Plaid tokens,
//! integration credentials).
//!
//! AES-256-GCM. The key is argon2id over `APP_ENCRYPTION_KEY` with a random
//! per-install salt kept in `crypto_meta`, so a human-chosen passphrase
//! cannot be brute-forced from a database dump alone. Ciphertext is
//! `v2.base64(nonce ‖ ciphertext+tag)`.
//!
//! Rows written before the salt existed (`base64(...)` with no prefix) were
//! encrypted under `sha256(passphrase)`; they still decrypt and are rewritten
//! in the current form the next time they are read.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use sha2::{Digest, Sha256};

use super::{Ctx, Result, invalid};

const V2: &str = "v2.";

#[derive(Clone)]
pub struct Crypto {
    key: [u8; 32],
    legacy: [u8; 32],
}

impl std::fmt::Debug for Crypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Crypto(<redacted>)")
    }
}

impl Crypto {
    /// Derive the key with argon2id (default parameters: 19 MiB, 2 passes)
    /// from the passphrase and the install's salt. Runs once at start.
    pub fn derive(passphrase: &str, salt: &[u8]) -> anyhow::Result<Crypto> {
        if salt.len() < 16 {
            anyhow::bail!("encryption salt is too short");
        }
        let mut key = [0u8; 32];
        argon2::Argon2::default()
            .hash_password_into(passphrase.as_bytes(), salt, &mut key)
            .map_err(|e| anyhow::anyhow!("deriving the encryption key: {e}"))?;
        let mut legacy = [0u8; 32];
        legacy.copy_from_slice(&Sha256::digest(passphrase.as_bytes()));
        Ok(Crypto { key, legacy })
    }

    fn cipher(key: &[u8; 32]) -> Aes256Gcm {
        let key: Key<Aes256Gcm> = (*key).into();
        Aes256Gcm::new(&key)
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce: Nonce<aes_gcm::aead::consts::U12> = nonce_bytes.into();
        let ct = Self::cipher(&self.key)
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;
        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ct);
        Ok(format!(
            "{V2}{}",
            base64::engine::general_purpose::STANDARD.encode(out)
        ))
    }

    /// Was this ciphertext written with the current derivation?
    pub fn is_current(ciphertext: &str) -> bool {
        ciphertext.starts_with(V2)
    }

    pub fn decrypt(&self, stored: &str) -> Result<String> {
        let (key, b64) = match stored.strip_prefix(V2) {
            Some(rest) => (&self.key, rest),
            None => (&self.legacy, stored),
        };
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|_| invalid("bad ciphertext"))?;
        if raw.len() < 13 {
            return Err(invalid("bad ciphertext"));
        }
        let (nonce_bytes, ct) = raw.split_at(12);
        let nonce_arr: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| invalid("bad ciphertext"))?;
        let nonce: Nonce<aes_gcm::aead::consts::U12> = nonce_arr.into();
        let pt = Self::cipher(key)
            .decrypt(&nonce, ct)
            .map_err(|_| anyhow::anyhow!("decrypt failed (wrong APP_ENCRYPTION_KEY?)"))?;
        String::from_utf8(pt).map_err(|_| anyhow::anyhow!("secret is not utf-8").into())
    }
}

/// The install's KDF salt: created on first use, never changed.
pub async fn load_or_create_salt(pool: &sqlx::PgPool) -> anyhow::Result<Vec<u8>> {
    let fresh: [u8; 16] = rand::random();
    sqlx::query(
        "insert into crypto_meta (id, kdf_salt) values (1, $1) on conflict (id) do nothing",
    )
    .bind(fresh.as_slice())
    .execute(pool)
    .await?;
    let (salt,): (Vec<u8>,) = sqlx::query_as("select kdf_salt from crypto_meta where id = 1")
        .fetch_one(pool)
        .await?;
    Ok(salt)
}

pub const ANTHROPIC_API_KEY: &str = "anthropic_api_key";
pub const VAPID_PRIVATE_KEY: &str = "vapid_private_key";
pub const VAPID_PUBLIC_KEY: &str = "vapid_public_key";

fn crypto(ctx: &Ctx) -> Result<&Crypto> {
    ctx.crypto
        .as_ref()
        .ok_or_else(|| invalid("APP_ENCRYPTION_KEY is not set; secrets cannot be stored"))
}

async fn store(ctx: &Ctx, name: &str, enc: &str) -> Result<()> {
    sqlx::query(
        "insert into secrets (household_id, name, value_enc) values ($1, $2, $3)
         on conflict (household_id, name) do update set value_enc = excluded.value_enc, updated_at = now()",
    )
    .bind(ctx.household_id)
    .bind(name)
    .bind(enc)
    .execute(&ctx.pool)
    .await?;
    Ok(())
}

pub async fn set(ctx: &Ctx, name: &str, value: &str) -> Result<()> {
    let enc = crypto(ctx)?.encrypt(value)?;
    store(ctx, name, &enc).await
}

pub async fn get(ctx: &Ctx, name: &str) -> Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("select value_enc from secrets where household_id = $1 and name = $2")
            .bind(ctx.household_id)
            .bind(name)
            .fetch_optional(&ctx.pool)
            .await?;
    match row {
        None => Ok(None),
        Some((enc,)) => {
            let c = crypto(ctx)?;
            let value = c.decrypt(&enc)?;
            if !Crypto::is_current(&enc) {
                // Rewrite a pre-salt row under the derived key; best effort.
                if let Ok(fresh) = c.encrypt(&value)
                    && let Err(e) = store(ctx, name, &fresh).await
                {
                    tracing::warn!(name, error = %e, "could not re-encrypt a legacy secret");
                }
            }
            Ok(Some(value))
        }
    }
}

pub async fn exists(ctx: &Ctx, name: &str) -> Result<bool> {
    let row: Option<(i32,)> =
        sqlx::query_as("select 1 from secrets where household_id = $1 and name = $2")
            .bind(ctx.household_id)
            .bind(name)
            .fetch_optional(&ctx.pool)
            .await?;
    Ok(row.is_some())
}

pub async fn delete(ctx: &Ctx, name: &str) -> Result<()> {
    sqlx::query("delete from secrets where household_id = $1 and name = $2")
        .bind(ctx.household_id)
        .bind(name)
        .execute(&ctx.pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALT: &[u8] = b"0123456789abcdef";

    #[test]
    fn roundtrip() {
        let c = Crypto::derive("a-long-passphrase-for-tests-1234567890", SALT).unwrap();
        let enc = c.encrypt("sk-ant-example").unwrap();
        assert!(Crypto::is_current(&enc));
        assert_ne!(enc, "sk-ant-example");
        assert_eq!(c.decrypt(&enc).unwrap(), "sk-ant-example");
        let other = Crypto::derive("different-passphrase-of-enough-length-1", SALT).unwrap();
        assert!(other.decrypt(&enc).is_err());
        let other_salt = Crypto::derive(
            "a-long-passphrase-for-tests-1234567890",
            b"fedcba9876543210",
        )
        .unwrap();
        assert!(other_salt.decrypt(&enc).is_err());
    }

    /// Rows written before the salt existed still decrypt.
    #[test]
    fn legacy_ciphertext_still_decrypts() {
        let pass = "a-long-passphrase-for-tests-1234567890";
        let mut legacy_key = [0u8; 32];
        legacy_key.copy_from_slice(&Sha256::digest(pass.as_bytes()));
        let nonce_bytes: [u8; 12] = [7; 12];
        let nonce: Nonce<aes_gcm::aead::consts::U12> = nonce_bytes.into();
        let ct = Crypto::cipher(&legacy_key)
            .encrypt(&nonce, b"old-secret".as_slice())
            .unwrap();
        let mut raw = nonce_bytes.to_vec();
        raw.extend_from_slice(&ct);
        let legacy = base64::engine::general_purpose::STANDARD.encode(raw);
        assert!(!Crypto::is_current(&legacy));
        let c = Crypto::derive(pass, SALT).unwrap();
        assert_eq!(c.decrypt(&legacy).unwrap(), "old-secret");
    }

    #[test]
    fn short_salt_is_refused() {
        assert!(Crypto::derive("passphrase-passphrase-passphrase-1", b"short").is_err());
    }
}
