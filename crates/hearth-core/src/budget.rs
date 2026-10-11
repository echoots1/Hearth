//! Budget math: effective plan phases, rollups, and the month view.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::calendar::Month;
use crate::money::Cents;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sign {
    Income,
    Spend,
}

/// One plan phase: `amount` applies from `starts_month` until the next phase.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanPhase {
    pub starts_month: Month,
    pub amount_cents: Cents,
    pub label: Option<String>,
}

/// The phase in force for `month`: the one with the greatest `starts_month <= month`.
pub fn effective_phase(phases: &[PlanPhase], month: Month) -> Option<&PlanPhase> {
    phases
        .iter()
        .filter(|p| p.starts_month <= month)
        .max_by_key(|p| p.starts_month)
}

/// Plan amount for `month`; zero when no phase applies yet.
pub fn effective_plan(phases: &[PlanPhase], month: Month) -> Cents {
    effective_phase(phases, month)
        .map(|p| p.amount_cents)
        .unwrap_or(Cents::ZERO)
}

/// The first phase that starts *after* `month`, if any (the "after the change" column).
pub fn next_phase(phases: &[PlanPhase], month: Month) -> Option<&PlanPhase> {
    phases
        .iter()
        .filter(|p| p.starts_month > month)
        .min_by_key(|p| p.starts_month)
}

/// Minimal transaction shape for in-memory rollups.
#[derive(Clone, Debug)]
pub struct TxnForRollup {
    pub category_id: Option<Uuid>,
    pub amount_cents: Cents,
    pub deleted: bool,
    pub shadow: bool,
}

/// Signed sum per category (`None` = uncategorized). Deleted and shadow rows are excluded.
pub fn rollup_by_category<'a, I>(txns: I) -> HashMap<Option<Uuid>, Cents>
where
    I: IntoIterator<Item = &'a TxnForRollup>,
{
    let mut out: HashMap<Option<Uuid>, Cents> = HashMap::new();
    for t in txns {
        if t.deleted || t.shadow {
            continue;
        }
        *out.entry(t.category_id).or_insert(Cents::ZERO) += t.amount_cents;
    }
    out
}

/// Input per category for the month view.
#[derive(Clone, Debug)]
pub struct CategoryInput {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    pub sign: Sign,
    pub note: String,
    pub is_everyday: bool,
    pub sort_order: i32,
    pub phases: Vec<PlanPhase>,
    /// Monthly-equivalent total of the active bills in this category (derived by the caller).
    pub bills_total_cents: Cents,
    /// Signed sum of this month's transactions in the category.
    pub actual_cents: Cents,
}

