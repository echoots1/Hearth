//! The cash timeline: every expected paycheck, bill, planned one-off and the
//! everyday drain from today to a horizon, in date order, with the running
//! balance after each. The low point before the next payday is the first
//! stretch of it; this is the same arithmetic carried further out.

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bills::advance;
use crate::budget::{PlanPhase, Sign, effective_plan, monthly_plan};
use crate::calendar::Month;
use crate::money::Cents;
use crate::settings::PaySchedule;

/// Category fields the timeline needs (every non-archived category).
#[derive(Clone, Debug)]
pub struct CategoryForTimeline {
    pub id: Uuid,
    pub name: String,
    pub sign: Sign,
    pub is_everyday: bool,
    pub plan_per_paycheck: bool,
    pub phases: Vec<PlanPhase>,
    /// Unsigned activity so far in the current month (spent, or received).
    pub activity_cents: Cents,
}

/// An active recurring bill. Occurrences are generated from `next_due` by
/// its cadence; `next_due` only moves when a charge posts, so a bill past due
/// with no match is still expected.
#[derive(Clone, Debug)]
pub struct BillForTimeline {
    pub id: Uuid,
    pub name: String,
    pub next_due: NaiveDate,
    pub cadence_months: u32,
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
}

/// An outstanding planned one-off (planned, not yet matched).
#[derive(Clone, Debug)]
pub struct PlannedForTimeline {
    pub id: Uuid,
    pub item: String,
    pub date: NaiveDate,
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind {
    Paycheck,
    Bill,
    Planned,
    /// Everyday-category spending at its pace over a run of days.
    Everyday,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// When it lands. An overdue bill or one-off is dated today; an everyday
    /// run is dated its last day.
    pub date: NaiveDate,
    pub kind: TimelineKind,
    pub label: String,
    /// Signed: income positive, money out negative.
    pub amount_cents: Cents,
    pub balance_after_cents: Cents,
    /// Past its due date with no matching charge yet.
    pub overdue: bool,
    /// The original due date of an overdue item.
    pub due_date: Option<NaiveDate>,
    /// Days an everyday run covers.
    pub days: Option<i64>,
    pub bill_id: Option<Uuid>,
    pub planned_item_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeline {
    pub today: NaiveDate,
    /// Last day covered (inclusive).
    pub until: NaiveDate,
    /// The household estimate the timeline runs from.
    pub start_cents: Cents,
    pub end_cents: Cents,
    /// The lowest running balance in the window and when it happens.
    pub low_cents: Cents,
    pub low_date: NaiveDate,
    pub income_cents: Cents,
    pub bills_cents: Cents,
    pub planned_cents: Cents,
    pub everyday_cents: Cents,
    /// Paydays in the window (strictly after today).
    pub paydays: Vec<NaiveDate>,
    pub overdue_count: usize,
    /// This month's plan for income categories that are not per paycheck.
    /// The timeline cannot place it on a day, so it is left out and reported.
    pub unscheduled_income_monthly_cents: Cents,
    pub entries: Vec<TimelineEntry>,
}

struct Event {
    date: NaiveDate,
    kind: TimelineKind,
    label: String,
    amount_cents: Cents,
    overdue: bool,
    due_date: Option<NaiveDate>,
    bill_id: Option<Uuid>,
    planned_item_id: Option<Uuid>,
    category_id: Option<Uuid>,
}

fn kind_order(k: TimelineKind) -> u8 {
    match k {
        TimelineKind::Paycheck => 0,
        TimelineKind::Bill => 1,
        TimelineKind::Planned => 2,
        TimelineKind::Everyday => 3,
    }
}

/// Cents per day an everyday category drains on a given date: this month,
/// the larger of its actual pace and its plan pace (pessimistic, like the low
/// point); later months, the plan pace of that month.
fn everyday_pace(
    c: &CategoryForTimeline,
    date: NaiveDate,
    today: NaiveDate,
    paychecks: &mut HashMap<Month, u32>,
    schedule: &PaySchedule,
) -> Cents {
    let month = Month::of(date);
    let n = *paychecks
        .entry(month)
        .or_insert_with(|| schedule.paydays_in_month(month).len() as u32);
    let plan = monthly_plan(&c.phases, month, c.plan_per_paycheck, n);
    let plan_pace = plan.div_round(month.days_in_month() as i64);
    if month == Month::of(today) {
        let actual_pace = c.activity_cents.div_round(today.day() as i64);
        actual_pace.max(plan_pace)
    } else {
        plan_pace
    }
}

/// Build the timeline from `today` through `until` (inclusive).
///
/// - Paychecks: each payday strictly after today credits every income
///   category whose plan is per paycheck with that month's phase amount.
/// - Bills: each occurrence of an active bill from `next_due` by its cadence.
///   A `next_due` before today is overdue and lands today.
/// - Planned: outstanding one-offs on their date; overdue ones land today.
/// - Everyday: the drain of everyday spend categories accrues day by day and
///   is shown as one row before each dated event and one at the end.
pub fn cash_timeline(
    start_cents: Cents,
    today: NaiveDate,
    until: NaiveDate,
    schedule: &PaySchedule,
    categories: &[CategoryForTimeline],
    bills: &[BillForTimeline],
    planned: &[PlannedForTimeline],
) -> Timeline {
    let until = until.max(today);
    let mut paychecks: HashMap<Month, u32> = HashMap::new();
    let mut events: Vec<Event> = Vec::new();

    let paydays = schedule.paydays_between(today, until);
    for p in &paydays {
        for c in categories
            .iter()
            .filter(|c| c.sign == Sign::Income && c.plan_per_paycheck)
        {
            let amt = effective_plan(&c.phases, Month::of(*p));
            if amt.0 > 0 {
                events.push(Event {
                    date: *p,
                    kind: TimelineKind::Paycheck,
                    label: c.name.clone(),
                    amount_cents: amt,
                    overdue: false,
                    due_date: None,
                    bill_id: None,
                    planned_item_id: None,
                    category_id: Some(c.id),
                });
            }
        }
    }

    for b in bills.iter().filter(|b| b.amount_cents.0 > 0) {
        let mut due = b.next_due;
        let mut first = true;
        let mut steps = 0;
        while due <= until && steps < 400 {
            if due >= today {
                events.push(Event {
                    date: due,
                    kind: TimelineKind::Bill,
                    label: b.name.clone(),
                    amount_cents: -b.amount_cents,
                    overdue: false,
                    due_date: None,
                    bill_id: Some(b.id),
                    planned_item_id: None,
                    category_id: b.category_id,
                });
            } else if first {
                events.push(Event {
                    date: today,
                    kind: TimelineKind::Bill,
                    label: b.name.clone(),
                    amount_cents: -b.amount_cents,
                    overdue: true,
                    due_date: Some(due),
                    bill_id: Some(b.id),
                    planned_item_id: None,
                    category_id: b.category_id,
                });
            }
            first = false;
            due = advance(due, b.cadence_months);
            steps += 1;
        }
    }

    for p in planned.iter().filter(|p| p.date <= until) {
        let overdue = p.date < today;
        events.push(Event {
            date: if overdue { today } else { p.date },
            kind: TimelineKind::Planned,
            label: p.item.clone(),
            amount_cents: -p.amount_cents,
            overdue,
            due_date: overdue.then_some(p.date),
            bill_id: None,
            planned_item_id: Some(p.id),
            category_id: p.category_id,
        });
    }

    events.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then_with(|| kind_order(a.kind).cmp(&kind_order(b.kind)))
            .then_with(|| a.label.cmp(&b.label))
    });

