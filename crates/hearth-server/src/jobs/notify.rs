//! `notify`: tell the humans. New alerts once, nudges on the nag schedule,
//! questions and proposals once; quiet hours honored (critical may override).
//! Channels: Web Push (always, when a device subscribed), ntfy and email
//! (when configured). Never interprets a reply.

use chrono::{Duration, Utc};
use hearth_core::alerts::{Severity, in_quiet_hours, nudge_due};
use serde_json::{Value, json};

use crate::api::AppState;
use crate::auth::actions::{self, Action};
use crate::db::models::{Alert, AlertSeverity, NotificationChannel, Question, QuestionKind};
use crate::integrations::{ntfy, smtp, webpush};
use crate::services::{self, Ctx, notifications, secrets};

struct Channels {
    client: reqwest::Client,
    vapid: Option<webpush::Vapid>,
    push_subs: Vec<crate::db::models::PushSubscription>,
    ntfy: Option<crate::config::NtfyConfig>,
    smtp: Option<crate::config::SmtpConfig>,
}

struct Msg {
    title: String,
    body: String,
    url: String,
    tag: String,
    severity: Severity,
    /// (key, label, url) one-tap actions.
    actions: Vec<(&'static str, &'static str, String)>,
}

pub async fn run(state: AppState) -> anyhow::Result<Value> {
    let ctx = state.ctx().await?;
    let settings = services::settings::load(&ctx).await?;
    let push_subs = services::push::all(&ctx).await?;
    let vapid = match (
        secrets::get(&ctx, secrets::VAPID_PUBLIC_KEY).await?,
        secrets::get(&ctx, secrets::VAPID_PRIVATE_KEY).await?,
    ) {
        (Some(public), Some(private)) if !push_subs.is_empty() => {
            let contact = owner_contact(&ctx).await?;
            Some(webpush::Vapid::new(&public, &private, &contact)?)
        }
        _ => None,
    };
    let channels = Channels {
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?,
        vapid,
        push_subs,
        ntfy: services::integrations::ntfy(&ctx, &state.config)
            .await?
            .map(|(c, _)| c),
        smtp: services::integrations::smtp(&ctx, &state.config)
            .await?
            .map(|(c, _)| c),
    };
    let any_channel =
        channels.vapid.is_some() || channels.ntfy.is_some() || channels.smtp.is_some();
    if !any_channel {
        return Ok(json!({ "skipped": "no channel configured and no device subscribed" }));
    }

    let now = Utc::now();
    let mut sent = 0;
    let mut deferred = 0;
    let mut failed = 0;

    // New alerts.
    for a in notifications::alerts_to_announce(&ctx).await? {
        let sev = severity(a.severity);
        if in_quiet_hours(&settings.notifications, ctx.tz, now, sev) {
            deferred += 1;
            continue;
        }
        let m = alert_message(&state, &a, false);
        let (ok, bad) = deliver(&ctx, &channels, &m, Some(a.id), None, "initial").await;
        sent += ok;
        failed += bad;
        if ok > 0 {
            notifications::mark_announced(&ctx, a.id, now).await?;
        }
    }

    // Nudges.
    for a in notifications::alerts_to_nudge(&ctx).await? {
        let Some(notified_at) = a.notified_at else {
            continue;
        };
        if !nudge_due(
            &settings.nag,
            notified_at,
            a.last_nudged_at,
            a.nudge_count,
            a.snoozed_until,
            now,
        ) {
            continue;
        }
        let sev = severity(a.severity);
        if in_quiet_hours(&settings.notifications, ctx.tz, now, sev) {
            deferred += 1;
            continue;
        }
        let m = alert_message(&state, &a, true);
        let (ok, bad) = deliver(&ctx, &channels, &m, Some(a.id), None, "nudge").await;
        sent += ok;
        failed += bad;
        if ok > 0 {
            notifications::mark_nudged(&ctx, a.id, now).await?;
        }
    }

    // Questions and proposals.
    for q in notifications::questions_to_announce(&ctx).await? {
        if in_quiet_hours(&settings.notifications, ctx.tz, now, Severity::Info) {
            deferred += 1;
            continue;
        }
        let m = question_message(&state, &q);
        let kind = if q.kind == QuestionKind::Proposal {
            "proposal"
        } else {
            "question"
        };
        let (ok, bad) = deliver(&ctx, &channels, &m, None, Some(q.id), kind).await;
        sent += ok;
        failed += bad;
    }

    Ok(
        json!({ "sent": sent, "failed": failed, "deferred_quiet_hours": deferred, "push_devices": channels.push_subs.len() }),
    )
}

