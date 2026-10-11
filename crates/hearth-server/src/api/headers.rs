//! Security headers on every response.
//!
//! The UI is a Vite build with no inline scripts and no third-party origins
//! except Plaid Link (loaded from `cdn.plaid.com`, which opens its own
//! iframe), so the policy can be strict. Inline *styles* stay allowed: React
//! sets a few `style=` attributes and the one-tap page carries its own
//! `<style>`.

use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

use super::AppState;

pub const CSP: &str = "default-src 'self'; \
script-src 'self' https://cdn.plaid.com; \
style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; \
font-src 'self'; \
connect-src 'self' https://*.plaid.com; \
frame-src https://*.plaid.com; \
worker-src 'self'; \
manifest-src 'self'; \
base-uri 'self'; \
form-action 'self'; \
object-src 'none'; \
frame-ancestors 'none'";

pub async fn security_headers(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let https = state.runtime().is_https();
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    let set = |h: &mut axum::http::HeaderMap, name: header::HeaderName, v: &'static str| {
        if !h.contains_key(&name) {
            h.insert(name, HeaderValue::from_static(v));
        }
    };
    set(h, header::CONTENT_SECURITY_POLICY, CSP);
    set(h, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(h, header::X_FRAME_OPTIONS, "DENY");
    // Keys and signed tokens live in paths (`/mcp/<key>`, `/api/n/<token>`).
    set(h, header::REFERRER_POLICY, "no-referrer");
    set(
        h,
        header::HeaderName::from_static("permissions-policy"),
        "camera=(), microphone=(), geolocation=(), payment=()",
    );
    set(
        h,
        header::HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    );
    if https {
        set(h, header::STRICT_TRANSPORT_SECURITY, "max-age=31536000");
    }
    res
}