    let everyday: Vec<&CategoryForTimeline> = categories
        .iter()
        .filter(|c| c.sign == Sign::Spend && c.is_everyday)
        .collect();

    let mut entries: Vec<TimelineEntry> = Vec::with_capacity(events.len() * 2 + 1);
    let mut balance = start_cents;
    let mut totals = (Cents::ZERO, Cents::ZERO, Cents::ZERO, Cents::ZERO);
    let mut low = (start_cents, today);
    let mut cursor = today;

    // Accrue everyday spending for every day in [from, to) as one row.
    let mut accrue = |from: NaiveDate,
                      to: NaiveDate,
                      balance: &mut Cents,
                      entries: &mut Vec<TimelineEntry>,
                      low: &mut (Cents, NaiveDate),
                      everyday_total: &mut Cents| {
        let days = (to - from).num_days();
        if days <= 0 {
            return;
        }
        let mut amt = Cents::ZERO;
        let mut d = from;
        while d < to {
            for c in &everyday {
                amt += everyday_pace(c, d, today, &mut paychecks, schedule);
            }
            d = d.succ_opt().expect("date in range");
        }
        if amt.0 <= 0 {
            return;
        }
        *balance -= amt;
        *everyday_total += amt;
        let last = to.pred_opt().expect("date in range");
        entries.push(TimelineEntry {
            date: last,
            kind: TimelineKind::Everyday,
            label: "Everyday spending".into(),
            amount_cents: -amt,
            balance_after_cents: *balance,
            overdue: false,
            due_date: None,
            days: Some(days),
            bill_id: None,
            planned_item_id: None,
            category_id: None,
        });
        if *balance < low.0 {
            *low = (*balance, last);
        }
    };

