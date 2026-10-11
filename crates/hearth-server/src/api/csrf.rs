//! Same-origin guard for cookie-authenticated writes.
//!
//! The session cookie is `SameSite=Lax`, which already keeps it off
//! cross-site POSTs in current browsers. This is the second layer: a
//! state-changing `/api` request that a browser marks as coming from another
//! site (`Sec-Fetch-Site`), or whose `Origin` does not match the `Host` it
//! was sent to, is refused. Requests carrying an API key (`Authorization`)
//! are not browser sessions and pass; so do the signed one-tap links (GET)
//! and the Plaid webhook, which has its own signature.

use axum::extract::Request;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn same_origin_guard(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let exempt = !path.starts_with("/api/")
        || path == "/api/plaid/webhook"
        || matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        || req.headers().contains_key(header::AUTHORIZATION);
    if !exempt && cross_site(req.headers()) {
        tracing::warn!(path, "refused a cross-site write");
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({ "error": "cross-site request refused" })),
        )
            .into_response();
    }
    next.run(req).await
}

/// `host:port` lowercased, with the scheme's default port removed.
fn authority(s: &str, default_port: Option<&str>) -> String {
    let s = s.trim().to_ascii_lowercase();
    if let Some(d) = default_port
        && let Some(h) = s.strip_suffix(&format!(":{d}"))
    {
        return h.to_string();
    }
    s
}

/// Does the browser say this request came from somewhere else?
pub fn cross_site(headers: &HeaderMap) -> bool {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        // `same-origin` and `none` (a navigation the person started) are fine;
        // `same-site` is another subdomain, which this app never is.
        if matches!(site.trim(), "cross-site" | "same-site") {
            return true;
        }
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        // Browsers always send Origin on POST/PUT/DELETE; its absence means a
        // non-browser client, which the cookie never reaches anyway.
        return false;
    };
    let origin = origin.trim();
    if origin == "null" {
        return true;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return true;
    };
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "https" => Some("443"),
        "http" => Some("80"),
        _ => None,
    };
    let origin_host = authority(rest.split('/').next().unwrap_or(""), default_port);
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = authority(host, default_port);
    origin_host.is_empty() || origin_host != host
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn h(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut m = HeaderMap::new();
        for (k, v) in pairs {
            m.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        m
    }

    #[test]
    fn same_origin_passes() {
        assert!(!cross_site(&h(&[
            ("host", "money.example.net"),
            ("origin", "https://money.example.net"),
            ("sec-fetch-site", "same-origin"),
        ])));
        assert!(!cross_site(&h(&[
            ("host", "money.example.net"),
            ("origin", "https://money.example.net:443"),
        ])));
        assert!(!cross_site(&h(&[
            ("host", "192.168.1.20:3000"),
            ("origin", "http://192.168.1.20:3000"),
        ])));
        assert!(!cross_site(&h(&[
            ("host", "Money.Example.net"),
            ("origin", "https://money.example.NET"),
        ])));
    }

    #[test]
    fn non_browser_without_origin_passes() {
        assert!(!cross_site(&h(&[("host", "money.example.net")])));
    }

    #[test]
    fn cross_site_is_refused() {
        assert!(cross_site(&h(&[
            ("host", "money.example.net"),
            ("origin", "https://evil.example"),
        ])));
        assert!(cross_site(&h(&[
            ("host", "money.example.net"),
            ("origin", "https://money.example.net"),
            ("sec-fetch-site", "cross-site"),
        ])));
        assert!(cross_site(&h(&[
            ("host", "money.example.net"),
            ("origin", "https://other.example.net"),
            ("sec-fetch-site", "same-site"),
        ])));
        assert!(cross_site(&h(&[
            ("host", "money.example.net"),
            ("origin", "null"),
        ])));
        assert!(cross_site(&h(&[
            ("host", "money.example.net:3000"),
            ("origin", "http://money.example.net:8080"),
        ])));
        assert!(cross_site(&h(&[("origin", "https://money.example.net")])));
    }
}
