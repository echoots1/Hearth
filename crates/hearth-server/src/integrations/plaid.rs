//! Plaid: Link token, public-token exchange, `/transactions/sync`, item removal.
//! Amounts are parsed from their decimal text; no floats anywhere.

use hearth_core::money::Cents;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::config::PlaidConfig;

pub struct Plaid {
    client: reqwest::Client,
    cfg: PlaidConfig,
    base: String,
}

#[derive(Debug, Deserialize)]
struct PlaidError {
    error_type: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
}

/// Plaid sends amounts as JSON numbers; keep them as text and parse to cents.
fn cents(v: &Value) -> Option<Cents> {
    match v {
        Value::Number(n) => Cents::parse_decimal(&n.to_string()),
        Value::String(s) => Cents::parse_decimal(s),
        _ => None,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaidAccount {
    pub account_id: String,
    pub name: String,
    pub official_name: Option<String>,
    pub mask: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub subtype: Option<String>,
    #[serde(default)]
    pub balances: Value,
}

impl PlaidAccount {
    /// Signed the way Hearth stores it: liabilities (credit, loan) negative.
    pub fn current_balance_cents(&self) -> Option<Cents> {
        let c = cents(self.balances.get("current")?)?;
        Some(if self.kind == "credit" || self.kind == "loan" {
            -c
        } else {
            c
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaidTransaction {
    pub transaction_id: String,
    pub account_id: String,
    amount: Value,
    pub date: String,
    pub name: Option<String>,
    pub merchant_name: Option<String>,
    pub pending: bool,
    pub pending_transaction_id: Option<String>,
    #[serde(default)]
    pub original_description: Option<String>,
}

impl PlaidTransaction {
    /// Plaid: positive = money out. Hearth: negative = money out.
    pub fn amount_cents(&self) -> Option<Cents> {
        cents(&self.amount).map(|c| -c)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Removed {
    pub transaction_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SyncPage {
    #[serde(default)]
    pub added: Vec<PlaidTransaction>,
    #[serde(default)]
    pub modified: Vec<PlaidTransaction>,
    #[serde(default)]
    pub removed: Vec<Removed>,
    pub next_cursor: String,
    pub has_more: bool,
    #[serde(default)]
    pub accounts: Vec<PlaidAccount>,
}

impl Plaid {
    pub fn new(cfg: &PlaidConfig) -> anyhow::Result<Plaid> {
        let base = match cfg.env.as_str() {
            "sandbox" => "https://sandbox.plaid.com",
            "production" => "https://production.plaid.com",
            "development" => "https://development.plaid.com",
            other => anyhow::bail!("PLAID_ENV={other:?} must be sandbox or production"),
        };
        Ok(Plaid {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
            cfg: cfg.clone(),
            base: base.to_string(),
        })
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, mut body: Value) -> anyhow::Result<T> {
        if let Value::Object(m) = &mut body {
            m.insert(
                "client_id".into(),
                Value::String(self.cfg.client_id.clone()),
            );
            m.insert("secret".into(), Value::String(self.cfg.secret.clone()));
        }
        let res = self
            .client
            .post(format!("{}{}", self.base, path))
            .json(&body)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            let e: PlaidError = serde_json::from_str(&text).unwrap_or(PlaidError {
                error_type: None,
                error_code: None,
                error_message: None,
            });
            anyhow::bail!(
                "plaid {} {}: {} {}",
                path,
                status,
                e.error_code.or(e.error_type).unwrap_or_default(),
                e.error_message
                    .unwrap_or_else(|| text.chars().take(200).collect())
            );
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// A Link token for the browser; `user_id` is opaque to Plaid.
    pub async fn link_token(&self, user_id: &str, app_url: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct R {
            link_token: String,
        }
        let r: R = self
            .post(
                "/link/token/create",
                json!({
                    "client_name": "Hearth",
                    "user": { "client_user_id": user_id },
                    "products": self.cfg.products,
                    "country_codes": self.cfg.country_codes,
                    "language": "en",
                    "transactions": { "days_requested": 90 },
                    "webhook": format!("{app_url}/api/plaid/webhook"),
                }),
            )
            .await?;
        Ok(r.link_token)
    }

    pub async fn exchange(&self, public_token: &str) -> anyhow::Result<(String, String)> {
        #[derive(Deserialize)]
        struct R {
            access_token: String,
            item_id: String,
        }
        let r: R = self
            .post(
                "/item/public_token/exchange",
                json!({ "public_token": public_token }),
            )
            .await?;
        Ok((r.access_token, r.item_id))
    }

    /// `(institution_id, institution_name)` for an item, best effort.
    pub async fn institution(&self, access_token: &str) -> Option<(String, String)> {
        #[derive(Deserialize)]
        struct Item {
            institution_id: Option<String>,
        }
        #[derive(Deserialize)]
        struct ItemR {
            item: Item,
        }
        let item: ItemR = self
            .post("/item/get", json!({ "access_token": access_token }))
            .await
            .ok()?;
        let id = item.item.institution_id?;
        #[derive(Deserialize)]
        struct Inst {
            name: String,
        }
        #[derive(Deserialize)]
        struct InstR {
            institution: Inst,
        }
        let inst: InstR = self
            .post(
                "/institutions/get_by_id",
                json!({ "institution_id": id, "country_codes": self.cfg.country_codes }),
            )
            .await
            .ok()?;
        Some((id, inst.institution.name))
    }

    pub async fn accounts(&self, access_token: &str) -> anyhow::Result<Vec<PlaidAccount>> {
        #[derive(Deserialize)]
        struct R {
            accounts: Vec<PlaidAccount>,
        }
        let r: R = self
            .post("/accounts/get", json!({ "access_token": access_token }))
            .await?;
        Ok(r.accounts)
    }

    pub async fn sync(&self, access_token: &str, cursor: Option<&str>) -> anyhow::Result<SyncPage> {
        let mut body = json!({ "access_token": access_token, "count": 250, "options": { "include_original_description": true } });
        if let Some(c) = cursor {
            body["cursor"] = Value::String(c.to_string());
        }
        self.post("/transactions/sync", body).await
    }

    pub async fn remove(&self, access_token: &str) -> anyhow::Result<()> {
        let _: Value = self
            .post("/item/remove", json!({ "access_token": access_token }))
            .await?;
        Ok(())
    }
}

/// Plaid account type → Hearth account type.
pub fn account_type(kind: &str, subtype: Option<&str>) -> crate::db::models::AccountType {
    use crate::db::models::AccountType as T;
    match (kind, subtype.unwrap_or("")) {
        ("depository", "savings") | ("depository", "money market") | ("depository", "cd") => {
            T::Savings
        }
        ("depository", _) => T::Checking,
        ("credit", _) => T::CreditCard,
        ("loan", _) => T::Loan,
        ("investment", _) | ("brokerage", _) => T::Investment,
        _ => T::Other,
    }
}
