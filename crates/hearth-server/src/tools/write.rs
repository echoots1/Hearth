//! Write tools. Each runs through the autonomy gate: a person's call applies
//! directly; the assistant's call applies (Driver) or becomes a proposal
//! (Backseat), per area. Every applied change carries an undo spec where one
//! makes sense.

use chrono::{DateTime, NaiveDate, Utc};
use hearth_core::money::Cents;
use hearth_core::settings::AutonomyArea as Area;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ToolDef, gated, json as j, tool, undo};
use crate::db::models::{
    AlertStatus, BillStatus, DebtKind, GoalStatus, PlannedStatus, RuleMatchType,
};
use crate::services;

fn args_of<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

// ───────────────────────── accounts (never gated: bookkeeping, not money) ─────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct CreateAccountArgs {
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: crate::db::models::AccountType,
    pub mask: Option<String>,
    pub institution_label: Option<String>,
    /// Counts toward Cash now and the low point. Default: checking, savings and cash do.
    pub include_in_cash: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateAccountArgs {
    pub account_id: Uuid,
    pub name: Option<String>,
    pub include_in_cash: Option<bool>,
    pub is_active: Option<bool>,
    pub institution_label: Option<String>,
}

// ───────────────────────── categorizing & notes ─────────────────────────

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct CategorizeArgs {
    pub transaction_id: Uuid,
    /// Category id, or null to clear.
    pub category_id: Option<Uuid>,
    /// Or name the category instead of its id.
    pub category_name: Option<String>,
    pub note: Option<String>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct TxnNoteArgs {
    pub transaction_id: Uuid,
    pub note: String,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct LinkPlannedArgs {
    pub transaction_id: Uuid,
    /// Planned item id, or null to unlink.
    pub planned_item_id: Option<Uuid>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct CreateManualArgs {
    pub account_id: Uuid,
    pub date: NaiveDate,
    pub description: String,
    /// Signed cents: negative = money out.
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
    pub note: Option<String>,
    pub pending: Option<bool>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct DeleteTxnArgs {
    pub transaction_id: Uuid,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct ApplyRulesArgs {
    /// Re-run on every transaction, overriding earlier decisions. Default false.
    pub force: Option<bool>,
    pub transaction_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct RuleCreateArgs {
    pub pattern: String,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub name: Option<String>,
    pub regex: Option<bool>,
    pub priority: Option<i32>,
    pub account_id: Option<Uuid>,
    pub amount_min_cents: Option<Cents>,
    pub amount_max_cents: Option<Cents>,
    /// `true` marks matches as reviewed with no category (transfers, refunds).
    pub ignore: Option<bool>,
    pub set_note: Option<String>,
    pub apply_now: Option<bool>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct RuleUpdateArgs {
    pub rule_id: Uuid,
    pub name: Option<String>,
    pub pattern: Option<String>,
    pub regex: Option<bool>,
    pub priority: Option<i32>,
    pub category_id: Option<Uuid>,
    pub is_active: Option<bool>,
    pub set_note: Option<String>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct RuleDeleteArgs {
    pub rule_id: Uuid,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct CategoryNoteArgs {
    pub category_id: Uuid,
    pub note: String,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct CategoryCreateArgs {
    pub name: String,
    /// Group id, or a group name (created if missing).
    pub group_id: Option<Uuid>,
    pub group_name: Option<String>,
    /// `income` or `spend` (default).
    pub income: Option<bool>,
    pub is_everyday: Option<bool>,
    pub note: Option<String>,
    /// Plan for this month onward, in cents.
    pub plan_cents: Option<Cents>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct CategoryUpdateArgs {
    pub category_id: Uuid,
    pub name: Option<String>,
    pub group_id: Option<Uuid>,
    pub is_everyday: Option<bool>,
    pub is_archived: Option<bool>,
    pub sort_order: Option<i32>,
}

// ───────────────────────── plans & bills ─────────────────────────

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct SetPlanArgs {
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub amount_cents: Cents,
    /// `YYYY-MM` the new amount starts; defaults to the current month.
    pub starts_month: Option<String>,
    /// e.g. "after the move"
    pub label: Option<String>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct DeletePlanArgs {
    pub plan_id: Uuid,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct BillAddArgs {
    pub name: String,
    pub amount_cents: Cents,
    /// 1 monthly (default), 3 quarterly, 6, 12 yearly.
    pub cadence_months: Option<i16>,
    pub next_due: NaiveDate,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub account_id: Option<Uuid>,
    /// Substring of the bank description that identifies the charge; defaults to the name.
    pub match_pattern: Option<String>,
    pub note: Option<String>,
    /// Raise the category plan if its bills now exceed it (default true).
    pub raise_plan: Option<bool>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct BillUpdateArgs {
    pub bill_id: Uuid,
    pub name: Option<String>,
    pub amount_cents: Option<Cents>,
    pub cadence_months: Option<i16>,
    pub next_due: Option<NaiveDate>,
    pub category_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub match_pattern: Option<String>,
    pub note: Option<String>,
    pub status: Option<BillStatus>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct BillIdArgs {
    pub bill_id: Uuid,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct BillSplitArgs {
    pub bill_id: Uuid,
    /// Must total the bill amount; empty clears the split.
    pub splits: Vec<SplitPart>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct SplitPart {
    pub category_id: Uuid,
    pub amount_cents: Cents,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct PlannedCreateArgs {
    pub date: NaiveDate,
    pub item: String,
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub decided_by: Option<String>,
    pub note: Option<String>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct PlannedUpdateArgs {
    pub planned_item_id: Uuid,
    pub date: Option<NaiveDate>,
    pub item: Option<String>,
    pub amount_cents: Option<Cents>,
    pub category_id: Option<Uuid>,
    pub status: Option<PlannedStatus>,
    pub note: Option<String>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct SinkingBalanceArgs {
    pub balance_cents: Cents,
    pub as_of: Option<DateTime<Utc>>,
}

// ───────────────────────── debts & goals ─────────────────────────

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct DebtUpsertArgs {
    pub debt_id: Option<Uuid>,
    pub name: String,
    pub kind: Option<DebtKind>,
    pub balance_cents: Cents,
    /// APR in basis points: 24.99% = 2499.
    pub apr_bps: i32,
    pub min_payment_cents: Cents,
    pub planned_payment_cents: Option<Cents>,
    pub due_day: i16,
    pub account_id: Option<Uuid>,
    pub note: Option<String>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct DebtBalanceArgs {
    pub debt_id: Uuid,
    pub balance_cents: Cents,
    pub as_of: Option<DateTime<Utc>>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct StrategyArgs {
    /// avalanche | snowball | custom
    pub kind: Option<hearth_core::settings::DebtStrategyKind>,
    pub extra_payment_cents: Option<Cents>,
    pub custom_order: Option<Vec<Uuid>>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct GoalCreateArgs {
    pub name: String,
    pub target_cents: Cents,
    pub target_date: Option<NaiveDate>,
    pub account_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub saved_cents: Option<Cents>,
    pub note: Option<String>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct GoalUpdateArgs {
    pub goal_id: Uuid,
    pub name: Option<String>,
    pub target_cents: Option<Cents>,
    pub target_date: Option<NaiveDate>,
    pub category_id: Option<Uuid>,
    pub status: Option<GoalStatus>,
    pub note: Option<String>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct GoalSavedArgs {
    pub goal_id: Uuid,
    pub saved_cents: Cents,
    pub as_of: Option<DateTime<Utc>>,
}

// ───────────────────────── cash, alerts, questions, assistant ─────────────────────────

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct ReportedBalanceArgs {
    pub account_id: Uuid,
    pub balance_cents: Cents,
    /// Defaults to now.
    pub as_of: Option<DateTime<Utc>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AlertIdArgs {
    pub alert_id: Uuid,
}

#[derive(Deserialize, JsonSchema)]
pub struct ResolveArgs {
    pub alert_id: Uuid,
    pub resolution: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SnoozeArgs {
    pub alert_id: Uuid,
    pub until: DateTime<Utc>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AskArgs {
    pub question: String,
    /// `[{"key": "groceries", "label": "Groceries"}]`
    pub options: Option<Value>,
    pub transaction_id: Option<Uuid>,
    pub alert_id: Option<Uuid>,
    pub bill_id: Option<Uuid>,
    pub context: Option<Value>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AnswerArgs {
    pub question_id: Uuid,
    pub answer: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct QuestionIdArgs {
    pub question_id: Uuid,
}

#[derive(Deserialize, JsonSchema)]
pub struct SummaryArgs {
    /// A short report of what the pass did and found.
    pub text: String,
}

#[derive(Deserialize, JsonSchema, serde::Serialize, Clone)]
pub struct SettingsArgs {
    /// Setting key (anything except `assistant`, which is UI-only).
    pub key: String,
    pub value: Value,
}

async fn resolve_category(
    ctx: &services::Ctx,
    id: Option<Uuid>,
    name: &Option<String>,
) -> super::Result<Option<Uuid>> {
    match (id, name) {
        (Some(id), _) => Ok(Some(id)),
        (None, Some(n)) => Ok(Some(
            services::budget::find_category_by_name(ctx, n)
                .await?
                .ok_or_else(|| super::ToolError::Args(format!("no category named {n:?}")))?
                .id,
        )),
        _ => Ok(None),
    }
}

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "accounts.create",
            "Add a manual account (checking, savings, credit card, cash…). Synced accounts come from the integrations.",
            CreateAccountArgs,
            writes = true,
            area = None,
            |tc, a| {
                services::accounts::create(
                    &tc.ctx,
                    &tc.actor,
                    services::accounts::CreateAccount {
                        name: a.name,
                        account_type: a.account_type,
                        source: None,
                        external_id: None,
                        plaid_item_id: None,
                        institution_label: a.institution_label,
                        mask: a.mask,
                        include_in_cash: a.include_in_cash,
                    },
                )
                .await?
            }
        ),
        tool!(
            "accounts.update",
            "Rename an account, change whether it counts as cash, or deactivate it.",
            UpdateAccountArgs,
            writes = true,
            area = None,
            |tc, a| {
                services::accounts::update(
                    &tc.ctx,
                    &tc.actor,
                    a.account_id,
                    services::accounts::UpdateAccount {
                        name: a.name,
                        include_in_cash: a.include_in_cash,
                        is_active: a.is_active,
                        institution_label: a.institution_label,
                    },
                )
                .await?
            }
        ),
        // ── categorizing & notes ──
        tool!(
            "transactions.categorize",
            "Set (or clear) a transaction's category and optionally its note; resolves the review item.",
            CategorizeArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let cat = resolve_category(&tc.ctx, a.category_id, &a.category_name).await?;
                let before = services::transactions::get(&tc.ctx, a.transaction_id).await?;
                let name = match cat {
                    Some(c) => services::budget::get_category(&tc.ctx, c).await?.name,
                    None => "uncategorized".into(),
                };
                let summary = format!(
                    "Categorize “{}” ({}) as {}",
                    before.description, before.amount_cents, name
                );
                let args = args_of(&a);
                gated(&tc, Area::CategorizingNotes, "transactions.categorize", &args, summary, (Some(a.transaction_id), None), || async {
                let after = services::transactions::categorize(&tc.ctx, &tc.actor, a.transaction_id, cat, a.note.clone()).await?;
                let u = undo("transactions.categorize", json!({ "transaction_id": a.transaction_id, "category_id": before.category_id }), format!("Restore category on “{}”", before.description));
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "transactions.set_note",
            "Set a transaction's note.",
            TxnNoteArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let before = services::transactions::get(&tc.ctx, a.transaction_id).await?;
                let summary = format!("Note on “{}”: {}", before.description, a.note);
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::CategorizingNotes,
                    "transactions.set_note",
                    &args,
                    summary,
                    (Some(a.transaction_id), None),
                    || async {
                        let after = services::transactions::set_note(
                            &tc.ctx,
                            &tc.actor,
                            a.transaction_id,
                            a.note.clone(),
                        )
                        .await?;
                        let u = undo(
                            "transactions.set_note",
                            json!({ "transaction_id": a.transaction_id, "note": before.note }),
                            "Restore note",
                        );
                        Ok((after.clone(), j(&before), j(&after), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "transactions.link_planned",
            "Link a transaction to a planned item (marks it done) or unlink it.",
            LinkPlannedArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let before = services::transactions::get(&tc.ctx, a.transaction_id).await?;
                let summary = match a.planned_item_id {
                    Some(_) => format!("Link “{}” to a planned item", before.description),
                    None => format!("Unlink “{}” from its planned item", before.description),
                };
                let args = args_of(&a);
                gated(&tc, Area::CategorizingNotes, "transactions.link_planned", &args, summary, (Some(a.transaction_id), None), || async {
                let after = services::transactions::link_planned(&tc.ctx, &tc.actor, a.transaction_id, a.planned_item_id).await?;
                let u = undo("transactions.link_planned", json!({ "transaction_id": a.transaction_id, "planned_item_id": before.planned_item_id }), "Restore planned link");
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "transactions.create_manual",
            "Record a transaction by hand (cash, a check, something the feed missed).",
            CreateManualArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let summary = format!(
                    "Add transaction “{}” {} on {}",
                    a.description, a.amount_cents, a.date
                );
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::CategorizingNotes,
                    "transactions.create_manual",
                    &args,
                    summary,
                    (None, None),
                    || async {
                        let t = services::transactions::create_manual(
                            &tc.ctx,
                            &tc.actor,
                            services::transactions::CreateManual {
                                account_id: a.account_id,
                                date: a.date,
                                description: a.description.clone(),
                                amount_cents: a.amount_cents,
                                category_id: a.category_id,
                                note: a.note.clone(),
                                pending: a.pending,
                                planned_item_id: None,
                            },
                        )
                        .await?;
                        let u = undo(
                            "transactions.delete",
                            json!({ "transaction_id": t.id }),
                            format!("Remove “{}”", t.description),
                        );
                        Ok((t.clone(), None, j(&t), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "transactions.delete",
            "Soft-delete a transaction (manual or CSV rows; synced rows come back on the next sync).",
            DeleteTxnArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let before = services::transactions::get(&tc.ctx, a.transaction_id).await?;
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::CategorizingNotes,
                    "transactions.delete",
                    &args,
                    format!("Delete “{}”", before.description),
                    (Some(a.transaction_id), None),
                    || async {
                        let after = services::transactions::soft_delete(
                            &tc.ctx,
                            &tc.actor,
                            a.transaction_id,
                        )
                        .await?;
                        Ok((after.clone(), j(&before), j(&after), None))
                    },
                )
                .await?
            }
        ),
        tool!(
            "transactions.apply_rules",
            "Run the categorization rules over uncategorized transactions (or everything with force).",
            ApplyRulesArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                services::transactions::apply_rules(
                    &tc.ctx,
                    &tc.actor,
                    a.force.unwrap_or(false),
                    a.transaction_ids.as_deref(),
                )
                .await?
            }
        ),
        tool!(
            "rules.create",
            "Create a categorization rule (contains or regex) and apply it to existing uncategorized rows.",
            RuleCreateArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let cat = resolve_category(&tc.ctx, a.category_id, &a.category_name).await?;
                let summary = format!(
                    "Rule: “{}” → {}",
                    a.pattern,
                    if a.ignore == Some(true) {
                        "ignore".to_string()
                    } else {
                        a.category_name.clone().unwrap_or_else(|| "category".into())
                    }
                );
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::CategorizingNotes,
                    "rules.create",
                    &args,
                    summary,
                    (None, None),
                    || async {
                        let (r, applied) = services::rules::create(
                            &tc.ctx,
                            &tc.actor,
                            services::rules::CreateRule {
                                name: a.name.clone(),
                                pattern: a.pattern.clone(),
                                match_type: Some(if a.regex.unwrap_or(false) {
                                    RuleMatchType::Regex
                                } else {
                                    RuleMatchType::Contains
                                }),
                                priority: a.priority,
                                account_id: a.account_id,
                                amount_min_cents: a.amount_min_cents,
                                amount_max_cents: a.amount_max_cents,
                                action: Some(if a.ignore == Some(true) {
                                    crate::db::models::RuleAction::Ignore
                                } else {
                                    crate::db::models::RuleAction::Categorize
                                }),
                                category_id: cat,
                                set_note: a.set_note.clone(),
                                apply_now: a.apply_now,
                            },
                        )
                        .await?;
                        let u = undo(
                            "rules.delete",
                            json!({ "rule_id": r.id }),
                            format!("Delete rule “{}”", r.name),
                        );
                        Ok((json!({ "rule": r, "applied": applied }), None, j(&r), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "rules.update",
            "Change a rule.",
            RuleUpdateArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let before = services::rules::get(&tc.ctx, a.rule_id).await?;
                let args = args_of(&a);
                gated(&tc, Area::CategorizingNotes, "rules.update", &args, format!("Update rule “{}”", before.name), (None, None), || async {
                let after = services::rules::update(&tc.ctx, &tc.actor, a.rule_id, services::rules::UpdateRule {
                    name: a.name.clone(), pattern: a.pattern.clone(), match_type: a.regex.map(|r| if r { RuleMatchType::Regex } else { RuleMatchType::Contains }),
                    priority: a.priority, category_id: a.category_id, is_active: a.is_active, set_note: a.set_note.clone(), ..Default::default()
                }).await?;
                let u = undo("rules.update", json!({ "rule_id": before.id, "name": before.name, "pattern": before.pattern, "regex": before.match_type == RuleMatchType::Regex, "priority": before.priority, "category_id": before.category_id, "is_active": before.is_active, "set_note": before.set_note }), "Restore rule");
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "rules.delete",
            "Delete a rule.",
            RuleDeleteArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let before = services::rules::get(&tc.ctx, a.rule_id).await?;
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::CategorizingNotes,
                    "rules.delete",
                    &args,
                    format!("Delete rule “{}”", before.name),
                    (None, None),
                    || async {
                        services::rules::delete(&tc.ctx, &tc.actor, a.rule_id).await?;
                        Ok((json!({ "deleted": true }), j(&before), None, None))
                    },
                )
                .await?
            }
        ),
        tool!(
            "budget.set_category_note",
            "Set a category's standing note (read by the assistant; jobs honor [no-alerts], [no-large-charge], [no-pace]).",
            CategoryNoteArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let before = services::budget::get_category(&tc.ctx, a.category_id).await?;
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::CategorizingNotes,
                    "budget.set_category_note",
                    &args,
                    format!("Note on {}: {}", before.name, a.note),
                    (None, None),
                    || async {
                        let after = services::budget::set_note(
                            &tc.ctx,
                            &tc.actor,
                            a.category_id,
                            a.note.clone(),
                        )
                        .await?;
                        let u = undo(
                            "budget.set_category_note",
                            json!({ "category_id": a.category_id, "note": before.note }),
                            "Restore note",
                        );
                        Ok((after.clone(), j(&before), j(&after), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "budget.create_category",
            "Create a category (and its group if needed), optionally with a plan.",
            CategoryCreateArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let summary = format!(
                    "Create category “{}”{}",
                    a.name,
                    a.plan_cents
                        .map(|p| format!(" with plan {p}"))
                        .unwrap_or_default()
                );
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::PlansBills,
                    "budget.create_category",
                    &args,
                    summary,
                    (None, None),
                    || async {
                        let group_id = match (a.group_id, &a.group_name) {
                            (Some(g), _) => g,
                            (None, Some(n)) => {
                                let groups = services::budget::list_groups(&tc.ctx).await?;
                                match groups
                                    .iter()
                                    .find(|g| g.name.eq_ignore_ascii_case(n.trim()))
                                {
                                    Some(g) => g.id,
                                    None => {
                                        services::budget::create_group(
                                            &tc.ctx,
                                            &tc.actor,
                                            services::budget::CreateGroup {
                                                name: n.clone(),
                                                sort_order: None,
                                            },
                                        )
                                        .await?
                                        .id
                                    }
                                }
                            }
                            _ => {
                                return Err(services::invalid(
                                    "group_id or group_name is required",
                                ));
                            }
                        };
                        let c = services::budget::create_category(
                            &tc.ctx,
                            &tc.actor,
                            services::budget::CreateCategory {
                                group_id,
                                name: a.name.clone(),
                                sign: Some(if a.income == Some(true) {
                                    crate::db::models::CategorySign::Income
                                } else {
                                    crate::db::models::CategorySign::Spend
                                }),
                                note: a.note.clone(),
                                is_everyday: a.is_everyday,
                                sort_order: None,
                                plan_cents: a.plan_cents,
                            },
                        )
                        .await?;
                        let u = undo(
                            "budget.update_category",
                            json!({ "category_id": c.id, "is_archived": true }),
                            format!("Archive “{}”", c.name),
                        );
                        Ok((c.clone(), None, j(&c), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "budget.update_category",
            "Rename, move, flag or archive a category.",
            CategoryUpdateArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let before = services::budget::get_category(&tc.ctx, a.category_id).await?;
                let args = args_of(&a);
                gated(&tc, Area::PlansBills, "budget.update_category", &args, format!("Update category “{}”", before.name), (None, None), || async {
                let after = services::budget::update_category(&tc.ctx, &tc.actor, a.category_id, services::budget::UpdateCategory {
                    group_id: a.group_id, name: a.name.clone(), is_everyday: a.is_everyday, is_archived: a.is_archived, sort_order: a.sort_order, ..Default::default()
                }).await?;
                let u = undo("budget.update_category", json!({ "category_id": before.id, "name": before.name, "group_id": before.group_id, "is_everyday": before.is_everyday, "is_archived": before.is_archived, "sort_order": before.sort_order }), "Restore category");
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        // ── plans & bills ──
        tool!(
            "budget.set_plan",
            "Set a category's plan from a month onward (a plan phase). Omit starts_month for the current month.",
            SetPlanArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let cat_id = resolve_category(&tc.ctx, a.category_id, &a.category_name)
                    .await?
                    .ok_or_else(|| {
                        super::ToolError::Args("category_id or category_name is required".into())
                    })?;
                let cat = services::budget::get_category(&tc.ctx, cat_id).await?;
                let month = match &a.starts_month {
                    Some(m) => services::parse_month(m)?,
                    None => tc.ctx.month(),
                };
                let plans = services::budget::list_plans(&tc.ctx, Some(cat_id)).await?;
                let existing = plans
                    .iter()
                    .find(|p| p.starts_month == month.to_string())
                    .cloned();
                let current = {
                    let phases: Vec<hearth_core::budget::PlanPhase> = plans
                        .iter()
                        .filter_map(|p| {
                            Some(hearth_core::budget::PlanPhase {
                                starts_month: p.starts_month.parse().ok()?,
                                amount_cents: p.amount_cents,
                                label: p.label.clone(),
                            })
                        })
                        .collect();
                    hearth_core::budget::effective_plan(&phases, month)
                };
                let summary = format!(
                    "{} plan {} → {} from {}",
                    cat.name, current, a.amount_cents, month
                );
                let args = args_of(&a);
                gated(&tc, Area::PlansBills, "budget.set_plan", &args, summary, (None, None), || async {
                let p = services::budget::set_plan(&tc.ctx, &tc.actor, cat_id, month, a.amount_cents, a.label.clone(), None).await?;
                let u = match &existing {
                    Some(e) => undo("budget.set_plan", json!({ "category_id": cat_id, "amount_cents": e.amount_cents, "starts_month": e.starts_month, "label": e.label }), format!("Restore {} plan to {}", cat.name, e.amount_cents)),
                    None => undo("budget.delete_plan", json!({ "plan_id": p.id }), format!("Remove the {} plan phase", month)),
                };
                Ok((p.clone(), j(&existing), j(&p), u))
            }).await?
            }
        ),
        tool!(
            "budget.delete_plan",
            "Remove a plan phase.",
            DeletePlanArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::PlansBills,
                    "budget.delete_plan",
                    &args,
                    "Remove a plan phase".into(),
                    (None, None),
                    || async {
                        services::budget::delete_plan(&tc.ctx, &tc.actor, a.plan_id).await?;
                        Ok((json!({ "deleted": true }), None, None, None))
                    },
                )
                .await?
            }
        ),
        tool!(
            "bills.add",
            "Add a recurring bill from a sentence: name, amount, cadence, next due; optional category (by id or name), account, match pattern. Returns the bill, the category's bill total and plan, and whether the plan now falls short.",
            BillAddArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let cat = resolve_category(&tc.ctx, a.category_id, &a.category_name).await?;
                let cadence = a.cadence_months.unwrap_or(1);
                let summary = format!(
                    "Add bill “{}” {} every {} month{} from {}",
                    a.name,
                    a.amount_cents,
                    cadence,
                    if cadence == 1 { "" } else { "s" },
                    a.next_due
                );
                let args = args_of(&a);
                gated(&tc, Area::PlansBills, "bills.add", &args, summary, (None, None), || async {
                let b = services::bills::add(&tc.ctx, &tc.actor, services::bills::AddBill {
                    name: a.name.clone(), amount_cents: a.amount_cents, cadence_months: a.cadence_months, next_due: a.next_due,
                    category_id: cat, account_id: a.account_id, match_pattern: a.match_pattern.clone(), note: a.note.clone(),
                }).await?;
                // Plan check for the category.
                let mut plan_info = Value::Null;
                if let Some(c) = cat {
                    let view = services::budget::month_view(&tc.ctx, tc.ctx.month()).await?;
                    if let Some(cv) = view.groups.iter().flat_map(|g| g.categories.iter()).find(|x| x.id == c) {
                        let mut raised = None;
                        if cv.bills_over_plan_cents.0 > 0 && a.raise_plan.unwrap_or(true) && !tc.actor.is_assistant() {
                            raised = Some(services::budget::set_plan(&tc.ctx, &tc.actor, c, tc.ctx.month(), cv.bills_total_cents, None, Some("raised to cover bills".into())).await?);
                        }
                        plan_info = json!({ "category": cv.name, "plan_cents": cv.plan_cents, "bills_total_cents": cv.bills_total_cents, "short_cents": cv.bills_over_plan_cents, "plan_raised_to": raised.map(|p| p.amount_cents) });
                    }
                }
                let u = undo("bills.cancel", json!({ "bill_id": b.id }), format!("Cancel “{}”", b.name));
                Ok((json!({ "bill": b, "plan": plan_info }), None, j(&b), u))
            }).await?
            }
        ),
        tool!(
            "bills.update",
            "Change a bill's amount, due date, cadence, category, pattern, note or status.",
            BillUpdateArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let before = services::bills::get(&tc.ctx, a.bill_id).await?;
                let args = args_of(&a);
                gated(&tc, Area::PlansBills, "bills.update", &args, format!("Update bill “{}”", before.name), (None, Some(a.bill_id)), || async {
                let after = services::bills::update(&tc.ctx, &tc.actor, a.bill_id, services::bills::UpdateBill {
                    name: a.name.clone(), amount_cents: a.amount_cents, cadence_months: a.cadence_months, next_due: a.next_due, category_id: a.category_id,
                    account_id: a.account_id, match_pattern: a.match_pattern.clone(), note: a.note.clone(), status: a.status,
                }).await?;
                let u = undo("bills.update", json!({ "bill_id": before.id, "name": before.name, "amount_cents": before.amount_cents, "cadence_months": before.cadence_months, "next_due": before.next_due, "category_id": before.category_id, "account_id": before.account_id, "match_pattern": before.match_pattern, "note": before.note, "status": before.status }), format!("Restore “{}”", before.name));
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "bills.cancel",
            "Mark a bill canceled; any later charge matching it raises a critical alert.",
            BillIdArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let before = services::bills::get(&tc.ctx, a.bill_id).await?;
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::PlansBills,
                    "bills.cancel",
                    &args,
                    format!("Cancel “{}”", before.name),
                    (None, Some(a.bill_id)),
                    || async {
                        let after = services::bills::set_status(
                            &tc.ctx,
                            &tc.actor,
                            a.bill_id,
                            BillStatus::Canceled,
                        )
                        .await?;
                        let u = undo(
                            "bills.update",
                            json!({ "bill_id": before.id, "status": before.status }),
                            format!("Reactivate “{}”", before.name),
                        );
                        Ok((after.clone(), j(&before), j(&after), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "bills.pause",
            "Pause a bill (no due-date alerts, not counted in totals) without canceling it.",
            BillIdArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let before = services::bills::get(&tc.ctx, a.bill_id).await?;
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::PlansBills,
                    "bills.pause",
                    &args,
                    format!("Pause “{}”", before.name),
                    (None, Some(a.bill_id)),
                    || async {
                        let after = services::bills::set_status(
                            &tc.ctx,
                            &tc.actor,
                            a.bill_id,
                            BillStatus::Paused,
                        )
                        .await?;
                        let u = undo(
                            "bills.update",
                            json!({ "bill_id": before.id, "status": before.status }),
                            format!("Resume “{}”", before.name),
                        );
                        Ok((after.clone(), j(&before), j(&after), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "bills.resume",
            "Reactivate a paused or canceled bill.",
            BillIdArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let before = services::bills::get(&tc.ctx, a.bill_id).await?;
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::PlansBills,
                    "bills.resume",
                    &args,
                    format!("Resume “{}”", before.name),
                    (None, Some(a.bill_id)),
                    || async {
                        let after = services::bills::set_status(
                            &tc.ctx,
                            &tc.actor,
                            a.bill_id,
                            BillStatus::Active,
                        )
                        .await?;
                        let u = undo(
                            "bills.update",
                            json!({ "bill_id": before.id, "status": before.status }),
                            format!("Restore “{}” status", before.name),
                        );
                        Ok((after.clone(), j(&before), j(&after), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "bills.split",
            "Split a bill across categories. Only when a person asks for it; the assistant must not split on its own initiative.",
            BillSplitArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                if tc.actor.is_assistant() {
                    return Err(super::ToolError::Args(
                        "splits are set only at a person's request, in the UI".into(),
                    ));
                }
                let parts: Vec<(Uuid, Cents)> = a
                    .splits
                    .iter()
                    .map(|s| (s.category_id, s.amount_cents))
                    .collect();
                services::bills::set_splits(&tc.ctx, &tc.actor, a.bill_id, parts).await?
            }
        ),
        tool!(
            "bills.delete",
            "Delete a bill outright (prefer cancel, which keeps history).",
            BillIdArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let before = services::bills::get(&tc.ctx, a.bill_id).await?;
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::PlansBills,
                    "bills.delete",
                    &args,
                    format!("Delete “{}”", before.name),
                    (None, Some(a.bill_id)),
                    || async {
                        services::bills::delete(&tc.ctx, &tc.actor, a.bill_id).await?;
                        Ok((json!({ "deleted": true }), j(&before), None, None))
                    },
                )
                .await?
            }
        ),
        tool!(
            "planned.create",
            "Approve a one-off expense ahead of time so it isn't flagged when it posts.",
            PlannedCreateArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let cat = resolve_category(&tc.ctx, a.category_id, &a.category_name).await?;
                let summary = format!("Plan “{}” {} on {}", a.item, a.amount_cents, a.date);
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::PlansBills,
                    "planned.create",
                    &args,
                    summary,
                    (None, None),
                    || async {
                        let p = services::planned::create(
                            &tc.ctx,
                            &tc.actor,
                            services::planned::CreatePlanned {
                                date: a.date,
                                item: a.item.clone(),
                                amount_cents: a.amount_cents,
                                category_id: cat,
                                decided_by: a.decided_by.clone(),
                                note: a.note.clone(),
                            },
                        )
                        .await?;
                        let u = undo(
                            "planned.update",
                            json!({ "planned_item_id": p.id, "status": "dropped" }),
                            format!("Drop “{}”", p.item),
                        );
                        Ok((p.clone(), None, j(&p), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "planned.update",
            "Change a planned item or its status (planned, done, dropped).",
            PlannedUpdateArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let before = services::planned::get(&tc.ctx, a.planned_item_id).await?;
                let args = args_of(&a);
                gated(&tc, Area::PlansBills, "planned.update", &args, format!("Update planned “{}”", before.item), (None, None), || async {
                let after = services::planned::update(&tc.ctx, &tc.actor, a.planned_item_id, services::planned::UpdatePlanned {
                    date: a.date, item: a.item.clone(), amount_cents: a.amount_cents, category_id: a.category_id, status: a.status, decided_by: None, note: a.note.clone(),
                }).await?;
                let u = undo("planned.update", json!({ "planned_item_id": before.id, "date": before.date, "item": before.item, "amount_cents": before.amount_cents, "category_id": before.category_id, "status": before.status, "note": before.note }), format!("Restore “{}”", before.item));
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "sinking.set_balance",
            "Record what's actually saved in the sinking fund.",
            SinkingBalanceArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                let fund = services::sinking::ensure_fund(&tc.ctx).await?;
                let args = args_of(&a);
                gated(&tc, Area::PlansBills, "sinking.set_balance", &args, format!("Sinking fund balance {} → {}", fund.funded_balance_cents, a.balance_cents), (None, None), || async {
                let after = services::sinking::set_balance(&tc.ctx, &tc.actor, fund.id, a.balance_cents, a.as_of).await?;
                let u = undo("sinking.set_balance", json!({ "balance_cents": fund.funded_balance_cents, "as_of": fund.balance_as_of }), "Restore fund balance");
                Ok((after.clone(), j(&fund), j(&after), u))
            }).await?
            }
        ),
        // ── debts & goals ──
        tool!(
            "debts.upsert",
            "Add a debt or update one (name, balance, APR in bps, minimum, planned payment, due day).",
            DebtUpsertArgs,
            writes = true,
            area = Some(Area::DebtsGoals),
            |tc, a| {
                let before = match a.debt_id {
                    Some(id) => Some(services::debts::get(&tc.ctx, id).await?),
                    None => None,
                };
                let summary = match &before {
                    Some(b) => format!("Update debt “{}”", b.name),
                    None => format!(
                        "Add debt “{}” {} at {} bps",
                        a.name, a.balance_cents, a.apr_bps
                    ),
                };
                let args = args_of(&a);
                gated(&tc, Area::DebtsGoals, "debts.upsert", &args, summary, (None, None), || async {
                let after = services::debts::upsert(&tc.ctx, &tc.actor, services::debts::UpsertDebt {
                    id: a.debt_id, name: a.name.clone(), kind: a.kind, balance_cents: a.balance_cents, apr_bps: a.apr_bps, min_payment_cents: a.min_payment_cents,
                    planned_payment_cents: a.planned_payment_cents, due_day: a.due_day, account_id: a.account_id, custom_order: None, note: a.note.clone(),
                }).await?;
                let u = match &before {
                    Some(b) => undo("debts.upsert", json!({ "debt_id": b.id, "name": b.name, "kind": b.kind, "balance_cents": b.balance_cents, "apr_bps": b.apr_bps, "min_payment_cents": b.min_payment_cents, "planned_payment_cents": b.planned_payment_cents, "due_day": b.due_day, "account_id": b.account_id, "note": b.note }), format!("Restore “{}”", b.name)),
                    None => None,
                };
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "debts.set_balance",
            "Record a debt's current balance.",
            DebtBalanceArgs,
            writes = true,
            area = Some(Area::DebtsGoals),
            |tc, a| {
                let before = services::debts::get(&tc.ctx, a.debt_id).await?;
                let args = args_of(&a);
                gated(&tc, Area::DebtsGoals, "debts.set_balance", &args, format!("{} balance {} → {}", before.name, before.balance_cents, a.balance_cents), (None, None), || async {
                let after = services::debts::set_balance(&tc.ctx, &tc.actor, a.debt_id, a.balance_cents, a.as_of).await?;
                let u = undo("debts.set_balance", json!({ "debt_id": before.id, "balance_cents": before.balance_cents, "as_of": before.balance_as_of }), "Restore balance");
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "debts.set_strategy",
            "Set the payoff strategy (avalanche, snowball, custom order) and the extra monthly payment.",
            StrategyArgs,
            writes = true,
            area = Some(Area::DebtsGoals),
            |tc, a| {
                let settings = services::settings::load(&tc.ctx).await?;
                let before = settings.debt_strategy.clone();
                let mut after = before.clone();
                if let Some(k) = a.kind {
                    after.kind = k;
                }
                if let Some(e) = a.extra_payment_cents {
                    after.extra_payment_cents = e;
                }
                if let Some(o) = &a.custom_order {
                    after.custom_order = o.clone();
                }
                let summary = format!(
                    "Debt strategy {:?} extra {} → {:?} extra {}",
                    before.kind, before.extra_payment_cents, after.kind, after.extra_payment_cents
                );
                let args = args_of(&a);
                gated(&tc, Area::DebtsGoals, "debts.set_strategy", &args, summary, (None, None), || async {
                let v = services::debts::set_strategy(&tc.ctx, &tc.actor, after.clone()).await?;
                let u = undo("debts.set_strategy", json!({ "kind": before.kind, "extra_payment_cents": before.extra_payment_cents, "custom_order": before.custom_order }), "Restore strategy");
                Ok((v, j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "goals.create",
            "Create a savings goal.",
            GoalCreateArgs,
            writes = true,
            area = Some(Area::DebtsGoals),
            |tc, a| {
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::DebtsGoals,
                    "goals.create",
                    &args,
                    format!("Goal “{}” {}", a.name, a.target_cents),
                    (None, None),
                    || async {
                        let g = services::goals::create(
                            &tc.ctx,
                            &tc.actor,
                            services::goals::CreateGoal {
                                name: a.name.clone(),
                                target_cents: a.target_cents,
                                target_date: a.target_date,
                                account_id: a.account_id,
                                category_id: a.category_id,
                                saved_cents: a.saved_cents,
                                note: a.note.clone(),
                            },
                        )
                        .await?;
                        let u = undo(
                            "goals.update",
                            json!({ "goal_id": g.id, "status": "dropped" }),
                            format!("Drop goal “{}”", g.name),
                        );
                        Ok((g.clone(), None, j(&g), u))
                    },
                )
                .await?
            }
        ),
        tool!(
            "goals.update",
            "Change a goal or its status.",
            GoalUpdateArgs,
            writes = true,
            area = Some(Area::DebtsGoals),
            |tc, a| {
                let before = services::goals::get(&tc.ctx, a.goal_id).await?;
                let args = args_of(&a);
                gated(&tc, Area::DebtsGoals, "goals.update", &args, format!("Update goal “{}”", before.name), (None, None), || async {
                let after = services::goals::update(&tc.ctx, &tc.actor, a.goal_id, services::goals::UpdateGoal {
                    name: a.name.clone(), target_cents: a.target_cents, target_date: a.target_date, account_id: None, category_id: a.category_id, status: a.status, note: a.note.clone(),
                }).await?;
                let u = undo("goals.update", json!({ "goal_id": before.id, "name": before.name, "target_cents": before.target_cents, "target_date": before.target_date, "category_id": before.category_id, "status": before.status, "note": before.note }), format!("Restore “{}”", before.name));
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        tool!(
            "goals.set_saved",
            "Record how much is saved toward a goal.",
            GoalSavedArgs,
            writes = true,
            area = Some(Area::DebtsGoals),
            |tc, a| {
                let before = services::goals::get(&tc.ctx, a.goal_id).await?;
                let args = args_of(&a);
                gated(&tc, Area::DebtsGoals, "goals.set_saved", &args, format!("{} saved {} → {}", before.name, before.saved_cents, a.saved_cents), (None, None), || async {
                let after = services::goals::set_saved(&tc.ctx, &tc.actor, a.goal_id, a.saved_cents, a.as_of).await?;
                let u = undo("goals.set_saved", json!({ "goal_id": before.id, "saved_cents": before.saved_cents, "as_of": before.saved_as_of }), "Restore saved amount");
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        // ── cash ──
        tool!(
            "balance.set_reported",
            "Record the balance a person just read off their bank, with when they read it.",
            ReportedBalanceArgs,
            writes = true,
            area = Some(Area::CategorizingNotes),
            |tc, a| {
                let before = services::accounts::get(&tc.ctx, a.account_id).await?;
                let args = args_of(&a);
                gated(&tc, Area::CategorizingNotes, "balance.set_reported", &args, format!("{} balance {} → {}", before.name, before.reported_balance_cents.map(|c| c.to_string()).unwrap_or_else(|| "—".into()), a.balance_cents), (None, None), || async {
                let after = services::accounts::set_reported_balance(&tc.ctx, &tc.actor, a.account_id, a.balance_cents, a.as_of).await?;
                let u = before.reported_balance_cents.map(|c| services::questions::Proposal { tool: "balance.set_reported".into(), args: json!({ "account_id": before.id, "balance_cents": c, "as_of": before.reported_balance_as_of }), summary: "Restore reported balance".into() });
                Ok((after.clone(), j(&before), j(&after), u))
            }).await?
            }
        ),
        // ── alerts & questions (never gated: the assistant acting as hands on the inbox) ──
        tool!(
            "alerts.ack",
            "Acknowledge an alert (a human has seen it; nagging stops).",
            AlertIdArgs,
            writes = true,
            area = None,
            |tc, a| { services::alerts::ack(&tc.ctx, &tc.actor, a.alert_id).await? }
        ),
        tool!(
            "alerts.resolve",
            "Mark an alert done with a short resolution.",
            ResolveArgs,
            writes = true,
            area = None,
            |tc, a| {
                services::alerts::resolve(&tc.ctx, &tc.actor, a.alert_id, a.resolution).await?
            }
        ),
        tool!(
            "alerts.snooze",
            "Pause nagging on an alert until a time.",
            SnoozeArgs,
            writes = true,
            area = None,
            |tc, a| { services::alerts::snooze(&tc.ctx, &tc.actor, a.alert_id, a.until).await? }
        ),
        tool!(
            "alerts.reopen",
            "Reopen a done alert.",
            AlertIdArgs,
            writes = true,
            area = None,
            |tc, a| {
                let _ = AlertStatus::Open;
                services::alerts::reopen(&tc.ctx, &tc.actor, a.alert_id).await?
            }
        ),
        tool!(
            "questions.ask",
            "Ask the humans something (shows in the inbox and is pushed); optional one-tap options.",
            AskArgs,
            writes = true,
            area = None,
            |tc, a| {
                services::questions::ask(
                    &tc.ctx,
                    &tc.actor,
                    services::questions::AskQuestion {
                        question: a.question,
                        options: a.options,
                        transaction_id: a.transaction_id,
                        alert_id: a.alert_id,
                        planned_item_id: None,
                        bill_id: a.bill_id,
                        context: a.context,
                        notify: None,
                    },
                )
                .await?
            }
        ),
        tool!(
            "questions.answer",
            "Answer an open question (people, or the assistant relaying an answer given in chat).",
            AnswerArgs,
            writes = true,
            area = None,
            |tc, a| {
                services::questions::answer(&tc.ctx, &tc.actor, a.question_id, a.answer).await?
            }
        ),
        tool!(
            "questions.dismiss",
            "Dismiss a question without answering.",
            QuestionIdArgs,
            writes = true,
            area = None,
            |tc, a| { services::questions::dismiss(&tc.ctx, &tc.actor, a.question_id).await? }
        ),
        tool!(
            "assistant.summary_write",
            "Leave a short report of a pass; shows in Activity and marks the pass time.",
            SummaryArgs,
            writes = true,
            area = None,
            |tc, a| { services::assistant::write_summary(&tc.ctx, &tc.actor, a.text).await? }
        ),
        tool!(
            "household.update_settings",
            "Change a household setting (pay schedule, cash floor, thresholds, quiet hours, nag). The `assistant` setting is UI-only.",
            SettingsArgs,
            writes = true,
            area = Some(Area::PlansBills),
            |tc, a| {
                if a.key == "assistant" {
                    return Err(super::ToolError::Args(
                        "the assistant setting can only be changed in the app".into(),
                    ));
                }
                let before = services::settings::get(&tc.ctx, &a.key).await.ok();
                let args = args_of(&a);
                gated(
                    &tc,
                    Area::PlansBills,
                    "household.update_settings",
                    &args,
                    format!("Set {} to {}", a.key, a.value),
                    (None, None),
                    || async {
                        let v =
                            services::settings::set(&tc.ctx, &tc.actor, &a.key, a.value.clone())
                                .await?;
                        let u = before.clone().map(|b| services::questions::Proposal {
                            tool: "household.update_settings".into(),
                            args: json!({ "key": a.key, "value": b }),
                            summary: format!("Restore {}", a.key),
                        });
                        Ok((v, before.clone(), Some(a.value.clone()), u))
                    },
                )
                .await?
            }
        ),
    ]
}