    let mut overdue_count = 0;
    for e in events {
        if e.date > cursor {
            accrue(
                cursor,
                e.date,
                &mut balance,
                &mut entries,
                &mut low,
                &mut totals.3,
            );
            cursor = e.date;
        }
        balance += e.amount_cents;
        match e.kind {
            TimelineKind::Paycheck => totals.0 += e.amount_cents,
            TimelineKind::Bill => totals.1 -= e.amount_cents,
            TimelineKind::Planned => totals.2 -= e.amount_cents,
            TimelineKind::Everyday => {}
        }
        if e.overdue {
            overdue_count += 1;
        }
        entries.push(TimelineEntry {
            date: e.date,
            kind: e.kind,
            label: e.label,
            amount_cents: e.amount_cents,
            balance_after_cents: balance,
            overdue: e.overdue,
            due_date: e.due_date,
            days: None,
            bill_id: e.bill_id,
            planned_item_id: e.planned_item_id,
            category_id: e.category_id,
        });
        if balance < low.0 {
            low = (balance, e.date);
        }
    }
    accrue(
        cursor,
        until.succ_opt().expect("date in range"),
        &mut balance,
        &mut entries,
        &mut low,
        &mut totals.3,
    );

    let this_month = Month::of(today);
    let unscheduled: Cents = categories
        .iter()
        .filter(|c| c.sign == Sign::Income && !c.plan_per_paycheck)
        .map(|c| effective_plan(&c.phases, this_month))
        .sum();

