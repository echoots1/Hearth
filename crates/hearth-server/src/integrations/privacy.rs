//! Privacy.com: virtual cards as accounts; card transactions and declines.
//!
//! Shapes follow the public v1 API (`Authorization: api-key …`, paged
//! `/v1/cards` and `/v1/transactions`); unknown fields are ignored and
//! missing ones default, so a field the docs add later doesn't break a sync.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::config::PrivacyConfig;

pub struct Privacy {
    client: reqwest::Client,
    cfg: PrivacyConfig,
    base: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Card {
    pub token: String,
    #[serde(default)]
    pub last_four: String,
    #[serde(default)]
    pub memo: String,
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Merchant {
    #[serde(default)]
    pub descriptor: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CardRef {
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Transaction {
    pub token: String,
    /// Cents, positive for charges.
    #[serde(default)]
    pub amount: i64,
    #[serde(default)]
    pub settled_amount: Option<i64>,
    /// PENDING | VOIDED | SETTLING | SETTLED | BOUNCED | DECLINED
    #[serde(default)]
    pub status: String,
    /// APPROVED | DECLINED | … (decline reasons such as CARD_PAUSED)
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub merchant: Merchant,
    #[serde(default)]
    pub card: CardRef,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct Page<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
    #[serde(default)]
    page: u32,
    #[serde(default)]
    total_pages: u32,
}

impl Privacy {
    pub fn new(cfg: &PrivacyConfig) -> anyhow::Result<Privacy> {
        let base = match cfg.env.as_str() {
            "sandbox" => "https://sandbox.privacy.com/v1",
            "production" => "https://api.privacy.com/v1",
            other => anyhow::bail!("PRIVACY_ENV={other:?} must be sandbox or production"),
        };
        Ok(Privacy {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            cfg: cfg.clone(),
            base: base.to_string(),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> anyhow::Result<T> {
        let res = self
            .client
            .get(format!("{}{}", self.base, path))
            .header("Authorization", format!("api-key {}", self.cfg.api_key))
            .query(query)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            anyhow::bail!(
                "privacy.com {} {}: {}",
                path,
                status,
                text.chars().take(200).collect::<String>()
            );
        }
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn cards(&self) -> anyhow::Result<Vec<Card>> {
        let mut out = Vec::new();
        let mut page = 1u32;
        loop {
            let p: Page<Card> = self
                .get(
                    "/cards",
                    &[("page", page.to_string()), ("page_size", "100".into())],
                )
                .await?;
            out.extend(p.data);
            if p.page == 0 || p.page >= p.total_pages || page > 50 {
                break;
            }
            page += 1;
        }
        Ok(out)
    }

    /// Transactions created on or after `begin` (ISO date), all pages.
    pub async fn transactions(&self, begin: &str) -> anyhow::Result<Vec<Transaction>> {
        let mut out = Vec::new();
        let mut page = 1u32;
        loop {
            let p: Page<Transaction> = self
                .get(
                    "/transactions",
                    &[
                        ("begin", begin.to_string()),
                        ("page", page.to_string()),
                        ("page_size", "100".into()),
                    ],
                )
                .await?;
            out.extend(p.data);
            if p.page == 0 || p.page >= p.total_pages || page > 200 {
                break;
            }
            page += 1;
        }
        Ok(out)
    }
}
