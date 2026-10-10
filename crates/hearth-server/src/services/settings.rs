//! Household settings: one JSON value per key, merged into `hearth_core::settings::Settings`.

use hearth_core::settings::Settings;
use serde_json::Value;

use super::{Actor, Ctx, Result, invalid};

/// Keys the UI/MCP may set directly. Anything else is rejected.
pub const KEYS: &[&str] = &[
    "pay_schedule",
    "cash_floor_cents",
    "large_charge_threshold_cents",
    "planned_match",
    "pace_alert",
    "stale_balance_days",
    "review_backlog",
    "sync_failure_threshold",
    "debt_strategy",
    "notifications",
    "nag",
    "csv_presets",
];

/// Load every stored key over the defaults.
pub async fn load(ctx: &Ctx) -> Result<Settings> {
    let rows: Vec<(String, Value)> =
        sqlx::query_as("select key, value from settings where household_id = $1")
            .bind(ctx.household_id)
            .fetch_all(&ctx.pool)
            .await?;
    let mut map = serde_json::Map::new();
    for (k, v) in rows {
        if k != "csv_presets" {
            map.insert(k, v);
        }
    }
    let s: Settings = serde_json::from_value(Value::Object(map))
        .map_err(|e| anyhow::anyhow!("stored settings are invalid: {e}"))?;
    Ok(s)
}

/// All stored settings as raw JSON (for the settings UI), with defaults filled in.
pub async fn load_raw(ctx: &Ctx) -> Result<Value> {
    let settings = load(ctx).await?;
    let mut v = serde_json::to_value(&settings).map_err(anyhow::Error::from)?;
    let presets: Option<(Value,)> = sqlx::query_as(
        "select value from settings where household_id = $1 and key = 'csv_presets'",
    )
    .bind(ctx.household_id)
    .fetch_optional(&ctx.pool)
    .await?;
    if let Value::Object(m) = &mut v {
        m.insert(
            "csv_presets".into(),
            presets.map(|p| p.0).unwrap_or(Value::Array(vec![])),
        );
    }
    Ok(v)
}

pub async fn get(ctx: &Ctx, key: &str) -> Result<Value> {
    let raw = load_raw(ctx).await?;
    raw.get(key)
        .cloned()
        .ok_or_else(|| invalid(format!("unknown setting {key:?}")))
}

/// Validate by merging into the typed struct, then store.
pub async fn set(ctx: &Ctx, actor: &Actor, key: &str, value: Value) -> Result<Value> {
    if !KEYS.contains(&key) {
        return Err(invalid(format!("unknown setting {key:?}")));
    }
    if key == "csv_presets" {
        if !value.is_array() {
            return Err(invalid("csv_presets must be an array"));
        }
    } else {
        let mut current = serde_json::to_value(load(ctx).await?).map_err(anyhow::Error::from)?;
        current[key] = value.clone();
        serde_json::from_value::<Settings>(current)
            .map_err(|e| invalid(format!("invalid value for {key}: {e}")))?;
    }
    let before: Option<(Value,)> =
        sqlx::query_as("select value from settings where household_id = $1 and key = $2")
            .bind(ctx.household_id)
            .bind(key)
            .fetch_optional(&ctx.pool)
            .await?;
    sqlx::query(
        "insert into settings (household_id, key, value) values ($1, $2, $3)
         on conflict (household_id, key) do update set value = excluded.value, updated_at = now()",
    )
    .bind(ctx.household_id)
    .bind(key)
    .bind(&value)
    .execute(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "settings.set",
        "settings",
        key,
        before.as_ref().map(|b| &b.0),
        Some(&value),
    )
    .await?;
    Ok(value)
}