#[derive(Clone, Debug)]
pub struct GroupInput {
    pub id: Uuid,
    pub name: String,
    pub sort_order: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextPhaseView {
    pub starts_month: Month,
    pub plan_cents: Cents,
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryView {
    pub id: Uuid,
    pub name: String,
    pub sign: Sign,
    pub note: String,
    pub is_everyday: bool,
    /// Monthly-equivalent total of the active bills in this category.
    pub bills_total_cents: Cents,
    /// `max(0, bills_total − plan)`: how far the plan falls short of the bills alone.
    pub bills_over_plan_cents: Cents,
    /// Plan for the month, unsigned.
    pub plan_cents: Cents,
    /// Signed sum of transactions.
    pub actual_cents: Cents,
    /// Unsigned activity: `-actual` for spend, `actual` for income.
    pub activity_cents: Cents,
    /// `plan − activity`. For spend: left to spend (negative = over).
    /// For income: still expected (negative = received more than planned).
    pub remaining_cents: Cents,
    /// `activity / plan` in basis points, capped at 100_00 when plan is zero and activity > 0.
    pub used_bps: i64,
    pub next_phase: Option<NextPhaseView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupView {
    pub id: Uuid,
    pub name: String,
    pub plan_cents: Cents,
    pub activity_cents: Cents,
    pub remaining_cents: Cents,
    pub categories: Vec<CategoryView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub planned_income_cents: Cents,
    pub planned_spend_cents: Cents,
    /// `planned_income − planned_spend` (zero-based target is 0).
    pub left_to_assign_cents: Cents,
    pub actual_income_cents: Cents,
    pub actual_spend_cents: Cents,
    /// `actual_income − actual_spend`.
    pub net_cents: Cents,
    /// Sum of spend-category remaining (how much room is left this month overall).
    pub spend_remaining_cents: Cents,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthView {
    pub month: Month,
    pub groups: Vec<GroupView>,
    pub totals: Totals,
    /// Signed sum of transactions with no category this month.
    pub uncategorized_cents: Cents,
}

pub fn category_view(c: &CategoryInput, month: Month) -> CategoryView {
    let plan = effective_plan(&c.phases, month);
    let activity = match c.sign {
        Sign::Spend => -c.actual_cents,
        Sign::Income => c.actual_cents,
    };
    let remaining = plan - activity;
    let used_bps = if plan.0 > 0 {
        activity.mul_div(10_000, plan.0).0
    } else if activity.0 > 0 {
        10_000
    } else {
        0
    };
    CategoryView {
        id: c.id,
        name: c.name.clone(),
        sign: c.sign,
        note: c.note.clone(),
        is_everyday: c.is_everyday,
        bills_total_cents: c.bills_total_cents,
        bills_over_plan_cents: (c.bills_total_cents - plan).floor_zero(),
        plan_cents: plan,
        actual_cents: c.actual_cents,
        activity_cents: activity,
        remaining_cents: remaining,
        used_bps,
        next_phase: next_phase(&c.phases, month).map(|p| NextPhaseView {
            starts_month: p.starts_month,
            plan_cents: p.amount_cents,
            label: p.label.clone(),
        }),
    }
}

/// Build the whole month view. Groups come out in `sort_order`, categories in
/// `sort_order` then name. Uncategorized activity is reported separately.
pub fn month_view(
    month: Month,
    groups: &[GroupInput],
    categories: &[CategoryInput],
    uncategorized_cents: Cents,
) -> MonthView {
    let mut groups_sorted: Vec<&GroupInput> = groups.iter().collect();
    groups_sorted.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut totals = Totals {
        planned_income_cents: Cents::ZERO,
        planned_spend_cents: Cents::ZERO,
        left_to_assign_cents: Cents::ZERO,
        actual_income_cents: Cents::ZERO,
        actual_spend_cents: Cents::ZERO,
        net_cents: Cents::ZERO,
        spend_remaining_cents: Cents::ZERO,
    };

    let mut out_groups = Vec::with_capacity(groups_sorted.len());
    for g in groups_sorted {
        let mut cats: Vec<&CategoryInput> =
            categories.iter().filter(|c| c.group_id == g.id).collect();
        cats.sort_by(|a, b| {
            a.sort_order
                .cmp(&b.sort_order)
                .then_with(|| a.name.cmp(&b.name))
        });
        let views: Vec<CategoryView> = cats.iter().map(|c| category_view(c, month)).collect();

        let mut plan = Cents::ZERO;
        let mut activity = Cents::ZERO;
        for v in &views {
            plan += v.plan_cents;
            activity += v.activity_cents;
            match v.sign {
                Sign::Income => {
                    totals.planned_income_cents += v.plan_cents;
                    totals.actual_income_cents += v.activity_cents;
                }
                Sign::Spend => {
                    totals.planned_spend_cents += v.plan_cents;
                    totals.actual_spend_cents += v.activity_cents;
                    totals.spend_remaining_cents += v.remaining_cents;
                }
            }
        }
        out_groups.push(GroupView {
            id: g.id,
            name: g.name.clone(),
            plan_cents: plan,
            activity_cents: activity,
            remaining_cents: plan - activity,
            categories: views,
        });
    }
    totals.left_to_assign_cents = totals.planned_income_cents - totals.planned_spend_cents;
    totals.net_cents = totals.actual_income_cents - totals.actual_spend_cents;

    MonthView {
        month,
        groups: out_groups,
        totals,
        uncategorized_cents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> Month {
        s.parse().unwrap()
    }

    fn phase(s: &str, dollars: i64, label: Option<&str>) -> PlanPhase {
        PlanPhase {
            starts_month: m(s),
            amount_cents: Cents::dollars(dollars),
            label: label.map(String::from),
        }
    }

    #[test]
    fn effective_and_next_phase() {
        let phases = vec![
            phase("2026-01", 1500, None),
            phase("2026-06", 2100, Some("new mortgage")),
        ];
        assert_eq!(effective_plan(&phases, m("2025-12")), Cents::ZERO);
        assert_eq!(effective_plan(&phases, m("2026-01")), Cents::dollars(1500));
        assert_eq!(effective_plan(&phases, m("2026-05")), Cents::dollars(1500));
        assert_eq!(effective_plan(&phases, m("2026-06")), Cents::dollars(2100));
        assert_eq!(effective_plan(&phases, m("2027-01")), Cents::dollars(2100));
        assert_eq!(
            next_phase(&phases, m("2026-03")).unwrap().starts_month,
            m("2026-06")
        );
        assert!(next_phase(&phases, m("2026-06")).is_none());
        assert_eq!(
            next_phase(&phases, m("2025-11")).unwrap().starts_month,
            m("2026-01")
        );
    }

    #[test]
    fn rollup_skips_deleted_and_shadow() {
        let c = Uuid::new_v4();
        let txns = vec![
            TxnForRollup {
                category_id: Some(c),
                amount_cents: Cents(-1000),
                deleted: false,
                shadow: false,
            },
            TxnForRollup {
                category_id: Some(c),
                amount_cents: Cents(-500),
                deleted: true,
                shadow: false,
            },
            TxnForRollup {
                category_id: Some(c),
                amount_cents: Cents(-250),
                deleted: false,
                shadow: true,
            },
            TxnForRollup {
                category_id: None,
                amount_cents: Cents(-99),
                deleted: false,
                shadow: false,
            },
        ];
        let r = rollup_by_category(&txns);
        assert_eq!(r[&Some(c)], Cents(-1000));
        assert_eq!(r[&None], Cents(-99));
    }

    #[test]
    fn month_view_totals() {
        let g_income = Uuid::new_v4();
        let g_spend = Uuid::new_v4();
        let groups = vec![
            GroupInput {
                id: g_spend,
                name: "Everyday".into(),
                sort_order: 2,
            },
            GroupInput {
                id: g_income,
                name: "Income".into(),
                sort_order: 1,
            },
        ];
        let cats = vec![
            CategoryInput {
                id: Uuid::new_v4(),
                group_id: g_income,
                name: "Paycheck".into(),
                sign: Sign::Income,
                note: String::new(),
                is_everyday: false,
                bills_total_cents: Cents::ZERO,
                sort_order: 0,
                phases: vec![phase("2026-01", 5000, None)],
                actual_cents: Cents::dollars(2500),
            },
            CategoryInput {
                id: Uuid::new_v4(),
                group_id: g_spend,
                name: "Groceries".into(),
                sign: Sign::Spend,
                note: String::new(),
                is_everyday: true,
                bills_total_cents: Cents::ZERO,
                sort_order: 1,
                phases: vec![phase("2026-01", 800, None)],
                actual_cents: Cents::dollars(-900),
            },
            CategoryInput {
                id: Uuid::new_v4(),
                group_id: g_spend,
                name: "Fuel".into(),
                sign: Sign::Spend,
                note: String::new(),
                is_everyday: true,
                bills_total_cents: Cents::dollars(250),
                sort_order: 0,
                phases: vec![phase("2026-01", 200, None)],
                actual_cents: Cents::dollars(-50),
            },
        ];
        let v = month_view(m("2026-03"), &groups, &cats, Cents(-1234));
        assert_eq!(v.groups[0].name, "Income");
        assert_eq!(v.groups[1].categories[0].name, "Fuel"); // sort_order 0 first
        assert_eq!(
            v.groups[1].categories[0].bills_over_plan_cents,
            Cents::dollars(50)
        );
        assert_eq!(v.groups[1].categories[1].bills_over_plan_cents, Cents::ZERO);
        let groceries = &v.groups[1].categories[1];
        assert_eq!(groceries.activity_cents, Cents::dollars(900));
        assert_eq!(groceries.remaining_cents, Cents::dollars(-100));
        assert_eq!(groceries.used_bps, 11_250);
        assert_eq!(v.totals.planned_income_cents, Cents::dollars(5000));
        assert_eq!(v.totals.planned_spend_cents, Cents::dollars(1000));
        assert_eq!(v.totals.left_to_assign_cents, Cents::dollars(4000));
        assert_eq!(v.totals.actual_income_cents, Cents::dollars(2500));
        assert_eq!(v.totals.actual_spend_cents, Cents::dollars(950));
        assert_eq!(v.totals.net_cents, Cents::dollars(1550));
        assert_eq!(v.totals.spend_remaining_cents, Cents::dollars(50));
        assert_eq!(v.uncategorized_cents, Cents(-1234));
        let paycheck = &v.groups[0].categories[0];
        assert_eq!(paycheck.remaining_cents, Cents::dollars(2500)); // still expected
    }

    #[test]
    fn used_bps_with_zero_plan() {
        let c = CategoryInput {
            id: Uuid::new_v4(),
            group_id: Uuid::new_v4(),
            name: "Misc".into(),
            sign: Sign::Spend,
            note: String::new(),
            is_everyday: false,
            bills_total_cents: Cents::ZERO,
            sort_order: 0,
            phases: vec![],
            actual_cents: Cents(-1),
        };
        assert_eq!(category_view(&c, m("2026-01")).used_bps, 10_000);
    }
}
