//! A small fixed-window rate limiter keyed by client address. In-memory and
//! per process, which is right for a one-household server: it stops password
//! guessing and runaway MCP clients, nothing more.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::api::AppState;

#[derive(Clone)]
pub struct Limiter {
    inner: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>,
    pub max: u32,
    pub window: Duration,
}

impl Limiter {
    pub fn new(max: u32, window: Duration) -> Limiter {
        Limiter {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max,
            window,
        }
    }

    /// `Ok(())` to proceed, `Err(retry_after_secs)` when over the limit.
    pub fn check(&self, ip: IpAddr, now: Instant) -> Result<(), u64> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > 10_000 {
            map.retain(|_, (start, _)| now.duration_since(*start) < self.window);
        }
        let entry = map.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        if entry.1 >= self.max {
            let left = self.window.saturating_sub(now.duration_since(entry.0));
            return Err(left.as_secs().max(1));
        }
        entry.1 += 1;
        Ok(())
    }
}

/// The client address: the hop the trusted proxy appended to
/// `X-Forwarded-For` when `TRUST_PROXY` is on, otherwise the socket peer.
pub fn client_ip(state: &AppState, req: &Request) -> IpAddr {
    if state.runtime().trust_proxy
        && let Some(ip) = forwarded_client(req.headers().get("x-forwarded-for"))
    {
        return ip;
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// The **last** `X-Forwarded-For` entry. A proxy appends the address it saw
/// to whatever the client sent, so only the last hop is the proxy's word;
/// everything before it is attacker-controlled and would let one client
/// spoof a fresh rate-limit bucket per request.
pub fn forwarded_client(header: Option<&HeaderValue>) -> Option<IpAddr> {
    header?
        .to_str()
        .ok()?
        .rsplit(',')
        .next()?
        .trim()
        .trim_matches('"')
        .parse()
        .ok()
}

/// Middleware: `State((AppState, Limiter))`.
pub async fn limit(
    State((state, limiter)): State<(AppState, Limiter)>,
    req: Request,
    next: Next,
) -> Response {
    let ip = client_ip(&state, &req);
    match limiter.check(ip, Instant::now()) {
        Ok(()) => next.run(req).await,
        Err(retry) => {
            tracing::warn!(%ip, path = %crate::mcp::redact_path(req.uri().path()), "rate limited");
            let mut r = (
                StatusCode::TOO_MANY_REQUESTS,
                axum::Json(serde_json::json!({ "error": "too many requests; slow down" })),
            )
                .into_response();
            if let Ok(v) = HeaderValue::from_str(&retry.to_string()) {
                r.headers_mut().insert(header::RETRY_AFTER, v);
            }
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_and_reset() {
        let l = Limiter::new(3, Duration::from_secs(60));
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let t0 = Instant::now();
        assert!(l.check(ip, t0).is_ok());
        assert!(l.check(ip, t0).is_ok());
        assert!(l.check(ip, t0).is_ok());
        assert_eq!(l.check(ip, t0), Err(60));
        let other: IpAddr = "10.0.0.2".parse().unwrap();
        assert!(l.check(other, t0).is_ok());
        assert!(l.check(ip, t0 + Duration::from_secs(61)).is_ok());
    }

    #[test]
    fn forwarded_for_takes_the_proxy_appended_hop() {
        let h = HeaderValue::from_static("1.2.3.4, 10.0.0.9");
        assert_eq!(
            forwarded_client(Some(&h)),
            Some("10.0.0.9".parse::<IpAddr>().unwrap())
        );
        let spoofed = HeaderValue::from_static("203.0.113.7,2001:db8::1");
        assert_eq!(
            forwarded_client(Some(&spoofed)),
            Some("2001:db8::1".parse::<IpAddr>().unwrap())
        );
        let single = HeaderValue::from_static("192.0.2.1");
        assert_eq!(
            forwarded_client(Some(&single)),
            Some("192.0.2.1".parse::<IpAddr>().unwrap())
        );
        assert_eq!(
            forwarded_client(Some(&HeaderValue::from_static("junk"))),
            None
        );
        assert_eq!(forwarded_client(None), None);
    }
}
