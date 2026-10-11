//! Read tools: open to every caller.

use chrono::NaiveDate;
use hearth_core::money::Cents;
use schemars::JsonSchema;
use serde::Deserialize;
use uuid::Uuid;

use super::{ToolDef, tool};
use crate::db::models::{
    AlertSeverity, AlertStatus, AlertType, BillStatus, PlannedStatus, QuestionKind, QuestionStatus,
};
use crate::services;

#[derive(Deserialize, JsonSchema, Default)]
pub struct Empty {}

#[derive(Deserialize, JsonSchema)]
pub struct MonthArgs {
    /// `YYYY-MM`; defaults to the current month.
    pub month: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct IdArgs {
    pub id: Uuid,
}

#[derive(Deserialize, JsonSchema)]
pub struct CategoryArgs {
    /// Category id, or omit and give `name`.
    pub id: Option<Uuid>,
    pub name: Option<String>,
    pub month: Option<String>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct TxnListArgs {
    pub month: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub account_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub uncategorized: Option<bool>,
    pub pending: Option<bool>,
    /// Substring search on description and note.
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct LimitArgs {
    pub limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct TimelineArgs {
    /// How far ahead, in days from today (default 60, at most 366).
    pub days: Option<i64>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct BillsListArgs {
    pub status: Option<BillStatus>,
    /// Only bills due before this date.
    pub due_before: Option<NaiveDate>,
    pub category_id: Option<Uuid>,
    pub include_canceled: Option<bool>,
    /// Shortcut: only bills due before the next payday.
    pub upcoming: Option<bool>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct PlannedListArgs {
    pub status: Option<PlannedStatus>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct WhatIfArgs {
    pub debt_id: Uuid,
    /// A one-time charge to put on this debt, in cents.
    pub charge_cents: Option<Cents>,
    /// An extra amount paid every month, in cents.
    pub extra_monthly_cents: Option<Cents>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct AlertsListArgs {
    pub status: Option<AlertStatus>,
    /// Everything not done.
    pub live: Option<bool>,
    pub severity: Option<AlertSeverity>,
    #[serde(rename = "type")]
    pub alert_type: Option<AlertType>,
    pub limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct QuestionsListArgs {
    pub status: Option<QuestionStatus>,
    pub kind: Option<QuestionKind>,
    pub limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct RulesListArgs {
    pub include_inactive: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RulePreviewArgs {
    pub pattern: String,
    pub regex: Option<bool>,
    pub account_id: Option<Uuid>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct ActionsArgs {
    pub limit: Option<i64>,
    pub undoable_only: Option<bool>,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct AuditArgs {
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub limit: Option<i64>,
}

fn month_or_now(
    ctx: &services::Ctx,
    m: &Option<String>,
) -> services::Result<hearth_core::calendar::Month> {
    match m {
        Some(s) => services::parse_month(s),
        None => Ok(ctx.month()),
    }
}

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "household.today",
            "Today's date and month in the household timezone, days left in the month, the next payday, this month's paydays (a biweekly schedule gives some months three), the assistant mode and autonomy settings, and the household name.",
            Empty,
            writes = false,
            area = None,
            |tc, _a| {
                let h = services::household::get(&tc.ctx).await?;
                let s = services::settings::load(&tc.ctx).await?;
                let today = tc.ctx.today();
                let month = tc.ctx.month();
                serde_json::json!({
                    "household": h.name, "timezone": h.timezone, "currency": h.currency,
                    "today": today, "month": month.to_string(),
                    "day_of_month": chrono::Datelike::day(&today), "days_in_month": month.days_in_month(),
                    "days_left": (month.last_day() - today).num_days(),
                    "next_payday": s.pay_schedule.next_payday(today),
                    "paydays_this_month": s.pay_schedule.paydays_in_month(month),
                    "assistant": s.assistant,
                    "cash_floor_cents": s.cash_floor_cents,
                })
            }
        ),
        tool!(
            "household.settings",
            "All household settings (pay schedule, cash floor, thresholds, quiet hours, nag, assistant mode and autonomy).",
            Empty,
            writes = false,
            area = None,
            |tc, _a| { services::settings::load_raw(&tc.ctx).await? }
        ),
        tool!(
            "budget.month",
            "The budget for a month: groups → categories with plan, spent (activity), left, pace, bill totals and the next plan phase; plus household totals and uncategorized activity. A category with `plan_per_paycheck` plans per paycheck: its `plan_cents` is `per_paycheck_cents × paychecks_in_month`, so a three-paycheck month plans more.",
            MonthArgs,
            writes = false,
            area = None,
            |tc, a| {
                let m = month_or_now(&tc.ctx, &a.month)?;
                services::budget::month_view(&tc.ctx, m).await?
            }
        ),
        tool!(
            "budget.category",
            "One category in detail: its view for the month, its plan phases, the bills in it, and this month's transactions.",
            CategoryArgs,
            writes = false,
            area = None,
            |tc, a| {
                let cat = match (a.id, &a.name) {
                    (Some(id), _) => services::budget::get_category(&tc.ctx, id).await?,
                    (None, Some(n)) => services::budget::find_category_by_name(&tc.ctx, n)
                        .await?
                        .ok_or(services::Error::NotFound)?,
                    _ => return Err(super::ToolError::Args("id or name is required".into())),
                };
                let m = month_or_now(&tc.ctx, &a.month)?;
                let view = services::budget::month_view(&tc.ctx, m).await?;
                let cv = view
                    .groups
                    .iter()
                    .flat_map(|g| g.categories.iter())
                    .find(|c| c.id == cat.id)
                    .cloned();
                let plans = services::budget::list_plans(&tc.ctx, Some(cat.id)).await?;
                let bills = services::bills::list(
                    &tc.ctx,
                    &services::bills::ListQuery {
                        category_id: Some(cat.id),
                        ..Default::default()
                    },
                )
                .await?;
                let txns = services::transactions::list(
                    &tc.ctx,
                    &services::transactions::ListQuery {
                        month: Some(m.to_string()),
                        category_id: Some(cat.id),
                        limit: Some(200),
                        ..Default::default()
                    },
                )
                .await?;
                serde_json::json!({ "category": cat, "view": cv, "plans": plans, "bills": bills, "transactions": txns.items })
            }
        ),
        tool!(
            "budget.groups",
            "Category groups and every category (including archived when asked).",
            RulesListArgs,
            writes = false,
            area = None,
            |tc, a| {
                let groups = services::budget::list_groups(&tc.ctx).await?;
                let cats =
                    services::budget::list_categories(&tc.ctx, a.include_inactive.unwrap_or(false))
                        .await?;
                serde_json::json!({ "groups": groups, "categories": cats })
            }
        ),
        tool!(
            "balance.overview",
            "Cash now: per-account running estimates with ages, the household estimate, and the projected low point before the next payday with its components (bills due, including any past due that have not posted; everyday pace; planned items) and headroom over the cash floor. Each account runs forward from its newest balance (`anchor`: the bank-synced one or a person's reported one), so synced accounts count without anyone typing a balance.",
            Empty,
            writes = false,
            area = None,
            |tc, _a| { services::balance::overview(&tc.ctx).await? }
        ),
        tool!(
            "balance.timeline",
            "Expected cash day by day from the current estimate: every coming paycheck (income categories planned per paycheck, on each payday), each bill occurrence, planned one-offs and the everyday drain, in date order with the running balance after each; plus the low point in the window and headroom over the cash floor. A bill or one-off past its date with no matching charge is still expected: it lands today, flagged `overdue`, and stays until the charge posts.",
            TimelineArgs,
            writes = false,
            area = None,
            |tc, a| { services::balance::timeline(&tc.ctx, a.days).await? }
        ),
        tool!(
            "accounts.list",
            "Accounts with reported and bank balances.",
            RulesListArgs,
            writes = false,
            area = None,
            |tc, a| {
                services::accounts::list(&tc.ctx, a.include_inactive.unwrap_or(false)).await?
            }
        ),
        tool!(
            "transactions.list",
            "Transactions, newest first, with filters and cursor paging.",
            TxnListArgs,
            writes = false,
            area = None,
            |tc, a| {
                services::transactions::list(
                    &tc.ctx,
                    &services::transactions::ListQuery {
                        month: a.month,
                        from: a.from,
                        to: a.to,
                        account_id: a.account_id,
                        category_id: a.category_id,
                        uncategorized: a.uncategorized,
                        pending: a.pending,
                        q: a.q,
                        limit: a.limit,
                        cursor: a.cursor,
                        ..Default::default()
                    },
                )
                .await?
            }
        ),
        tool!(
            "transactions.get",
            "One transaction.",
            IdArgs,
            writes = false,
            area = None,
            |tc, a| { services::transactions::get(&tc.ctx, a.id).await? }
        ),
        tool!(
            "transactions.review_queue",
            "Uncategorized transactions waiting for a decision.",
            LimitArgs,
            writes = false,
            area = None,
            |tc, a| { services::transactions::review_queue(&tc.ctx, a.limit).await? }
        ),
        tool!(
            "bills.list",
            "Recurring bills (active by default) with monthly equivalents and category names. `upcoming` limits to bills due before the next payday.",
            BillsListArgs,
            writes = false,
            area = None,
            |tc, a| {
                let mut q = services::bills::ListQuery {
                    status: a.status,
                    due_before: a.due_before,
                    category_id: a.category_id,
                    include_canceled: a.include_canceled,
                };
                if a.upcoming == Some(true) {
                    let s = services::settings::load(&tc.ctx).await?;
                    q.due_before = Some(s.pay_schedule.next_payday(tc.ctx.today()));
                }
                services::bills::views(&tc.ctx, &q).await?
            }
        ),
        tool!(
            "bills.get",
            "One recurring bill with its splits.",
            IdArgs,
            writes = false,
            area = None,
            |tc, a| {
                let b = services::bills::get(&tc.ctx, a.id).await?;
                let splits = services::bills::splits(&tc.ctx, a.id).await?;
                serde_json::json!({ "bill": b, "splits": splits, "monthly_equivalent_cents": services::bills::to_core(&b).monthly_equivalent() })
            }
        ),
        tool!(
            "planned.list",
            "Planned one-off items.",
            PlannedListArgs,
            writes = false,
            area = None,
            |tc, a| {
                services::planned::list(
                    &tc.ctx,
                    &services::planned::ListQuery {
                        status: a.status,
                        from: a.from,
                        to: a.to,
                        limit: a.limit,
                    },
                )
                .await?
            }
        ),
        tool!(
            "sinking.overview",
            "The sinking fund: funded balance, monthly set-aside, should-have-saved and shortfall across every non-monthly bill.",
            MonthArgs,
            writes = false,
            area = None,
            |tc, a| {
                let m = a.month.as_deref().map(services::parse_month).transpose()?;
                services::sinking::overview(&tc.ctx, m).await?
            }
        ),
        tool!(
            "debts.list",
            "Debts with interest per day and per month.",
            Empty,
            writes = false,
            area = None,
            |tc, _a| { services::debts::list_with_math(&tc.ctx).await? }
        ),
        tool!(
            "debts.projection",
            "Payoff projection under the current strategy: payoff month and total interest per debt, debt-free month.",
            Empty,
            writes = false,
            area = None,
            |tc, _a| { services::debts::projection(&tc.ctx).await? }
        ),
        tool!(
            "debts.compare",
            "Payoff month and total interest under avalanche, snowball and custom order, side by side.",
            Empty,
            writes = false,
            area = None,
            |tc, _a| { services::debts::compare(&tc.ctx).await? }
        ),
        tool!(
            "debts.what_if",
            "What a one-time charge on a debt, or an extra monthly payment, does to payoff dates and interest.",
            WhatIfArgs,
            writes = false,
            area = None,
            |tc, a| {
                services::debts::what_if(&tc.ctx, a.debt_id, a.charge_cents, a.extra_monthly_cents)
                    .await?
            }
        ),
        tool!(
            "goals.list",
            "Savings goals with projections (reach month at current pace, pace needed for the target date).",
            Empty,
            writes = false,
            area = None,
            |tc, _a| { services::goals::projections(&tc.ctx).await? }
        ),
        tool!(
            "alerts.list",
            "Alerts, most severe first.",
            AlertsListArgs,
            writes = false,
            area = None,
            |tc, a| {
                services::alerts::list(
                    &tc.ctx,
                    &services::alerts::ListQuery {
                        status: a.status,
                        live: a.live,
                        severity: a.severity,
                        alert_type: a.alert_type,
                        limit: a.limit,
                    },
                )
                .await?
            }
        ),
        tool!(
            "alerts.get",
            "One alert.",
            IdArgs,
            writes = false,
            area = None,
            |tc, a| { services::alerts::get(&tc.ctx, a.id).await? }
        ),
        tool!(
            "questions.list",
            "Questions and proposals, with answers.",
            QuestionsListArgs,
            writes = false,
            area = None,
            |tc, a| {
                services::questions::list(
                    &tc.ctx,
                    &services::questions::ListQuery {
                        status: a.status,
                        kind: a.kind,
                        limit: a.limit,
                    },
                )
                .await?
            }
        ),
        tool!(
            "rules.list",
            "Categorization rules in priority order.",
            RulesListArgs,
            writes = false,
            area = None,
            |tc, a| { services::rules::list(&tc.ctx, a.include_inactive.unwrap_or(false)).await? }
        ),
        tool!(
            "rules.preview",
            "Which recent transactions a pattern would match.",
            RulePreviewArgs,
            writes = false,
            area = None,
            |tc, a| {
                let mt = if a.regex.unwrap_or(false) {
                    crate::db::models::RuleMatchType::Regex
                } else {
                    crate::db::models::RuleMatchType::Contains
                };
                services::rules::preview(&tc.ctx, mt, &a.pattern, a.account_id).await?
            }
        ),
        tool!(
            "assistant.work",
            "Everything waiting for the assistant in one call: transactions to categorize (with suggestions), answered questions since the last pass, pending proposals, open alerts, overdue bills, categories whose bills exceed their plan, stale balances, and the autonomy settings.",
            Empty,
            writes = false,
            area = None,
            |tc, _a| { services::assistant::work(&tc.ctx).await? }
        ),
        tool!(
            "assistant.actions",
            "What the assistant changed in Driver mode, newest first, with undo availability.",
            ActionsArgs,
            writes = false,
            area = None,
            |tc, a| {
                services::assistant::actions(&tc.ctx, a.limit, a.undoable_only.unwrap_or(false))
                    .await?
            }
        ),
        tool!(
            "jobs.status",
            "Every job with its interval, last run (status, summary, error) and whether it is due.",
            Empty,
            writes = false,
            area = None,
            |tc, _a| {
                crate::jobs::status(&tc.state)
                    .await
                    .map_err(services::Error::Other)?
            }
        ),
        tool!(
            "audit.list",
            "The audit log (every write with actor, before and after).",
            AuditArgs,
            writes = false,
            area = None,
            |tc, a| {
                services::audit::list(
                    &tc.ctx,
                    &services::audit::AuditQuery {
                        entity: a.entity,
                        entity_id: a.entity_id,
                        limit: a.limit,
                    },
                )
                .await?
            }
        ),
    ]
}
