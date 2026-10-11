//! Recurring bills: cadence math, due dates, matching, and the numbers the
//! budget, low point and sinking fund derive from them.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::calendar::Month;
use crate::money::Cents;
use crate::rules::normalize;
use crate::settings::{BillMatch, PaySchedule};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillStatus {
    Active,
    Paused,
    Canceled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bill {
    pub id: Uuid,
    pub name: String,
    pub amount_cents: Cents,
    /// Months between occurrences: 1 monthly, 3 quarterly, 12 yearly.
    /// Ignored when `per_paycheck`.
    pub cadence_months: u32,
    /// Due on every payday of the household pay schedule (so three times in
    /// a three-paycheck month).
    pub per_paycheck: bool,
    pub next_due: NaiveDate,
    pub category_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    /// Substring matched against the normalized description; `None` = name.
    pub match_pattern: Option<String>,
    pub status: BillStatus,
}

impl Bill {
    /// What this bill costs in a month: a per-paycheck bill its amount times
    /// the paydays in that month; otherwise the amount spread over its
    /// cadence (rounded up).
    pub fn monthly_equivalent(&self, paychecks_in_month: u32) -> Cents {
        if self.per_paycheck {
            Cents(self.amount_cents.0 * paychecks_in_month as i64)
        } else {
            self.amount_cents
                .div_ceil(self.cadence_months.max(1) as i64)
        }
    }

    /// Posts at least monthly (so it is not a sinking-fund item).
    pub fn is_monthly(&self) -> bool {
        self.per_paycheck || self.cadence_months <= 1
    }

    /// The due date after `due`: the next payday for a per-paycheck bill,
    /// else one cadence on.
    pub fn next_occurrence(&self, due: NaiveDate, schedule: &PaySchedule) -> NaiveDate {
        if self.per_paycheck {
            schedule.next_payday(due)
        } else {
            advance(due, self.cadence_months)
        }
    }

    /// Advance `due` until it is on or after `from`. Returns the number of steps.
    pub fn catch_up(
        &self,
        mut due: NaiveDate,
        schedule: &PaySchedule,
        from: NaiveDate,
    ) -> (NaiveDate, u32) {
        let mut steps = 0;
        while due < from && steps < 1200 {
            due = self.next_occurrence(due, schedule);
            steps += 1;
        }
        (due, steps)
    }

    fn pattern(&self) -> String {
        normalize(self.match_pattern.as_deref().unwrap_or(&self.name))
    }
}

/// The due date one cadence after `due`, keeping the day of month where the
/// target month allows (the 31st becomes the 28th/29th/30th as needed).
pub fn advance(due: NaiveDate, cadence_months: u32) -> NaiveDate {
    use chrono::Datelike;
    Month::of(due)
        .add_months(cadence_months.max(1) as i32)
        .date_with_day_clamped(due.day())
}

/// Advance `next_due` until it is on or after `from` (a bill that was never
/// matched for several cycles catches up). Returns the number of steps.
pub fn catch_up(mut due: NaiveDate, cadence_months: u32, from: NaiveDate) -> (NaiveDate, u32) {
    let mut steps = 0;
    while due < from && steps < 1200 {
        due = advance(due, cadence_months);
        steps += 1;
    }
    (due, steps)
}

/// Sum of monthly equivalents of active bills per category, for a month
/// with `paychecks_in_month` paydays (per-paycheck bills scale with it).
pub fn monthly_total_by_category(
    bills: &[Bill],
    paychecks_in_month: u32,
) -> std::collections::HashMap<Uuid, Cents> {
    let mut out = std::collections::HashMap::new();
    for b in bills.iter().filter(|b| b.status == BillStatus::Active) {
        if let Some(c) = b.category_id {
            *out.entry(c).or_insert(Cents::ZERO) += b.monthly_equivalent(paychecks_in_month);
        }
    }
    out
}

/// Active bills due in `[today, until)` — the "bills due before payday" set.
pub fn due_before(bills: &[Bill], today: NaiveDate, until: NaiveDate) -> Vec<&Bill> {
    let mut v: Vec<&Bill> = bills
        .iter()
        .filter(|b| b.status == BillStatus::Active && b.next_due >= today && b.next_due < until)
        .collect();
    v.sort_by_key(|b| b.next_due);
    v
}

/// A transaction as the bill matcher sees it.
#[derive(Clone, Debug)]
pub struct TxnForBills {
    pub date: NaiveDate,
    pub description: String,
    /// Signed; only outflows match.
    pub amount_cents: Cents,
    pub account_id: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchQuality {
    /// Amount within tolerance.
    Exact,
    /// Pattern and date match but the amount drifted past tolerance.
    AmountChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BillMatchResult<'a> {
    pub bill: &'a Bill,
    pub quality: MatchQuality,
    /// `abs(amount) − bill.amount` (positive = charged more).
    pub drift_cents: Cents,
}

/// Find the bill a transaction pays. Pattern must match; date within
/// `cfg.days` of `next_due` (or, for a canceled bill, any date); the account
/// must match when the bill names one. Active bills win over paused ones;
/// canceled bills are returned too so the caller can raise the alert.
pub fn find_match<'a>(
    txn: &TxnForBills,
    bills: &'a [Bill],
    cfg: &BillMatch,
) -> Option<BillMatchResult<'a>> {
    if txn.amount_cents.0 >= 0 {
        return None;
    }
    let desc = normalize(&txn.description);
    let spent = txn.amount_cents.abs();
    let mut best: Option<(i64, i64, BillMatchResult)> = None;
    for b in bills {
        if !desc.contains(&b.pattern()) {
            continue;
        }
        if let Some(a) = b.account_id
            && a != txn.account_id
        {
            continue;
        }
        let dd = (txn.date - b.next_due).num_days().abs();
        if b.status != BillStatus::Canceled && dd > cfg.days {
            continue;
        }
        let drift = spent - b.amount_cents;
        let tol = b.amount_cents.mul_div(cfg.pct, 100).max(cfg.min_cents);
        let quality = if drift.abs() <= tol {
            MatchQuality::Exact
        } else {
            MatchQuality::AmountChanged
        };
        // Rank: active first, then closest date, then smallest drift.
        let rank = match b.status {
            BillStatus::Active => 0,
            BillStatus::Paused => 1,
            BillStatus::Canceled => 2,
        };
        let key = (rank * 10_000 + dd, drift.abs().0);
        let better = match &best {
            None => true,
            Some((r, d, _)) => (key.0, key.1) < (*r, *d),
        };
        if better {
            best = Some((
                key.0,
                key.1,
                BillMatchResult {
                    bill: b,
                    quality,
                    drift_cents: drift,
                },
            ));
        }
    }
    best.map(|(_, _, m)| m)
}

