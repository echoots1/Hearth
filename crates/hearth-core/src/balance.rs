//! Running balance estimate and projected low point to the next paycheck.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::budget::Sign;
use crate::calendar::{Month, days_between, local_date};
use crate::money::Cents;

/// Account fields the estimate needs.
#[derive(Clone, Debug)]
pub struct AccountForEstimate {
    pub id: Uuid,
    pub name: String,
    pub include_in_cash: bool,
    pub reported_balance_cents: Option<Cents>,
    pub reported_balance_as_of: Option<DateTime<Utc>>,
    pub bank_balance_cents: Option<Cents>,
    pub bank_balance_as_of: Option<DateTime<Utc>>,
}

/// Transaction fields the estimate needs.
#[derive(Clone, Debug)]
pub struct TxnForEstimate {
    pub account_id: Uuid,
    pub date: NaiveDate,
    pub amount_cents: Cents,
    pub pending: bool,
    pub deleted: bool,
    /// The bank's line for a charge another row on the **same** account already
    /// carries (a Privacy.com charge and its `PwP` bank line). Never counted.
    pub duplicate: bool,
}

/// Which balance an account's estimate runs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BalanceAnchor {
    /// A person read it off the bank and typed it in.
    Reported,
    /// A sync (Plaid) fetched it.
    Bank,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountEstimate {
    pub account_id: Uuid,
    pub name: String,
    pub include_in_cash: bool,
    pub reported_balance_cents: Option<Cents>,
    pub reported_balance_as_of: Option<DateTime<Utc>>,
    pub bank_balance_cents: Option<Cents>,
    pub bank_balance_as_of: Option<DateTime<Utc>>,
    /// The balance the estimate runs from: the newer of the reported and the
    /// bank balance. `None` when the account has neither.
    pub anchor: Option<BalanceAnchor>,
    pub anchor_balance_cents: Option<Cents>,
    pub anchor_as_of: Option<DateTime<Utc>>,
    /// Household-local date of the anchor's as-of instant.
    pub as_of_date: Option<NaiveDate>,
    /// Signed sum of transactions counted since the anchor (posted after `as_of_date`, plus pending).
    pub since_cents: Cents,
    pub since_count: usize,
    /// `anchor + since`; `None` when the account has no dated balance at all.
    pub estimate_cents: Option<Cents>,
    /// Age of the anchor balance in whole days as of `today`.
    pub anchor_age_days: Option<i64>,
    /// Age of the reported balance in whole days as of `today`.
    pub reported_age_days: Option<i64>,
}

/// The balance an estimate runs from: the newer of the person-reported and the
/// bank-synced balance. A balance without an as-of instant cannot anchor. On a
/// tie the reported one wins: a person typed it on purpose.
pub fn anchor(acct: &AccountForEstimate) -> Option<(BalanceAnchor, Cents, DateTime<Utc>)> {
    let reported = acct
        .reported_balance_cents
        .zip(acct.reported_balance_as_of)
        .map(|(c, t)| (BalanceAnchor::Reported, c, t));
    let bank = acct
        .bank_balance_cents
        .zip(acct.bank_balance_as_of)
        .map(|(c, t)| (BalanceAnchor::Bank, c, t));
    match (reported, bank) {
        (Some(r), Some(b)) => Some(if b.2 > r.2 { b } else { r }),
        (r, b) => r.or(b),
    }
}

