//! Email over SMTP (STARTTLS), plain text.

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::SmtpConfig;

pub async fn send(cfg: &SmtpConfig, subject: &str, body: &str) -> anyhow::Result<String> {
    let from: Mailbox = cfg
        .from
        .parse()
        .map_err(|e| anyhow::anyhow!("SMTP_FROM: {e}"))?;
    let to: Mailbox = cfg
        .alert_to
        .parse()
        .map_err(|e| anyhow::anyhow!("ALERT_EMAIL_TO: {e}"))?;
    let msg = Message::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .body(body.to_string())?;
    let mut builder =
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?.port(cfg.port);
    if let (Some(u), Some(p)) = (&cfg.user, &cfg.pass) {
        builder = builder.credentials(Credentials::new(u.clone(), p.clone()));
    }
    let transport = builder.build();
    let res = transport.send(msg).await?;
    Ok(res.message().collect::<Vec<_>>().join(" "))
}
