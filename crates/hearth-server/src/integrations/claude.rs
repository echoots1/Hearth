//! The Claude Messages API over raw HTTPS (there is no Rust SDK), with the
//! tool definitions taken from Hearth's registry. Costs are computed in
//! integer cents from the usage block.

use hearth_core::money::Cents;
use serde::Deserialize;
use serde_json::{Value, json};

const API: &str = "https://api.anthropic.com/v1/messages";
const VERSION: &str = "2023-06-01";

pub struct Claude {
    client: reqwest::Client,
    api_key: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub cache_read_input_tokens: i64,
    #[serde(default)]
    pub cache_creation_input_tokens: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    pub content: Vec<Value>,
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Usage,
}

pub struct Request<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub messages: &'a [Value],
    pub tools: &'a [Value],
    pub max_tokens: u32,
}

impl Claude {
    pub fn new(api_key: String) -> anyhow::Result<Claude> {
        Ok(Claude {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()?,
            api_key,
        })
    }

    pub async fn messages(&self, r: Request<'_>) -> anyhow::Result<Response> {
        let mut body = json!({
            "model": r.model,
            "max_tokens": r.max_tokens,
            "system": [{ "type": "text", "text": r.system, "cache_control": { "type": "ephemeral" } }],
            "messages": r.messages,
        });
        if !r.tools.is_empty() {
            body["tools"] = Value::Array(r.tools.to_vec());
        }
        let res = self
            .client
            .post(API)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            #[derive(Deserialize)]
            struct E {
                error: Option<Inner>,
            }
            #[derive(Deserialize)]
            struct Inner {
                #[serde(rename = "type")]
                kind: Option<String>,
                message: Option<String>,
            }
            let e: E = serde_json::from_str(&text).unwrap_or(E { error: None });
            let (kind, msg) = e
                .error
                .map(|i| (i.kind.unwrap_or_default(), i.message.unwrap_or_default()))
                .unwrap_or_default();
            anyhow::bail!(
                "Claude API {status} {kind}: {}",
                if msg.is_empty() {
                    text.chars().take(300).collect()
                } else {
                    msg
                }
            );
        }
        Ok(serde_json::from_str(&text)?)
    }
}

/// Cents per million tokens: (input, output, cache read, cache write).
fn prices(model: &str) -> (i64, i64, i64, i64) {
    if model.contains("haiku") {
        (10, 50, 1, 13)
    } else if model.contains("sonnet") {
        (200, 1000, 20, 250)
    } else if model.contains("fable") || model.contains("mythos") {
        (1000, 5000, 25, 1250)
    } else {
        // Opus 5.5 and anything unknown: price as Opus.
        (400, 2000, 20, 500)
    }
}

fn per_mtok(tokens: i64, cents_per_mtok: i64) -> i64 {
    (tokens.max(0) * cents_per_mtok + 999_999) / 1_000_000
}

/// What one response cost, rounded up to the cent per component.
pub fn cost_cents(model: &str, u: &Usage) -> Cents {
    let (i, o, cr, cw) = prices(model);
    Cents(
        per_mtok(u.input_tokens, i)
            + per_mtok(u.output_tokens, o)
            + per_mtok(u.cache_read_input_tokens, cr)
            + per_mtok(u.cache_creation_input_tokens, cw),
    )
}

/// Registry tool names use dots; the API allows `[a-zA-Z0-9_-]`.
pub fn api_tool_name(name: &str) -> String {
    name.replace('.', "__")
}

pub fn registry_tool_name(name: &str) -> String {
    name.replace("__", ".")
}

/// Every registry tool as a Messages API tool definition.
pub fn tool_definitions() -> Vec<Value> {
    crate::tools::all()
        .map(|t| {
            let mut schema = t.schema();
            if let Value::Object(m) = &mut schema {
                m.remove("$schema");
                m.remove("title");
            }
            json!({ "name": api_tool_name(t.name), "description": t.description, "input_schema": schema })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs_round_up_per_component() {
        let u = Usage {
            input_tokens: 1000,
            output_tokens: 1,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        // Opus: 1000 in × 400c/M = 0.4c → 1c; 1 out → 1c
        assert_eq!(cost_cents("claude-opus-5-5", &u), Cents(2));
        let big = Usage {
            input_tokens: 2_000_000,
            output_tokens: 100_000,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        assert_eq!(cost_cents("claude-haiku-5-5", &big), Cents(20 + 5));
    }

    #[test]
    fn tool_names_roundtrip() {
        assert_eq!(api_tool_name("budget.set_plan"), "budget__set_plan");
        assert_eq!(registry_tool_name("budget__set_plan"), "budget.set_plan");
        for t in crate::tools::all() {
            assert!(
                api_tool_name(t.name)
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            );
        }
    }
}