    Timeline {
        today,
        until,
        start_cents,
        end_cents: balance,
        low_cents: low.0,
        low_date: low.1,
        income_cents: totals.0,
        bills_cents: totals.1,
        planned_cents: totals.2,
        everyday_cents: totals.3,
        paydays,
        overdue_count,
        unscheduled_income_monthly_cents: unscheduled,
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::balance::{
        BillForLowPoint, CategoryForLowPoint, PlannedForLowPoint, projected_low_point,
    };

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn phase(s: &str, dollars: i64) -> PlanPhase {
        PlanPhase {
            starts_month: s.parse().unwrap(),
            amount_cents: Cents::dollars(dollars),
            label: None,
        }
    }

    fn income(name: &str, per_paycheck: Option<i64>, monthly: Option<i64>) -> CategoryForTimeline {
        CategoryForTimeline {
            id: Uuid::new_v4(),
            name: name.into(),
            sign: Sign::Income,
            is_everyday: false,
            plan_per_paycheck: per_paycheck.is_some(),
            phases: vec![phase("2026-01", per_paycheck.or(monthly).unwrap_or(0))],
            activity_cents: Cents::ZERO,
        }
    }

    fn everyday(name: &str, plan: i64, spent: i64) -> CategoryForTimeline {
        CategoryForTimeline {
            id: Uuid::new_v4(),
            name: name.into(),
            sign: Sign::Spend,
            is_everyday: true,
            plan_per_paycheck: false,
            phases: vec![phase("2026-01", plan)],
            activity_cents: Cents::dollars(spent),
        }
    }

    fn bill(name: &str, due: &str, dollars: i64, cadence: u32) -> BillForTimeline {
        BillForTimeline {
            id: Uuid::new_v4(),
            name: name.into(),
            next_due: d(due),
            cadence_months: cadence,
            amount_cents: Cents::dollars(dollars),
            category_id: None,
        }
    }

    fn planned(item: &str, date: &str, dollars: i64) -> PlannedForTimeline {
        PlannedForTimeline {
            id: Uuid::new_v4(),
            item: item.into(),
            date: d(date),
            amount_cents: Cents::dollars(dollars),
            category_id: None,
        }
    }

    fn biweekly() -> PaySchedule {
        // Fridays: Oct 2, 16, 30 (three in October 2026), Nov 13, 27, Dec 11, 25.
        PaySchedule::Biweekly {
            anchor: d("2026-10-02"),
        }
    }

    #[test]
    fn three_paycheck_month_and_running_balance() {
        let today = d("2026-10-11");
        let cats = vec![
            income("Paycheck", Some(1800), None),
            income("Side gig", None, Some(300)),
            everyday("Groceries", 620, 330), // 31-day month: plan 20/day, actual 330/11 = 30/day
        ];
        let bills = vec![
            bill("Rent", "2026-11-01", 1500, 1),
            bill("Power", "2026-10-15", 150, 1),
            bill("Insurance", "2026-12-01", 600, 6),
        ];
        let planned = vec![planned("Tires", "2026-10-20", 400)];
        let t = cash_timeline(
            Cents::dollars(2000),
            today,
            d("2026-11-15"),
            &biweekly(),
            &cats,
            &bills,
            &planned,
        );
        assert_eq!(
            t.paydays,
            vec![d("2026-10-16"), d("2026-10-30"), d("2026-11-13")]
        );
        let paychecks: Vec<&TimelineEntry> = t
            .entries
            .iter()
            .filter(|e| e.kind == TimelineKind::Paycheck)
            .collect();
        assert_eq!(paychecks.len(), 3);
        assert!(
            paychecks
                .iter()
                .all(|e| e.amount_cents == Cents::dollars(1800))
        );
        assert_eq!(t.income_cents, Cents::dollars(5400));
        assert_eq!(t.unscheduled_income_monthly_cents, Cents::dollars(300));
        // Bills: Power Oct 15 and Nov 15 (two occurrences), Rent Nov 1; Insurance is past the horizon.
        assert_eq!(t.bills_cents, Cents::dollars(150 + 150 + 1500));
        assert_eq!(t.planned_cents, Cents::dollars(400));
        assert_eq!(t.overdue_count, 0);
        // The first rows: everyday Oct 11–14 (4 days × $30) then Power on the 15th.
        assert_eq!(t.entries[0].kind, TimelineKind::Everyday);
        assert_eq!(t.entries[0].days, Some(4));
        assert_eq!(t.entries[0].date, d("2026-10-14"));
        assert_eq!(t.entries[0].amount_cents, Cents::dollars(-120));
        assert_eq!(t.entries[0].balance_after_cents, Cents::dollars(1880));
        assert_eq!(t.entries[1].label, "Power");
        assert_eq!(t.entries[1].balance_after_cents, Cents::dollars(1730));
        // Everyday Oct 15 (1 day) then the paycheck on the 16th.
        assert_eq!(t.entries[2].kind, TimelineKind::Everyday);
        assert_eq!(t.entries[2].days, Some(1));
        assert_eq!(t.entries[3].kind, TimelineKind::Paycheck);
        assert_eq!(
            t.entries[3].balance_after_cents,
            Cents::dollars(1730 - 30 + 1800)
        );
        // End balance is the sum of the parts.
        assert_eq!(
            t.end_cents,
            t.start_cents + t.income_cents - t.bills_cents - t.planned_cents - t.everyday_cents
        );
        assert_eq!(t.entries.last().unwrap().date, d("2026-11-15"));
        assert_eq!(t.entries.last().unwrap().balance_after_cents, t.end_cents);
        // Every row's balance follows from the previous one.
        let mut bal = t.start_cents;
        for e in &t.entries {
            bal += e.amount_cents;
            assert_eq!(e.balance_after_cents, bal);
        }
        // The low point is the smallest balance shown, and never above the start.
        let min = t
            .entries
            .iter()
            .map(|e| e.balance_after_cents)
            .min()
            .unwrap();
        assert_eq!(t.low_cents, min.min(t.start_cents));
        // The window never goes under zero here.
        assert!(t.low_cents.0 > 0);
    }

    #[test]
    fn agrees_with_the_low_point_before_the_next_payday() {
        let today = d("2026-10-11");
        let payday = d("2026-10-16");
        let cats = vec![
            income("Paycheck", Some(1800), None),
            everyday("Groceries", 620, 300),
            everyday("Fuel", 310, 50),
        ];
        let bills = vec![
            bill("Power", "2026-10-15", 150, 1),
            bill("Missed", "2026-10-04", 40, 1), // overdue, not posted
            bill("Payday", "2026-10-16", 99, 1), // due on payday: after the low point
        ];
        let planned = vec![
            planned("Overdue thing", "2026-10-02", 25),
            planned("Tires", "2026-10-14", 400),
        ];
        let t = cash_timeline(
            Cents::dollars(2000),
            today,
            d("2026-11-30"),
            &biweekly(),
            &cats,
            &bills,
            &planned,
        );
        let lp_cats: Vec<CategoryForLowPoint> = cats
            .iter()
            .map(|c| CategoryForLowPoint {
                id: c.id,
                name: c.name.clone(),
                sign: c.sign,
                plan_cents: effective_plan(&c.phases, Month::of(today)),
                spent_cents: c.activity_cents,
                is_everyday: c.is_everyday,
            })
            .collect();
        let lp_bills: Vec<BillForLowPoint> = bills
            .iter()
            .map(|b| BillForLowPoint {
                id: b.id,
                name: b.name.clone(),
                due: b.next_due,
                amount_cents: b.amount_cents,
                posted: false,
            })
            .collect();
        let lp_planned: Vec<PlannedForLowPoint> = planned
            .iter()
            .map(|p| PlannedForLowPoint {
                id: p.id,
                item: p.item.clone(),
                date: p.date,
                amount_cents: p.amount_cents,
                outstanding: true,
            })
            .collect();
        let lp = projected_low_point(
            Cents::dollars(2000),
            today,
            payday,
            &lp_cats,
            &lp_bills,
            &lp_planned,
        );
        // The last row before the first paycheck is the low point the home screen shows.
        let before_payday = t
            .entries
            .iter()
            .take_while(|e| e.kind != TimelineKind::Paycheck)
            .last()
            .unwrap();
        assert_eq!(before_payday.date, d("2026-10-15"));
        assert_eq!(before_payday.balance_after_cents, lp.low_point_cents);
        assert_eq!(t.low_cents, lp.low_point_cents);
        assert_eq!(t.low_date, d("2026-10-15"));
        // Overdue items land today, flagged, keeping their original date.
        let overdue: Vec<&TimelineEntry> = t.entries.iter().filter(|e| e.overdue).collect();
        assert_eq!(overdue.len(), 2);
        assert!(overdue.iter().all(|e| e.date == today));
        assert_eq!(overdue[0].label, "Missed");
        assert_eq!(overdue[0].due_date, Some(d("2026-10-04")));
        assert_eq!(t.overdue_count, 2);
        // The overdue bill recurs on its own cadence after that: Nov 4.
        assert!(
            t.entries
                .iter()
                .any(|e| e.label == "Missed" && e.date == d("2026-11-04") && !e.overdue)
        );
    }

    #[test]
    fn everyday_uses_each_months_plan_and_a_payday_today_does_not_count() {
        let today = d("2026-10-30"); // a payday
        let mut groceries = everyday("Groceries", 620, 0); // Oct: 20/day
        groceries.phases.push(phase("2026-11", 900)); // Nov: 30/day
        let cats = vec![income("Paycheck", Some(1000), None), groceries];
        let t = cash_timeline(
            Cents::dollars(500),
            today,
            d("2026-11-03"),
            &biweekly(),
            &cats,
            &[],
            &[],
        );
        assert!(t.paydays.is_empty());
        assert_eq!(t.income_cents, Cents::ZERO);
        // Oct 30, 31 at $20 + Nov 1, 2, 3 at $30 = $130, as one row at the end.
        assert_eq!(t.entries.len(), 1);
        assert_eq!(t.entries[0].days, Some(5));
        assert_eq!(t.everyday_cents, Cents::dollars(130));
        assert_eq!(t.end_cents, Cents::dollars(370));
        assert_eq!(t.low_cents, Cents::dollars(370));
        assert_eq!(t.low_date, d("2026-11-03"));
    }

    #[test]
    fn empty_household_is_flat() {
        let t = cash_timeline(
            Cents::dollars(100),
            d("2026-10-11"),
            d("2026-10-01"), // before today: clamped
            &PaySchedule::default(),
            &[],
            &[],
            &[],
        );
        assert_eq!(t.until, d("2026-10-11"));
        assert!(t.entries.is_empty());
        assert_eq!(t.end_cents, Cents::dollars(100));
        assert_eq!(t.low_cents, Cents::dollars(100));
        assert_eq!(t.low_date, d("2026-10-11"));
    }
}
