//! Sinking fund: monthly set-aside and "should have saved by now".

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::calendar::Month;
use crate::money::Cents;

#[derive(Clone, Debug)]
pub struct SinkingItemInput {
    pub id: Uuid,
    pub name: String,
    pub amount_cents: Cents,
    /// Next due month.
    pub due_month: Month,
    /// 12 = annual, 6, 3, 1; `None` = one-off.
    pub cycle_months: Option<u32>,
    /// Month the item was created (start of saving for one-offs).
    pub created_month: Month,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SinkingItemView {
    pub id: Uuid,
    pub name: String,
    pub amount_cents: Cents,
    pub due_month: Month,
    pub cycle_months: Option<u32>,
    /// Months in the current saving cycle.
    pub cycle_len: u32,
    /// Months of the cycle elapsed as of the evaluated month (0..=cycle_len).
    pub months_elapsed: u32,
    pub monthly_set_aside_cents: Cents,
    pub should_have_saved_cents: Cents,
    /// Due month is before the evaluated month and hasn't been advanced.
    pub overdue: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SinkingFundView {
    pub month: Month,
    pub funded_balance_cents: Cents,
    pub monthly_set_aside_cents: Cents,
    pub should_have_saved_cents: Cents,
    /// `max(0, should_have − funded)`.
    pub shortfall_cents: Cents,
    /// `funded − should_have` (positive = ahead).
    pub surplus_cents: Cents,
    /// Total of all items' amounts (what the fund covers over a full cycle).
    pub annualized_cents: Cents,
    pub items: Vec<SinkingItemView>,
}

/// `(cycle_len, cycle_start)` for an item: for recurring items the cycle starts
/// the month after the previous due month; for one-offs it starts at creation.
fn cycle(item: &SinkingItemInput) -> (u32, Month) {
    match item.cycle_months {
        Some(c) if c > 0 => (c, item.due_month.add_months(-(c as i32))),
        _ => {
            let len = item.created_month.months_until(item.due_month).max(1) as u32;
            (len, item.due_month.add_months(-(len as i32)))
        }
    }
}

pub fn item_view(item: &SinkingItemInput, month: Month) -> SinkingItemView {
    let (cycle_len, cycle_start) = cycle(item);
    // Months elapsed: cycle_start is the "previous due"; the month after it is
    // elapsed-month 1; the due month itself is elapsed-month cycle_len.
    let elapsed = cycle_start.months_until(month).clamp(0, cycle_len as i32) as u32;
    let set_aside = item.amount_cents.div_ceil(cycle_len as i64);
    let should_have = item.amount_cents.mul_div(elapsed as i64, cycle_len as i64);
    SinkingItemView {
        id: item.id,
        name: item.name.clone(),
        amount_cents: item.amount_cents,
        due_month: item.due_month,
        cycle_months: item.cycle_months,
        cycle_len,
        months_elapsed: elapsed,
        monthly_set_aside_cents: set_aside,
        should_have_saved_cents: should_have,
        overdue: item.due_month < month,
    }
}

pub fn fund_view(
    items: &[SinkingItemInput],
    funded_balance: Cents,
    month: Month,
) -> SinkingFundView {
    let views: Vec<SinkingItemView> = items.iter().map(|i| item_view(i, month)).collect();
    let set_aside: Cents = views.iter().map(|v| v.monthly_set_aside_cents).sum();
    let should_have: Cents = views.iter().map(|v| v.should_have_saved_cents).sum();
    let annualized: Cents = views.iter().map(|v| v.amount_cents).sum();
    SinkingFundView {
        month,
        funded_balance_cents: funded_balance,
        monthly_set_aside_cents: set_aside,
        should_have_saved_cents: should_have,
        shortfall_cents: (should_have - funded_balance).floor_zero(),
        surplus_cents: funded_balance - should_have,
        annualized_cents: annualized,
        items: views,
    }
}

/// After the bill posts, the next due month for a recurring item.
pub fn advance_due_month(item: &SinkingItemInput) -> Option<Month> {
    item.cycle_months
        .filter(|c| *c > 0)
        .map(|c| item.due_month.add_months(c as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> Month {
        s.parse().unwrap()
    }

    fn annual(name: &str, dollars: i64, due: &str) -> SinkingItemInput {
        SinkingItemInput {
            id: Uuid::new_v4(),
            name: name.into(),
            amount_cents: Cents::dollars(dollars),
            due_month: m(due),
            cycle_months: Some(12),
            created_month: m("2025-01"),
        }
    }

    #[test]
    fn annual_item_progression() {
        let item = annual("Car registration", 120, "2026-12");
        let jan = item_view(&item, m("2026-01"));
        assert_eq!(jan.monthly_set_aside_cents, Cents::dollars(10));
        assert_eq!(jan.months_elapsed, 1);
        assert_eq!(jan.should_have_saved_cents, Cents::dollars(10));
        let jun = item_view(&item, m("2026-06"));
        assert_eq!(jun.should_have_saved_cents, Cents::dollars(60));
        let dec = item_view(&item, m("2026-12"));
        assert_eq!(dec.months_elapsed, 12);
        assert_eq!(dec.should_have_saved_cents, Cents::dollars(120));
        assert!(!dec.overdue);
        // Not advanced after due: still fully owed and flagged.
        let next_jan = item_view(&item, m("2027-01"));
        assert_eq!(next_jan.should_have_saved_cents, Cents::dollars(120));
        assert!(next_jan.overdue);
        // Before the cycle started (shouldn't happen, but clamps to 0).
        let early = item_view(&item, m("2025-06"));
        assert_eq!(early.should_have_saved_cents, Cents::ZERO);
    }

    #[test]
    fn set_aside_rounds_up() {
        let item = SinkingItemInput {
            id: Uuid::new_v4(),
            name: "Odd".into(),
            amount_cents: Cents(10_000),
            due_month: m("2026-12"),
            cycle_months: Some(12),
            created_month: m("2026-01"),
        };
        assert_eq!(
            item_view(&item, m("2026-01")).monthly_set_aside_cents,
            Cents(834)
        );
    }

    #[test]
    fn one_off_spreads_from_creation() {
        let item = SinkingItemInput {
            id: Uuid::new_v4(),
            name: "Passport".into(),
            amount_cents: Cents::dollars(300),
            due_month: m("2026-06"),
            cycle_months: None,
            created_month: m("2026-03"),
        };
        let v = item_view(&item, m("2026-04"));
        assert_eq!(v.cycle_len, 3);
        assert_eq!(v.monthly_set_aside_cents, Cents::dollars(100));
        assert_eq!(v.should_have_saved_cents, Cents::dollars(100));
        assert_eq!(advance_due_month(&item), None);
        // created == due → cycle of 1
        let now = SinkingItemInput {
            created_month: m("2026-06"),
            ..item.clone()
        };
        assert_eq!(item_view(&now, m("2026-06")).cycle_len, 1);
    }

    #[test]
    fn fund_totals_and_shortfall() {
        let items = vec![annual("A", 120, "2026-12"), annual("B", 600, "2026-06")];
        let v = fund_view(&items, Cents::dollars(200), m("2026-03"));
        // A: 3/12 of 120 = 30; B: due Jun, cycle start Jun 2025, elapsed 9 → 450
        assert_eq!(v.should_have_saved_cents, Cents::dollars(480));
        assert_eq!(v.monthly_set_aside_cents, Cents::dollars(60));
        assert_eq!(v.shortfall_cents, Cents::dollars(280));
        assert_eq!(v.surplus_cents, Cents::dollars(-280));
        assert_eq!(v.annualized_cents, Cents::dollars(720));
        assert_eq!(advance_due_month(&items[1]), Some(m("2027-06")));
    }
}
