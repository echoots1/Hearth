use base64::Engine;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::models::{ApiKey, ApiKeyScope};

const PREFIX: &str = "hearth_";

/// A freshly minted key: the plaintext is shown once and never stored.
pub struct NewKey {
    pub plaintext: String,
    pub prefix: String,
    pub hash: String,
}

pub fn generate() -> NewKey {
    let bytes: [u8; 32] = rand::random();
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let plaintext = format!("{PREFIX}{secret}");
    NewKey {
        prefix: secret[..8].to_string(),
        hash: hash_key(&plaintext),
        plaintext,
    }
}

pub fn hash_key(plaintext: &str) -> String {
    let d = Sha256::digest(plaintext.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn create(
    pool: &PgPool,
    household_id: Uuid,
    name: &str,
    scope: ApiKeyScope,
    created_by: Option<Uuid>,
) -> sqlx::Result<(ApiKey, String)> {
    let k = generate();
    let row = sqlx::query_as::<_, ApiKey>(
        "insert into api_keys (household_id, name, key_prefix, key_hash, scope, created_by) values ($1, $2, $3, $4, $5, $6) returning *",
    )
    .bind(household_id)
    .bind(name.trim())
    .bind(&k.prefix)
    .bind(&k.hash)
    .bind(scope)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok((row, k.plaintext))
}

/// Look up a bearer token; touches `last_used_at`.
pub async fn verify(pool: &PgPool, bearer: &str) -> sqlx::Result<Option<ApiKey>> {
    if !bearer.starts_with(PREFIX) {
        return Ok(None);
    }
    let h = hash_key(bearer.trim());
    let row = sqlx::query_as::<_, ApiKey>(
        "update api_keys set last_used_at = now() where key_hash = $1 and revoked_at is null returning *",
    )
    .bind(h)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list(pool: &PgPool, household_id: Uuid) -> sqlx::Result<Vec<ApiKey>> {
    sqlx::query_as::<_, ApiKey>(
        "select * from api_keys where household_id = $1 order by created_at desc",
    )
    .bind(household_id)
    .fetch_all(pool)
    .await
}

pub async fn revoke(pool: &PgPool, household_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let n = sqlx::query("update api_keys set revoked_at = now() where household_id = $1 and id = $2 and revoked_at is null")
        .bind(household_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_shape() {
        let k = generate();
        assert!(k.plaintext.starts_with("hearth_"));
        assert_eq!(k.prefix.len(), 8);
        assert_eq!(k.hash.len(), 64);
        assert_eq!(hash_key(&k.plaintext), k.hash);
    }
}
