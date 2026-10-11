use chrono_tz::Tz;
use serde::Deserialize;
use sqlx::PgPool;

use super::{Actor, Ctx, Result, invalid};
use crate::db::models::Household;

/// The single household row, created on first start if missing.
pub async fn ensure(pool: &PgPool) -> Result<Household> {
    if let Some(h) =
        sqlx::query_as::<_, Household>("select * from households order by created_at limit 1")
            .fetch_optional(pool)
            .await?
    {
        return Ok(h);
    }
    let h = sqlx::query_as::<_, Household>(
        "insert into households (name, timezone, currency) values ($1, $2, $3) returning *",
    )
    .bind("My household")
    .bind("UTC")
    .bind("USD")
    .fetch_one(pool)
    .await?;
    Ok(h)
}

pub async fn get(ctx: &Ctx) -> Result<Household> {
    Ok(
        sqlx::query_as::<_, Household>("select * from households where id = $1")
            .bind(ctx.household_id)
            .fetch_one(&ctx.pool)
            .await?,
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateHousehold {
    pub name: Option<String>,
    pub timezone: Option<String>,
    pub currency: Option<String>,
}

pub async fn update(ctx: &Ctx, actor: &Actor, u: UpdateHousehold) -> Result<Household> {
    let before = get(ctx).await?;
    if let Some(tz) = &u.timezone {
        tz.parse::<Tz>()
            .map_err(|_| invalid(format!("unknown timezone {tz:?}")))?;
    }
    if let Some(c) = &u.currency
        && c.len() != 3
    {
        return Err(invalid("currency must be a 3-letter code"));
    }
    let after = sqlx::query_as::<_, Household>(
        "update households set name = coalesce($2, name), timezone = coalesce($3, timezone),
         currency = coalesce($4, currency) where id = $1 returning *",
    )
    .bind(ctx.household_id)
    .bind(&u.name)
    .bind(&u.timezone)
    .bind(u.currency.as_ref().map(|c| c.to_uppercase()))
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "household.update",
        "households",
        after.id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub fn parse_tz(s: &str) -> Tz {
    s.parse().unwrap_or(chrono_tz::UTC)
}
