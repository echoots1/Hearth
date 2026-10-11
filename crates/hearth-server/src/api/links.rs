//! One-tap links from notifications: `/api/n/{token}` acknowledges or snoozes
//! an alert. The token is signed with `APP_SECRET` and expires; no session needed.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use chrono::{Duration, Utc};

use super::AppState;
use crate::auth::actions::{self, Action};
use crate::services::{self, Actor};

/// Minimal HTML escaping for the three values the page interpolates.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn page(title: &str, body: &str, app_url: &str) -> Html<String> {
    let (title, body, app_url) = (esc(title), esc(body), esc(app_url));
    Html(format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title} · Hearth</title>
<style>body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#f4f1ec;color:#1c1917;font:600 16px/1.4 Manrope,system-ui,sans-serif}}main{{background:#fff;border-radius:20px;padding:28px 24px;max-width:360px;text-align:center;box-shadow:0 1px 2px rgba(28,25,23,.06)}}h1{{font-size:20px;margin:0 0 8px}}p{{margin:0 0 16px;color:#6b6560}}a{{display:inline-block;background:#1c1917;color:#f4f1ec;text-decoration:none;border-radius:14px;padding:12px 18px;font-weight:700}}</style></head>
<body><main><h1>{title}</h1><p>{body}</p><a href="{app_url}/activity">Open Hearth</a></main></body></html>"#
    ))
}

pub async fn act(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let app_url = state.runtime().app_url.clone();
    let Some((alert_id, action)) = actions::verify(&state.config.app_secret, &token, Utc::now())
    else {
        return (
            StatusCode::BAD_REQUEST,
            page(
                "That link has expired",
                "Open Hearth to see the alert.",
                &app_url,
            ),
        )
            .into_response();
    };
    let ctx = match state.ctx().await {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                page("Something went wrong", "Try again from the app.", &app_url),
            )
                .into_response();
        }
    };
    let actor = Actor::System;
    let result = match action {
        Action::Ack => services::alerts::ack(&ctx, &actor, alert_id)
            .await
            .map(|_| ("Acknowledged", "Hearth will stop nudging about this one.")),
        Action::Snooze => {
            services::alerts::snooze(&ctx, &actor, alert_id, Utc::now() + Duration::days(1))
                .await
                .map(|_| ("Snoozed", "Hearth will bring it back tomorrow."))
        }
    };
    match result {
        Ok((title, body)) => page(title, body, &app_url).into_response(),
        Err(services::Error::Invalid(m)) => page("Already handled", &m, &app_url).into_response(),
        Err(services::Error::NotFound) => (
            StatusCode::NOT_FOUND,
            page("Alert not found", "It may have been resolved.", &app_url),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            page("Something went wrong", "Try again from the app.", &app_url),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::esc;

    #[test]
    fn escapes_html() {
        assert_eq!(
            esc(r#"<a href="x">&'"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
        assert_eq!(esc("plain"), "plain");
    }
}
