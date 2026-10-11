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

/// The client address: the first `X-Forwarded-For` hop when `TRUST_PROXY`
/// is on, otherwise the socket peer.
pub fn client_ip(state: &AppState, req: &Request) -> IpAddr {
    if state.runtime().trust_proxy
        && let Some(v) = req.headers().get("x-forwarded-for")
        && let Ok(s) = v.to_str()
        && let Some(first) = s.split(',').next()
        && let Ok(ip) = first.trim().parse::<IpAddr>()
    {
        return ip;
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
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
}