fn severity(s: AlertSeverity) -> Severity {
    match s {
        AlertSeverity::Info => Severity::Info,
        AlertSeverity::Warn => Severity::Warn,
        AlertSeverity::Critical => Severity::Critical,
    }
}

async fn owner_contact(ctx: &Ctx) -> anyhow::Result<String> {
    let email: Option<String> = sqlx::query_scalar(
        "select email from users where household_id = $1 order by case role when 'owner' then 0 else 1 end, created_at limit 1",
    )
    .bind(ctx.household_id)
    .fetch_optional(&ctx.pool)
    .await?;
    Ok(format!(
        "mailto:{}",
        email.unwrap_or_else(|| "owner@example.com".into())
    ))
}

fn alert_message(state: &AppState, a: &Alert, nudge: bool) -> Msg {
    let runtime = state.runtime();
    let base = &runtime.app_url;
    // One-tap links are bearer tokens for one alert; a day is plenty, the nag
    // schedule sends fresh ones.
    let exp = Utc::now() + Duration::hours(24);
    let ack = format!(
        "{base}/api/n/{}",
        actions::sign(&state.config.app_secret, a.id, Action::Ack, exp)
    );
    let snooze = format!(
        "{base}/api/n/{}",
        actions::sign(&state.config.app_secret, a.id, Action::Snooze, exp)
    );
    let prefix = match (a.severity, nudge) {
        (AlertSeverity::Critical, _) => "Hearth · critical",
        (_, true) => "Hearth · still open",
        _ => "Hearth",
    };
    Msg {
        title: format!("{prefix}: {}", a.title),
        body: a.detail.clone(),
        url: format!("{base}/activity"),
        tag: format!("alert-{}", a.id),
        severity: severity(a.severity),
        actions: vec![
            ("ack", "Acknowledge", ack),
            ("snooze", "Snooze a day", snooze),
        ],
    }
}

fn question_message(state: &AppState, q: &Question) -> Msg {
    let runtime = state.runtime();
    let base = &runtime.app_url;
    let (title, body) = match (&q.kind, &q.proposal) {
        (QuestionKind::Proposal, Some(p)) => (
            "Hearth: Claude proposes".to_string(),
            format!(
                "{}\n{}",
                p.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
                q.question
            ),
        ),
        _ => ("Hearth: Claude asks".to_string(), q.question.clone()),
    };
    Msg {
        title,
        body,
        url: format!("{base}/activity"),
        tag: format!("question-{}", q.id),
        severity: Severity::Info,
        actions: vec![],
    }
}

