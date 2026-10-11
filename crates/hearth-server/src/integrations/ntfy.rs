//! ntfy: one JSON POST per message. <https://docs.ntfy.sh/publish/>

use serde::Serialize;

use crate::config::NtfyConfig;

#[derive(Serialize)]
struct Action<'a> {
    action: &'static str,
    label: &'a str,
    url: &'a str,
    clear: bool,
}

#[derive(Serialize)]
struct Body<'a> {
    topic: &'a str,
    title: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    click: Option<&'a str>,
    priority: u8,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    actions: Vec<Action<'a>>,
}

pub struct Message<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub url: Option<&'a str>,
    /// 1 (min) … 5 (max); 3 is default.
    pub priority: u8,
    pub tag: Option<&'a str>,
    /// (label, url) pairs rendered as one-tap buttons.
    pub actions: Vec<(&'a str, &'a str)>,
}

pub async fn send(
    client: &reqwest::Client,
    cfg: &NtfyConfig,
    m: &Message<'_>,
) -> anyhow::Result<String> {
    let body = Body {
        topic: &cfg.topic,
        title: m.title,
        message: m.body,
        click: m.url,
        priority: m.priority.clamp(1, 5),
        tags: m.tag.into_iter().collect(),
        actions: m
            .actions
            .iter()
            .map(|(label, url)| Action {
                action: "http",
                label,
                url,
                clear: true,
            })
            .collect(),
    };
    let mut req = client.post(cfg.url.trim_end_matches('/')).json(&body);
    if let Some(t) = &cfg.token {
        req = req.bearer_auth(t);
    }
    let res = req.send().await?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!(
            "ntfy {}: {}",
            status,
            text.chars().take(200).collect::<String>()
        );
    }
    let id = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(str::to_string))
        .unwrap_or_default();
    Ok(id)
}