/// Active bills past due by more than `cfg.not_seen_after_days`.
pub fn overdue<'a>(bills: &'a [Bill], today: NaiveDate, cfg: &BillMatch) -> Vec<&'a Bill> {
    bills
        .iter()
        .filter(|b| {
            b.status == BillStatus::Active
                && (today - b.next_due).num_days() > cfg.not_seen_after_days
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn bill(name: &str, dollars: i64, cadence: u32, due: &str, status: BillStatus) -> Bill {
        Bill {
            id: Uuid::new_v4(),
            name: name.into(),
            amount_cents: Cents::dollars(dollars),
            cadence_months: cadence,
            per_paycheck: false,
            next_due: d(due),
            category_id: Some(Uuid::new_v4()),
            account_id: None,
            match_pattern: None,
            status,
        }
    }

    #[test]
    fn monthly_equivalent_rounds_up() {
        let b = bill("Insurance", 640, 12, "2027-03-15", BillStatus::Active);
        assert_eq!(b.monthly_equivalent(2), Cents(5334));
        assert!(!b.is_monthly());
        assert!(bill("Gym", 45, 1, "2026-11-03", BillStatus::Active).is_monthly());
    }

    #[test]
    fn per_paycheck_bills_follow_the_paydays() {
        // Fridays every two weeks from Oct 2, 2026: Oct 2, 16, 30; Nov 13, 27.
        let schedule = PaySchedule::Biweekly {
            anchor: d("2026-10-02"),
        };
        let mut giving = bill("Giving", 282, 1, "2026-10-16", BillStatus::Active);
        giving.per_paycheck = true;
        assert!(giving.is_monthly());
        assert_eq!(giving.monthly_equivalent(3), Cents::dollars(846));
        assert_eq!(giving.monthly_equivalent(2), Cents::dollars(564));
        assert_eq!(
            giving.next_occurrence(d("2026-10-16"), &schedule),
            d("2026-10-30")
        );
        // Paid on the 16th: the next due is the 30th. Missed until Nov 20: Nov 27.
        assert_eq!(
            giving.catch_up(d("2026-10-30"), &schedule, d("2026-10-17")),
            (d("2026-10-30"), 0)
        );
        assert_eq!(
            giving.catch_up(d("2026-10-30"), &schedule, d("2026-11-20")),
            (d("2026-11-27"), 2)
        );
        // A monthly bill advances by the calendar as before.
        let rent = bill("Rent", 1500, 1, "2026-11-01", BillStatus::Active);
        assert_eq!(
            rent.next_occurrence(d("2026-11-01"), &schedule),
            d("2026-12-01")
        );
        let c = giving.category_id.unwrap();
        assert_eq!(
            monthly_total_by_category(std::slice::from_ref(&giving), 3)[&c],
            Cents::dollars(846)
        );
    }

    #[test]
    fn advance_and_catch_up() {
        assert_eq!(advance(d("2026-01-31"), 1), d("2026-02-28"));
        assert_eq!(advance(d("2026-03-15"), 12), d("2027-03-15"));
        assert_eq!(advance(d("2026-11-30"), 3), d("2027-02-28"));
        let (due, steps) = catch_up(d("2026-01-14"), 1, d("2026-04-02"));
        assert_eq!((due, steps), (d("2026-04-14"), 3));
        assert_eq!(
            catch_up(d("2026-05-01"), 1, d("2026-04-02")),
            (d("2026-05-01"), 0)
        );
    }

    #[test]
    fn totals_and_due_before() {
        let c = Uuid::new_v4();
        let mut a = bill("Gym", 45, 1, "2026-10-14", BillStatus::Active);
        a.category_id = Some(c);
        let mut b = bill("Insurance", 600, 12, "2027-03-15", BillStatus::Active);
        b.category_id = Some(c);
        let mut p = bill("Paused", 99, 1, "2026-10-13", BillStatus::Paused);
        p.category_id = Some(c);
        let bills = vec![a.clone(), b, p];
        assert_eq!(monthly_total_by_category(&bills, 2)[&c], Cents::dollars(95));
        let due = due_before(&bills, d("2026-10-11"), d("2026-10-15"));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, a.id);
        assert!(due_before(&bills, d("2026-10-15"), d("2026-10-31")).is_empty());
    }

    #[test]
    fn matching() {
        let cfg = BillMatch::default();
        let gym = bill("Example Gym", 45, 1, "2026-10-14", BillStatus::Active);
        let mut stream = bill("Demo Video", 1599, 1, "2026-10-09", BillStatus::Canceled);
        stream.amount_cents = Cents(1599);
        stream.match_pattern = Some("DEMO VIDEO".into());
        let bills = vec![gym.clone(), stream.clone()];
        let acct = Uuid::new_v4();

        let t = TxnForBills {
            date: d("2026-10-15"),
            description: "EXAMPLE GYM MEMBERSHIP".into(),
            amount_cents: Cents::dollars(-45),
            account_id: acct,
        };
        let m = find_match(&t, &bills, &cfg).unwrap();
        assert_eq!(m.bill.id, gym.id);
        assert_eq!(m.quality, MatchQuality::Exact);

        let t = TxnForBills {
            date: d("2026-10-15"),
            description: "EXAMPLE GYM".into(),
            amount_cents: Cents::dollars(-59),
            account_id: acct,
        };
        let m = find_match(&t, &bills, &cfg).unwrap();
        assert_eq!(m.quality, MatchQuality::AmountChanged);
        assert_eq!(m.drift_cents, Cents::dollars(14));

        let t = TxnForBills {
            date: d("2026-10-25"),
            description: "EXAMPLE GYM".into(),
            amount_cents: Cents::dollars(-45),
            account_id: acct,
        };
        assert!(find_match(&t, &bills, &cfg).is_none()); // 11 days off

        let t = TxnForBills {
            date: d("2026-12-01"),
            description: "Demo Video 1234".into(),
            amount_cents: Cents(-1599),
            account_id: acct,
        };
        let m = find_match(&t, &bills, &cfg).unwrap(); // canceled bills match on any date
        assert_eq!(m.bill.id, stream.id);

        let t = TxnForBills {
            date: d("2026-10-14"),
            description: "EXAMPLE GYM".into(),
            amount_cents: Cents::dollars(45),
            account_id: acct,
        };
        assert!(find_match(&t, &bills, &cfg).is_none()); // inflow

        let mut scoped = gym.clone();
        scoped.account_id = Some(Uuid::new_v4());
        let t = TxnForBills {
            date: d("2026-10-14"),
            description: "EXAMPLE GYM".into(),
            amount_cents: Cents::dollars(-45),
            account_id: acct,
        };
        assert!(find_match(&t, std::slice::from_ref(&scoped), &cfg).is_none());
    }

    #[test]
    fn overdue_bills() {
        let cfg = BillMatch::default(); // 5 days
        let late = bill("Power", 160, 1, "2026-10-01", BillStatus::Active);
        let fine = bill("Water", 60, 1, "2026-10-08", BillStatus::Active);
        let canceled = bill("Old", 10, 1, "2026-01-01", BillStatus::Canceled);
        let bills = [late.clone(), fine, canceled];
        let v = overdue(&bills, d("2026-10-11"), &cfg);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, late.id);
    }
}