/// Send one message over every channel; returns (successes, failures).
async fn deliver(
    ctx: &Ctx,
    ch: &Channels,
    m: &Msg,
    alert_id: Option<uuid::Uuid>,
    question_id: Option<uuid::Uuid>,
    kind: &str,
) -> (usize, usize) {
    let mut ok = 0;
    let mut bad = 0;
    let rec = |channel: NotificationChannel,
               good: bool,
               target: Option<String>,
               external_id: Option<String>,
               error: Option<String>| {
        notifications::record(
            ctx,
            notifications::Record {
                alert_id,
                question_id,
                channel,
                kind,
                ok: good,
                target,
                external_id,
                error,
            },
        )
    };

    if let Some(vapid) = &ch.vapid {
        let payload = json!({
            "title": m.title,
            "body": m.body,
            "url": m.url,
            "tag": m.tag,
            "actions": m.actions.iter().map(|(k, l, _)| json!({ "key": k, "label": l })).collect::<Vec<_>>(),
            "action_urls": m.actions.iter().map(|(k, _, u)| (k.to_string(), u.clone())).collect::<std::collections::BTreeMap<_, _>>(),
        })
        .to_string();
        let urgency = match m.severity {
            Severity::Critical => "high",
            Severity::Warn => "normal",
            Severity::Info => "low",
        };
        for s in &ch.push_subs {
            match webpush::send(
                &ch.client,
                vapid,
                &s.endpoint,
                &s.p256dh,
                &s.auth,
                payload.as_bytes(),
                24 * 3600,
                urgency,
            )
            .await
            {
                webpush::Outcome::Sent => {
                    ok += 1;
                    let _ = services::push::mark_success(ctx, s.id).await;
                    let _ = rec(
                        NotificationChannel::Webpush,
                        true,
                        Some(s.id.to_string()),
                        None,
                        None,
                    )
                    .await;
                }
                webpush::Outcome::Gone => {
                    bad += 1;
                    let _ = services::push::mark_failure(ctx, s.id, true).await;
                    let _ = rec(
                        NotificationChannel::Webpush,
                        false,
                        Some(s.id.to_string()),
                        None,
                        Some("subscription gone".into()),
                    )
                    .await;
                }
                webpush::Outcome::Failed(e) => {
                    bad += 1;
                    let _ = services::push::mark_failure(ctx, s.id, false).await;
                    let _ = rec(
                        NotificationChannel::Webpush,
                        false,
                        Some(s.id.to_string()),
                        None,
                        Some(e),
                    )
                    .await;
                }
            }
        }
    }

    if let Some(cfg) = &ch.ntfy {
        let actions: Vec<(&str, &str)> =
            m.actions.iter().map(|(_, l, u)| (*l, u.as_str())).collect();
        let msg = ntfy::Message {
            title: &m.title,
            body: &m.body,
            url: Some(&m.url),
            priority: match m.severity {
                Severity::Critical => 5,
                Severity::Warn => 4,
                Severity::Info => 3,
            },
            tag: Some(match m.severity {
                Severity::Critical => "rotating_light",
                Severity::Warn => "warning",
                Severity::Info => "fire",
            }),
            actions,
        };
        match ntfy::send(&ch.client, cfg, &msg).await {
            Ok(id) => {
                ok += 1;
                let _ = rec(
                    NotificationChannel::Ntfy,
                    true,
                    Some(cfg.topic.clone()),
                    Some(id),
                    None,
                )
                .await;
            }
            Err(e) => {
                bad += 1;
                let _ = rec(
                    NotificationChannel::Ntfy,
                    false,
                    Some(cfg.topic.clone()),
                    None,
                    Some(e.to_string()),
                )
                .await;
            }
        }
    }

    if let Some(cfg) = &ch.smtp {
        let mut body = format!("{}\n\nOpen Hearth: {}\n", m.body, m.url);
        for (_, label, url) in &m.actions {
            body.push_str(&format!("{label}: {url}\n"));
        }
        match smtp::send(cfg, &m.title, &body).await {
            Ok(id) => {
                ok += 1;
                let _ = rec(
                    NotificationChannel::Smtp,
                    true,
                    Some(cfg.alert_to.clone()),
                    Some(id),
                    None,
                )
                .await;
            }
            Err(e) => {
                bad += 1;
                let _ = rec(
                    NotificationChannel::Smtp,
                    false,
                    Some(cfg.alert_to.clone()),
                    None,
                    Some(e.to_string()),
                )
                .await;
            }
        }
    }
    (ok, bad)
}
