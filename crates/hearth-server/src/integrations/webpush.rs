//! Web Push: VAPID (RFC 8292) and `aes128gcm` message encryption (RFC 8291 /
//! RFC 8188), implemented directly on p256, HKDF and AES-GCM. No third-party
//! push service beyond the browser vendor's endpoint.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Key, Nonce};
use base64::Engine;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use sha2::Sha256;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

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
    Ok((
        B64.encode(public.as_bytes()),
        B64.encode(signing.to_bytes()),
    ))
}

pub struct Vapid {
    pub public_b64: String,
    signing: SigningKey,
    pub contact: String,
}

impl Vapid {
    pub fn new(public_b64: &str, private_b64: &str, contact: &str) -> anyhow::Result<Vapid> {
        let private = B64.decode(private_b64)?;
        let signing = SigningKey::from_slice(&private)
            .map_err(|_| anyhow::anyhow!("bad VAPID private key"))?;
        Ok(Vapid {
            public_b64: public_b64.to_string(),
            signing,
            contact: contact.to_string(),
        })
    }

    /// `vapid t=<jwt>, k=<public>` for one push-service origin.
    fn authorization(&self, audience: &str, now: i64) -> String {
        let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims =
            serde_json::json!({ "aud": audience, "exp": now + 12 * 3600, "sub": self.contact });
        let payload = B64.encode(claims.to_string().as_bytes());
        let signing_input = format!("{header}.{payload}");
        let sig: Signature = self.signing.sign(signing_input.as_bytes());
        format!(
            "vapid t={}.{}, k={}",
            signing_input,
            B64.encode(sig.to_bytes()),
            self.public_b64
        )
    }
}

/// Encrypt `plaintext` for a subscription (`p256dh`, `auth` as the browser gave them).
pub fn encrypt(p256dh_b64: &str, auth_b64: &str, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let ua_public_bytes = B64.decode(p256dh_b64)?;
    let auth = B64.decode(auth_b64)?;
    let ua_public =
        PublicKey::from_sec1_bytes(&ua_public_bytes).map_err(|_| anyhow::anyhow!("bad p256dh"))?;

    let as_secret = loop {
        let bytes: [u8; 32] = rand::random();
        if let Ok(k) = SecretKey::from_slice(&bytes) {
            break k;
        }
    };
    let as_public = as_secret.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua_public.as_affine());

    // IKM = HKDF(auth, ecdh_secret, "WebPush: info" ‖ 0x00 ‖ ua_public ‖ as_public)
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(&ua_public_bytes);
    info.extend_from_slice(as_public.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&auth), shared.raw_secret_bytes())
        .expand(&info, &mut ikm)
        .map_err(|_| anyhow::anyhow!("hkdf"))?;

    let salt: [u8; 16] = rand::random();
    let prk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut cek = [0u8; 16];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .map_err(|_| anyhow::anyhow!("hkdf"))?;
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce)
        .map_err(|_| anyhow::anyhow!("hkdf"))?;

    let mut record = plaintext.to_vec();
    record.push(0x02); // last-record delimiter
    let key: &Key<Aes128Gcm> = (&cek).into();
    let nonce: &Nonce<<Aes128Gcm as aes_gcm::AeadCore>::NonceSize> = (&nonce).into();
    let cipher = Aes128Gcm::new(key);
    let ciphertext = cipher
        .encrypt(nonce, record.as_slice())
        .map_err(|_| anyhow::anyhow!("aes-gcm"))?;

    // aes128gcm header: salt(16) ‖ rs(4) ‖ idlen(1) ‖ keyid(65)
    let rs: u32 = 4096;
    let mut out = Vec::with_capacity(16 + 4 + 1 + 65 + ciphertext.len());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&rs.to_be_bytes());
    out.push(65);
    out.extend_from_slice(as_public.as_bytes());
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub enum Outcome {
    Sent,
    /// 404 / 410: the subscription is dead; drop it.
    Gone,
    Failed(String),
}

#[allow(clippy::too_many_arguments)]
pub async fn send(
    client: &reqwest::Client,
    vapid: &Vapid,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    payload: &[u8],
    ttl_seconds: u32,
    urgency: &str,
) -> Outcome {
    let body = match encrypt(p256dh, auth, payload) {
        Ok(b) => b,
        Err(e) => return Outcome::Failed(format!("encrypt: {e}")),
    };
    let audience = match url_origin(endpoint) {
        Some(a) => a,
        None => return Outcome::Failed("bad endpoint".into()),
    };
    let res = client
        .post(endpoint)
        .header("Content-Encoding", "aes128gcm")
        .header("Content-Type", "application/octet-stream")
        .header("TTL", ttl_seconds.to_string())
        .header("Urgency", urgency)
        .header(
            "Authorization",
            vapid.authorization(&audience, chrono::Utc::now().timestamp()),
        )
        .body(body)
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => Outcome::Sent,
        Ok(r) if r.status().as_u16() == 404 || r.status().as_u16() == 410 => Outcome::Gone,
        Ok(r) => {
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            Outcome::Failed(format!(
                "{status}: {}",
                text.chars().take(200).collect::<String>()
            ))
        }
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

fn url_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?;
    Some(format!("{scheme}://{host}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vapid_shapes() {
        let (public, private) = generate_vapid().unwrap();
        assert_eq!(public.len(), 87); // 65 bytes → 87 chars base64url
        assert_eq!(private.len(), 43); // 32 bytes → 43 chars
        let v = Vapid::new(&public, &private, "mailto:owner@example.com").unwrap();
        let auth = v.authorization("https://push.example", 1_700_000_000);
        assert!(auth.starts_with("vapid t=") && auth.contains(", k="));
    }

    #[test]
    fn encrypts_with_header() {
        // A throwaway "browser" keypair.
        let (ua_pub, _) = generate_vapid().unwrap();
        let auth: [u8; 16] = rand::random();
        let out = encrypt(&ua_pub, &B64.encode(auth), b"hello").unwrap();
        assert_eq!(&out[16..20], &4096u32.to_be_bytes());
        assert_eq!(out[20], 65);
        assert_eq!(out.len(), 86 + 5 + 1 + 16); // header + plaintext + delimiter + tag
    }

    #[test]
    fn origin() {
        assert_eq!(
            url_origin("https://fcm.googleapis.com/fcm/send/abc").as_deref(),
            Some("https://fcm.googleapis.com")
        );
    }
}
