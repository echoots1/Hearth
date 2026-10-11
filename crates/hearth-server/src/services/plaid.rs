//! Plaid items: link, sync, remove. Access tokens are encrypted at rest.

use chrono::NaiveDate;
use serde::Serialize;
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::{Account, PlaidItem, TransactionSource};
use crate::integrations::plaid::{self, Plaid};

pub async fn items(ctx: &Ctx) -> Result<Vec<PlaidItem>> {
    Ok(sqlx::query_as::<_, PlaidItem>(
        "select * from plaid_items where household_id = $1 order by created_at",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn get_item(ctx: &Ctx, id: Uuid) -> Result<PlaidItem> {
    sqlx::query_as::<_, PlaidItem>("select * from plaid_items where household_id = $1 and id = $2")
        .bind(ctx.household_id)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or(Error::NotFound)
}

fn crypto(ctx: &Ctx) -> Result<&super::secrets::Crypto> {
    ctx.crypto
        .as_ref()
        .ok_or_else(|| invalid("APP_ENCRYPTION_KEY is required for Plaid"))
}

async fn account_by_external(ctx: &Ctx, external_id: &str) -> Result<Option<Account>> {
    Ok(sqlx::query_as::<_, Account>(
        "select * from accounts where household_id = $1 and source = 'plaid' and external_id = $2",
    )
    .bind(ctx.household_id)
    .bind(external_id)
    .fetch_optional(&ctx.pool)
    .await?)
}

/// Create Hearth accounts for the Plaid accounts of an item that don't exist yet.
async fn ensure_accounts(
    ctx: &Ctx,
    actor: &Actor,
    item: &PlaidItem,
    accounts: &[plaid::PlaidAccount],
) -> Result<Vec<Account>> {
    let mut out = Vec::new();
    for a in accounts {
        let existing = account_by_external(ctx, &a.account_id).await?;
        let row = match existing {
            Some(r) => r,
            None => {
                // Plaid's type/subtype is a guess; people (or the assistant) fix it with accounts.update.
                let kind = plaid::account_type(&a.kind, a.subtype.as_deref());
                super::accounts::create(
                    ctx,
                    actor,
                    super::accounts::CreateAccount {
                        name: a
                            .official_name
                            .clone()
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| a.name.clone()),
                        account_type: kind,
                        source: Some(crate::db::models::AccountSource::Plaid),
                        external_id: Some(a.account_id.clone()),
                        plaid_item_id: Some(item.id),
                        institution_label: item.institution_name.clone(),
                        mask: a.mask.clone(),
                        include_in_cash: None,
                    },
                )
                .await?
            }
        };
        if let Some(c) = a.current_balance_cents() {
            super::accounts::set_bank_balance(ctx, row.id, c, ctx.now).await?;
        }
        out.push(row);
    }
    Ok(out)
}

/// Finish a Link flow: exchange the public token, store the item, create accounts.
pub async fn link(
    ctx: &Ctx,
    actor: &Actor,
    client: &Plaid,
    public_token: &str,
) -> Result<(PlaidItem, Vec<Account>)> {
    let crypto = crypto(ctx)?;
    let (access_token, item_id) = client
        .exchange(public_token)
        .await
        .map_err(|e| invalid(e.to_string()))?;
    let enc = crypto.encrypt(&access_token)?;
    let inst = client.institution(&access_token).await;
    let item = sqlx::query_as::<_, PlaidItem>(
        "insert into plaid_items (household_id, item_id, access_token_enc, institution_id, institution_name)
         values ($1, $2, $3, $4, $5)
         on conflict (item_id) do update set access_token_enc = excluded.access_token_enc, status = 'ok', last_error = null, consecutive_failures = 0, updated_at = now()
         returning *",
    )
    .bind(ctx.household_id)
    .bind(&item_id)
    .bind(&enc)
    .bind(inst.as_ref().map(|i| i.0.clone()))
    .bind(inst.as_ref().map(|i| i.1.clone()))
    .fetch_one(&ctx.pool)
    .await?;
    let accounts = client
        .accounts(&access_token)
        .await
        .map_err(|e| invalid(e.to_string()))?;
    let rows = ensure_accounts(ctx, actor, &item, &accounts).await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "plaid.link",
        "plaid_items",
        item.id,
        None::<&()>,
        Some(&item),
    )
    .await?;
    Ok((item, rows))
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncSummary {
    pub added: usize,
    pub modified: usize,
    pub removed: usize,
    /// $0 rows (verification holds, released authorizations), not stored.
    pub ignored: usize,
    pub accounts_updated: usize,
    pub pages: usize,
}

/// Pull everything since the item's cursor. Records success or failure on the item.
pub async fn sync_item(
    ctx: &Ctx,
    actor: &Actor,
    client: &Plaid,
    item: &PlaidItem,
) -> Result<SyncSummary> {
    match sync_inner(ctx, actor, client, item).await {
        Ok(s) => {
            sqlx::query("update plaid_items set status = 'ok', last_error = null, consecutive_failures = 0, last_synced_at = now(), updated_at = now() where id = $1")
                .bind(item.id)
                .execute(&ctx.pool)
                .await?;
            Ok(s)
        }
        Err(e) => {
            sqlx::query("update plaid_items set status = 'error', last_error = $2, consecutive_failures = consecutive_failures + 1, updated_at = now() where id = $1")
                .bind(item.id)
                .bind(e.to_string().chars().take(500).collect::<String>())
                .execute(&ctx.pool)
                .await?;
            Err(e)
        }
    }
}

async fn sync_inner(
    ctx: &Ctx,
    actor: &Actor,
    client: &Plaid,
    item: &PlaidItem,
) -> Result<SyncSummary> {
    let access_token = crypto(ctx)?.decrypt(&item.access_token_enc)?;
    let mut cursor = item.sync_cursor.clone();
    let mut summary = SyncSummary::default();
    let mut known: std::collections::HashMap<String, Uuid> = std::collections::HashMap::new();
    loop {
        let page = client
            .sync(&access_token, cursor.as_deref())
            .await
            .map_err(|e| Error::Other(anyhow::anyhow!(e)))?;
        summary.pages += 1;
        // Accounts first, so new accounts exist before their rows.
        if !page.accounts.is_empty() {
            let rows = ensure_accounts(ctx, actor, item, &page.accounts).await?;
            summary.accounts_updated += rows.len();
            for (a, r) in page.accounts.iter().zip(rows.iter()) {
                known.insert(a.account_id.clone(), r.id);
            }
        }
        for (t, modified) in page
            .added
            .iter()
            .map(|t| (t, false))
            .chain(page.modified.iter().map(|t| (t, true)))
        {
            let account_id = match known.get(&t.account_id) {
                Some(id) => *id,
                None => match account_by_external(ctx, &t.account_id).await? {
                    Some(a) => {
                        known.insert(t.account_id.clone(), a.id);
                        a.id
                    }
                    None => {
                        // An account Plaid didn't list: fetch the list once and create it.
                        let all = client
                            .accounts(&access_token)
                            .await
                            .map_err(|e| Error::Other(anyhow::anyhow!(e)))?;
                        let rows = ensure_accounts(ctx, actor, item, &all).await?;
                        for (a, r) in all.iter().zip(rows.iter()) {
                            known.insert(a.account_id.clone(), r.id);
                        }
                        match known.get(&t.account_id) {
                            Some(id) => *id,
                            None => continue,
                        }
                    }
                },
            };
            let Some(amount) = t.amount_cents() else {
                continue;
            };
            let Ok(date) = NaiveDate::parse_from_str(&t.date, "%Y-%m-%d") else {
                continue;
            };
            let name = t.name.clone().unwrap_or_default();
            let description = t
                .merchant_name
                .clone()
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| name.clone());
            let original = t
                .original_description
                .clone()
                .filter(|o| !o.is_empty())
                .unwrap_or(name);
            let inc = super::transactions::Incoming {
                account_id,
                source: TransactionSource::Plaid,
                external_id: t.transaction_id.clone(),
                replaces_external_id: t.pending_transaction_id.clone(),
                date,
                description,
                original_description: original,
                amount_cents: amount,
                pending: t.pending,
                raw: None,
            };
            let outcome = super::transactions::upsert_incoming(ctx, &inc).await?;
            if outcome == super::transactions::UpsertOutcome::Ignored {
                summary.ignored += 1;
            } else if modified {
                summary.modified += 1;
            } else {
                summary.added += 1;
            }
        }
        for r in &page.removed {
            if super::transactions::remove_external(
                ctx,
                TransactionSource::Plaid,
                &r.transaction_id,
            )
            .await?
            {
                summary.removed += 1;
            }
        }
        cursor = Some(page.next_cursor.clone());
        sqlx::query("update plaid_items set sync_cursor = $2, updated_at = now() where id = $1")
            .bind(item.id)
            .bind(&page.next_cursor)
            .execute(&ctx.pool)
            .await?;
        if !page.has_more {
            break;
        }
    }
    Ok(summary)
}

/// Unlink: tell Plaid, deactivate the accounts, drop the item. Transactions stay.
pub async fn remove_item(ctx: &Ctx, actor: &Actor, client: &Plaid, id: Uuid) -> Result<()> {
    let item = get_item(ctx, id).await?;
    if let Ok(token) = crypto(ctx)?.decrypt(&item.access_token_enc) {
        let _ = client.remove(&token).await;
    }
    sqlx::query("update accounts set is_active = false, plaid_item_id = null, updated_at = now() where plaid_item_id = $1")
        .bind(id)
        .execute(&ctx.pool)
        .await?;
    sqlx::query("delete from plaid_items where id = $1")
        .bind(id)
        .execute(&ctx.pool)
        .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "plaid.unlink",
        "plaid_items",
        id,
        Some(&item),
        None::<&()>,
    )
    .await?;
    Ok(())
}
