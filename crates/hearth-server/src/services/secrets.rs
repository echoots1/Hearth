//! Encrypted secrets at rest (Anthropic API key, VAPID keys, Plaid tokens).
//!
//! AES-256-GCM with a key derived from `APP_ENCRYPTION_KEY` (sha256 of the
//! string). Ciphertext is `base64(nonce ‖ ciphertext+tag)`.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use sha2::{Digest, Sha256};

use super::{Ctx, Result, invalid};

#[derive(Clone)]
pub struct Crypto {
    key: [u8; 32],
}

impl std::fmt::Debug for Crypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Crypto(<redacted>)")
    }
}

impl Crypto {
    pub fn from_passphrase(p: &str) -> Crypto {
        let mut key = [0u8; 32];
        key.copy_from_slice(&Sha256::digest(p.as_bytes()));
        Crypto { key }
    }

    fn cipher(&self) -> Aes256Gcm {
        let key: Key<Aes256Gcm> = self.key.into();
        Aes256Gcm::new(&key)
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce: Nonce<aes_gcm::aead::consts::U12> = nonce_bytes.into();
        let ct = self
            .cipher()
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;
        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ct);
        Ok(base64::engine::general_purpose::STANDARD.encode(out))
    }

    pub fn decrypt(&self, b64: &str) -> Result<String> {
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
        let pt = self
            .cipher()
            .decrypt(&nonce, ct)
            .map_err(|_| anyhow::anyhow!("decrypt failed (wrong APP_ENCRYPTION_KEY?)"))?;
        String::from_utf8(pt).map_err(|_| anyhow::anyhow!("secret is not utf-8").into())
    }
}

pub const ANTHROPIC_API_KEY: &str = "anthropic_api_key";
pub const VAPID_PRIVATE_KEY: &str = "vapid_private_key";
pub const VAPID_PUBLIC_KEY: &str = "vapid_public_key";

fn crypto(ctx: &Ctx) -> Result<&Crypto> {
    ctx.crypto
        .as_ref()
        .ok_or_else(|| invalid("APP_ENCRYPTION_KEY is not set; secrets cannot be stored"))
}

pub async fn set(ctx: &Ctx, name: &str, value: &str) -> Result<()> {
    let enc = crypto(ctx)?.encrypt(value)?;
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

pub async fn get(ctx: &Ctx, name: &str) -> Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("select value_enc from secrets where household_id = $1 and name = $2")
            .bind(ctx.household_id)
            .bind(name)
            .fetch_optional(&ctx.pool)
            .await?;
    match row {
        None => Ok(None),
        Some((enc,)) => Ok(Some(crypto(ctx)?.decrypt(&enc)?)),
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

    #[test]
    fn roundtrip() {
        let c = Crypto::from_passphrase("a-long-passphrase-for-tests-1234567890");
        let enc = c.encrypt("sk-ant-example").unwrap();
        assert_ne!(enc, "sk-ant-example");
        assert_eq!(c.decrypt(&enc).unwrap(), "sk-ant-example");
        let other = Crypto::from_passphrase("different");
        assert!(other.decrypt(&enc).is_err());
    }
}
