//! VAPID key generation for Web Push. Delivery lands with the notify job.

use base64::Engine;
use p256::ecdsa::SigningKey;

/// Returns `(public_key_b64url, private_key_b64url)` as browsers and the push
/// protocol expect: uncompressed public point, raw 32-byte private scalar.
pub fn generate_vapid() -> anyhow::Result<(String, String)> {
    // Draw scalars until one is valid for the curve (practically the first).
    let signing = loop {
        let bytes: [u8; 32] = rand::random();
        if let Ok(k) = SigningKey::from_slice(&bytes) {
            break k;
        }
    };
    let public = signing.verifying_key().to_encoded_point(false);
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    Ok((
        b64.encode(public.as_bytes()),
        b64.encode(signing.to_bytes()),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn vapid_shapes() {
        let (public, private) = super::generate_vapid().unwrap();
        assert_eq!(public.len(), 87); // 65 bytes → 87 chars base64url
        assert_eq!(private.len(), 43); // 32 bytes → 43 chars
    }
}