/// Running estimate for one account, run forward from its anchor balance.
///
/// Counts transactions with `date > as_of_date` **or** `pending`. Deleted rows
/// never count, nor do duplicates: when a shadow row and the row it shadows
/// live on the same account the charge is counted once, by the primary. A
/// shadow of a row on *another* account is still the bank's view of cash and
/// counts (the card row lives on an account excluded from cash).
pub fn account_estimate<'a>(
    acct: &AccountForEstimate,
    txns: impl IntoIterator<Item = &'a TxnForEstimate>,
    tz: Tz,
    today: NaiveDate,
) -> AccountEstimate {
    let anchor = anchor(acct);
    let as_of_date = anchor.map(|(_, _, t)| local_date(tz, t));
    let mut since = Cents::ZERO;
    let mut count = 0usize;
    if let Some(as_of) = as_of_date {
        for t in txns {
            if t.account_id != acct.id || t.deleted || t.duplicate {
                continue;
            }
            if t.pending || t.date > as_of {
                since += t.amount_cents;
                count += 1;
            }
        }
    }
    AccountEstimate {
        account_id: acct.id,
        name: acct.name.clone(),
        include_in_cash: acct.include_in_cash,
        reported_balance_cents: acct.reported_balance_cents,
        reported_balance_as_of: acct.reported_balance_as_of,
        bank_balance_cents: acct.bank_balance_cents,
        bank_balance_as_of: acct.bank_balance_as_of,
        anchor: anchor.map(|(k, _, _)| k),
        anchor_balance_cents: anchor.map(|(_, c, _)| c),
        anchor_as_of: anchor.map(|(_, _, t)| t),
        as_of_date,
        since_cents: since,
        since_count: count,
        estimate_cents: anchor.map(|(_, b, _)| b + since),
        anchor_age_days: as_of_date.map(|d| days_between(d, today)),
        reported_age_days: acct
            .reported_balance_as_of
            .map(|t| days_between(local_date(tz, t), today)),
    }
}

/// Household cash: sum of included accounts that have an estimate.
pub fn household_estimate(accounts: &[AccountEstimate]) -> Cents {
    accounts
        .iter()
        .filter(|a| a.include_in_cash)
        .filter_map(|a| a.estimate_cents)
        .sum()
}

/// Everyday-category fields the low point needs (for the current month).
#[derive(Clone, Debug)]
pub struct CategoryForLowPoint {
    pub id: Uuid,
    pub name: String,
    pub sign: Sign,
    pub plan_cents: Cents,
    /// Unsigned spend so far this month.
    pub spent_cents: Cents,
    pub is_everyday: bool,
}

/// A recurring bill due in the window.
#[derive(Clone, Debug)]
pub struct BillForLowPoint {
    pub id: Uuid,
    pub name: String,
    pub due: NaiveDate,
    pub amount_cents: Cents,
    /// Already matched to a posted transaction this cycle (so it's in the estimate).
    pub posted: bool,
}

#[derive(Clone, Debug)]
pub struct PlannedForLowPoint {
    pub id: Uuid,
    pub item: String,
    pub date: NaiveDate,
    pub amount_cents: Cents,
    /// `status == planned && matched_transaction_id.is_none()`
    pub outstanding: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillComponent {
    pub bill_id: Uuid,
    pub name: String,
    pub due_date: NaiveDate,
    pub amount_cents: Cents,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EverydayComponent {
    pub category_id: Uuid,
    pub name: String,
    /// Cents per day used for the projection.
    pub pace_cents_per_day: Cents,
    pub amount_cents: Cents,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedComponent {
    pub planned_item_id: Uuid,
    pub item: String,
    pub date: NaiveDate,
    pub amount_cents: Cents,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LowPoint {
    pub today: NaiveDate,
    pub next_payday: NaiveDate,
    pub days_until_payday: i64,
    pub estimate_cents: Cents,
    pub bills_due_cents: Cents,
    pub everyday_expected_cents: Cents,
    pub planned_items_cents: Cents,
    /// `estimate − bills − everyday − planned`.
    pub low_point_cents: Cents,
    pub bills: Vec<BillComponent>,
    pub everyday: Vec<EverydayComponent>,
    pub planned: Vec<PlannedComponent>,
}

/// Projected low point between `today` and `next_payday` (exclusive).
///
/// - Bills: active recurring bills due in `[today, next_payday)` that have not
///   posted yet contribute their amount.
/// - Everyday: `is_everyday` spend categories contribute
///   `pace × days_until_payday` where
///   `pace = max(spent / days_elapsed, plan / days_in_month)`.
/// - Planned: outstanding planned items dated before `next_payday` (including
///   overdue ones — they will still post).
pub fn projected_low_point(
    estimate: Cents,
    today: NaiveDate,
    next_payday: NaiveDate,
    categories: &[CategoryForLowPoint],
    bills: &[BillForLowPoint],
    planned: &[PlannedForLowPoint],
) -> LowPoint {
    let month = Month::of(today);
    let days_until = days_between(today, next_payday);
    let days_elapsed = today.day() as i64; // includes today
    let days_in_month = month.days_in_month() as i64;

    let mut bills_out: Vec<BillComponent> = bills
        .iter()
        .filter(|b| !b.posted && b.due >= today && b.due < next_payday && b.amount_cents.0 > 0)
        .map(|b| BillComponent {
            bill_id: b.id,
            name: b.name.clone(),
            due_date: b.due,
            amount_cents: b.amount_cents,
        })
        .collect();
    bills_out.sort_by_key(|b| b.due_date);

    let mut everyday = Vec::new();
    for c in categories
        .iter()
        .filter(|c| c.sign == Sign::Spend && c.is_everyday)
    {
        let actual_pace = c.spent_cents.div_round(days_elapsed);
        let plan_pace = c.plan_cents.div_round(days_in_month);
        let pace = actual_pace.max(plan_pace);
        let amt = Cents(pace.0 * days_until);
        if amt.0 > 0 {
            everyday.push(EverydayComponent {
                category_id: c.id,
                name: c.name.clone(),
                pace_cents_per_day: pace,
                amount_cents: amt,
            });
        }
    }

    let mut planned_out: Vec<PlannedComponent> = planned
        .iter()
        .filter(|p| p.outstanding && p.date < next_payday)
        .map(|p| PlannedComponent {
            planned_item_id: p.id,
            item: p.item.clone(),
            date: p.date,
            amount_cents: p.amount_cents,
        })
        .collect();
    planned_out.sort_by_key(|p| p.date);

    let bills_due: Cents = bills_out.iter().map(|b| b.amount_cents).sum();
    let everyday_expected: Cents = everyday.iter().map(|e| e.amount_cents).sum();
    let planned_total: Cents = planned_out.iter().map(|p| p.amount_cents).sum();

    LowPoint {
        today,
        next_payday,
        days_until_payday: days_until,
        estimate_cents: estimate,
        bills_due_cents: bills_due,
        everyday_expected_cents: everyday_expected,
        planned_items_cents: planned_total,
        low_point_cents: estimate - bills_due - everyday_expected - planned_total,
        bills: bills_out,
        everyday,
        planned: planned_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn chicago() -> Tz {
        "America/Chicago".parse().unwrap()
    }

    #[test]
    fn estimate_counts_after_as_of_and_pending() {
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();
        // Reported at 21:00 Chicago on Mar 10 → as_of_date Mar 10 (02:00 UTC Mar 11).
        let as_of = Utc.with_ymd_and_hms(2026, 3, 11, 2, 0, 0).unwrap();
        let acct = AccountForEstimate {
            id,
            name: "Demo Checking".into(),
            include_in_cash: true,
            reported_balance_cents: Some(Cents::dollars(1000)),
            reported_balance_as_of: Some(as_of),
            bank_balance_cents: None,
            bank_balance_as_of: None,
        };
        let t = |date: &str, amt: i64, pending: bool| TxnForEstimate {
            account_id: id,
            date: d(date),
            amount_cents: Cents::dollars(amt),
            pending,
            deleted: false,
            duplicate: false,
        };
        let txns = vec![
            t("2026-03-10", -50, false), // same day as as-of: already in the balance
            t("2026-03-11", -20, false), // after: counted
            t("2026-03-09", -30, true),  // pending, older date: counted
            t("2026-03-12", 500, false), // deposit after: counted
            TxnForEstimate {
                account_id: other,
                date: d("2026-03-12"),
                amount_cents: Cents::dollars(-999),
                pending: false,
                deleted: false,
                duplicate: false,
            },
            TxnForEstimate {
                account_id: id,
                date: d("2026-03-12"),
                amount_cents: Cents::dollars(-999),
                pending: false,
                deleted: true,
                duplicate: false,
            },
            // The bank's PwP line for a card charge already counted above.
            TxnForEstimate {
                account_id: id,
                date: d("2026-03-12"),
                amount_cents: Cents::dollars(-20),
                pending: false,
                deleted: false,
                duplicate: true,
            },
        ];
        let e = account_estimate(&acct, &txns, chicago(), d("2026-03-13"));
        assert_eq!(e.as_of_date, Some(d("2026-03-10")));
        assert_eq!(e.since_cents, Cents::dollars(450));
        assert_eq!(e.since_count, 3);
        assert_eq!(e.estimate_cents, Some(Cents::dollars(1450)));
        assert_eq!(e.anchor, Some(BalanceAnchor::Reported));
        assert_eq!(e.anchor_age_days, Some(3));
        assert_eq!(e.reported_age_days, Some(3));
    }

    #[test]
    fn estimate_none_without_a_dated_balance() {
        let id = Uuid::new_v4();
        let acct = AccountForEstimate {
            id,
            name: "x".into(),
            include_in_cash: true,
            reported_balance_cents: None,
            reported_balance_as_of: None,
            bank_balance_cents: Some(Cents(1)),
            bank_balance_as_of: None,
        };
        let e = account_estimate(&acct, &[], chicago(), d("2026-01-01"));
        assert_eq!(e.anchor, None);
        assert_eq!(e.estimate_cents, None);
        assert_eq!(household_estimate(&[e]), Cents::ZERO);
    }

    #[test]
    fn bank_balance_alone_anchors_the_estimate() {
        let id = Uuid::new_v4();
        let synced = Utc.with_ymd_and_hms(2026, 3, 11, 15, 0, 0).unwrap();
        let acct = AccountForEstimate {
            id,
            name: "Plaid Checking".into(),
            include_in_cash: true,
            reported_balance_cents: None,
            reported_balance_as_of: None,
            bank_balance_cents: Some(Cents::dollars(800)),
            bank_balance_as_of: Some(synced),
        };
        let txns = vec![
            TxnForEstimate {
                account_id: id,
                date: d("2026-03-12"),
                amount_cents: Cents::dollars(-25),
                pending: false,
                deleted: false,
                duplicate: false,
            },
            TxnForEstimate {
                account_id: id,
                date: d("2026-03-11"),
                amount_cents: Cents::dollars(-10),
                pending: true,
                deleted: false,
                duplicate: false,
            },
        ];
        let e = account_estimate(&acct, &txns, chicago(), d("2026-03-13"));
        assert_eq!(e.anchor, Some(BalanceAnchor::Bank));
        assert_eq!(e.anchor_balance_cents, Some(Cents::dollars(800)));
        assert_eq!(e.as_of_date, Some(d("2026-03-11")));
        assert_eq!(e.since_cents, Cents::dollars(-35));
        assert_eq!(e.estimate_cents, Some(Cents::dollars(765)));
        assert_eq!(e.anchor_age_days, Some(2));
        assert_eq!(e.reported_age_days, None);
        assert_eq!(household_estimate(&[e]), Cents::dollars(765));
    }

    #[test]
    fn newer_balance_wins_and_a_tie_goes_to_reported() {
        let id = Uuid::new_v4();
        let base = AccountForEstimate {
            id,
            name: "x".into(),
            include_in_cash: true,
            reported_balance_cents: Some(Cents::dollars(100)),
            reported_balance_as_of: Some(Utc.with_ymd_and_hms(2026, 3, 10, 12, 0, 0).unwrap()),
            bank_balance_cents: Some(Cents::dollars(200)),
            bank_balance_as_of: Some(Utc.with_ymd_and_hms(2026, 3, 11, 12, 0, 0).unwrap()),
        };
        let e = account_estimate(&base, &[], chicago(), d("2026-03-12"));
        assert_eq!(e.anchor, Some(BalanceAnchor::Bank));
        assert_eq!(e.estimate_cents, Some(Cents::dollars(200)));

        let reported_later = AccountForEstimate {
            reported_balance_as_of: Some(Utc.with_ymd_and_hms(2026, 3, 11, 18, 0, 0).unwrap()),
            ..base.clone()
        };
        let e = account_estimate(&reported_later, &[], chicago(), d("2026-03-12"));
        assert_eq!(e.anchor, Some(BalanceAnchor::Reported));
        assert_eq!(e.estimate_cents, Some(Cents::dollars(100)));

        let tie = AccountForEstimate {
            reported_balance_as_of: base.bank_balance_as_of,
            ..base.clone()
        };
        let e = account_estimate(&tie, &[], chicago(), d("2026-03-12"));
        assert_eq!(e.anchor, Some(BalanceAnchor::Reported));
    }

    fn cat(name: &str, plan: i64, spent: i64, everyday: bool) -> CategoryForLowPoint {
        CategoryForLowPoint {
            id: Uuid::new_v4(),
            name: name.into(),
            sign: Sign::Spend,
            plan_cents: Cents::dollars(plan),
            spent_cents: Cents::dollars(spent),
            is_everyday: everyday,
        }
    }

    fn bill(name: &str, due: &str, dollars: i64, posted: bool) -> BillForLowPoint {
        BillForLowPoint {
            id: Uuid::new_v4(),
            name: name.into(),
            due: d(due),
            amount_cents: Cents::dollars(dollars),
            posted,
        }
    }

    #[test]
    fn low_point_components() {
        // Today Mar 10 (31-day month), payday Mar 20 → 10 days.
        let today = d("2026-03-10");
        let payday = d("2026-03-20");
        let cats = vec![
            cat("Groceries", 620, 300, true), // actual pace 30/day > plan pace 20/day → 300
            cat("Fuel", 310, 50, true),       // actual pace 5/day < plan pace 10/day → 100
            cat("Rent", 1500, 1500, false),   // not everyday → ignored here
            CategoryForLowPoint {
                // income categories are ignored
                id: Uuid::new_v4(),
                name: "Pay".into(),
                sign: Sign::Income,
                plan_cents: Cents::dollars(5000),
                spent_cents: Cents::ZERO,
                is_everyday: true,
            },
        ];
        let bills = vec![
            bill("Rent", "2026-04-01", 1500, false), // outside window
            bill("Power", "2026-03-15", 150, false), // in window → 150
            bill("Water", "2026-03-12", 60, true),   // already posted → 0
            bill("Today", "2026-03-10", 10, false),  // due today counts
            bill("Payday", "2026-03-20", 99, false), // due on payday does not
        ];
        let planned = vec![
            PlannedForLowPoint {
                id: Uuid::new_v4(),
                item: "Tires".into(),
                date: d("2026-03-18"),
                amount_cents: Cents::dollars(400),
                outstanding: true,
            },
            PlannedForLowPoint {
                id: Uuid::new_v4(),
                item: "Overdue thing".into(),
                date: d("2026-03-02"),
                amount_cents: Cents::dollars(25),
                outstanding: true,
            },
            PlannedForLowPoint {
                id: Uuid::new_v4(),
                item: "Done".into(),
                date: d("2026-03-15"),
                amount_cents: Cents::dollars(999),
                outstanding: false,
            },
            PlannedForLowPoint {
                id: Uuid::new_v4(),
                item: "After payday".into(),
                date: d("2026-03-25"),
                amount_cents: Cents::dollars(999),
                outstanding: true,
            },
        ];
        let lp = projected_low_point(Cents::dollars(2000), today, payday, &cats, &bills, &planned);
        assert_eq!(lp.days_until_payday, 10);
        assert_eq!(lp.bills.len(), 2);
        assert_eq!(lp.bills[0].name, "Today");
        assert_eq!(lp.bills_due_cents, Cents::dollars(160));
        assert_eq!(lp.everyday.len(), 2);
        assert_eq!(lp.everyday[0].pace_cents_per_day, Cents::dollars(30));
        assert_eq!(lp.everyday_expected_cents, Cents::dollars(400));
        assert_eq!(lp.planned.len(), 2);
        assert_eq!(lp.planned[0].item, "Overdue thing");
        assert_eq!(lp.planned_items_cents, Cents::dollars(425));
        assert_eq!(lp.low_point_cents, Cents::dollars(2000 - 160 - 400 - 425));
    }
}
