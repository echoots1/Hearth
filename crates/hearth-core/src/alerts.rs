//! Deterministic alert evaluation and notification timing.
//!
//! `evaluate` takes a snapshot of household state and returns the alerts that
//! *should* exist right now. The caller (the `evaluate` job) upserts them by
//! `dedupe_key`. No I/O, no judgment: every rule is in `docs/ARCHITECTURE.md` §7.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::balance::LowPoint;
use crate::bills::{Bill, BillStatus};
use crate::budget::{CategoryView, Sign};
use crate::calendar::Month;
use crate::money::Cents;
use crate::rules::NoteDirectives;
use crate::settings::{ClockTime, NagSettings, NotificationSettings, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertType {
    UnplannedLargeCharge,
    CanceledBillCharged,
    BillAmountChanged,
    BillNotSeen,
    CategoryBlown,
    CategoryPace,
    FloorBreached,
    DebtRedraw,
    CardDeclined,
    PlannedItemMissing,
    BalanceStale,
    SyncFailed,
    ReviewQueueBacklog,
    Milestone,
    /// API mode stopped calling Claude because the monthly cap was reached.
    ApiSpendCap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warn,
    Critical,
}

/// An alert that should exist. Identity is `dedupe_key`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertCandidate {
    pub alert_type: AlertType,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub amount_cents: Option<Cents>,
    pub dedupe_key: String,
    pub transaction_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub bill_id: Option<Uuid>,
    pub debt_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub planned_item_id: Option<Uuid>,
    pub goal_id: Option<Uuid>,
    pub context: serde_json::Value,
}

impl AlertCandidate {
    fn new(
        alert_type: AlertType,
        severity: Severity,
        dedupe_key: String,
        title: String,
        detail: String,
    ) -> Self {
        AlertCandidate {
            alert_type,
            severity,
            title,
            detail,
            amount_cents: None,
            dedupe_key,
            transaction_id: None,
            category_id: None,
            bill_id: None,
            debt_id: None,
            account_id: None,
            planned_item_id: None,
            goal_id: None,
            context: serde_json::Value::Null,
        }
    }
}

/// A transaction as the evaluator sees it (current month, not deleted, not shadow).
#[derive(Clone, Debug)]
pub struct TxnSnapshot {
    pub id: Uuid,
    pub date: NaiveDate,
    pub description: String,
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
    pub planned_item_id: Option<Uuid>,
    /// The recurring bill this row was matched to, if any.
    pub bill_id: Option<Uuid>,
    /// `abs(amount) − bill.amount` when matched (positive = charged more).
    pub bill_drift_cents: Option<Cents>,
    pub pending: bool,
}

#[derive(Clone, Debug)]
pub struct PlannedSnapshot {
    pub id: Uuid,
    pub item: String,
    pub date: NaiveDate,
    pub amount_cents: Cents,
    pub outstanding: bool,
}

#[derive(Clone, Debug)]
pub struct DebtSnapshot {
    pub id: Uuid,
    pub name: String,
    pub balance_cents: Cents,
    pub balance_as_of: DateTime<Utc>,
    /// Balance at the previous sync, if known.
    pub previous_balance_cents: Option<Cents>,
    pub min_payment_cents: Cents,
    /// Projected payoff month now and at the last evaluation.
    pub payoff_month: Option<Month>,
    pub previous_payoff_month: Option<Month>,
}

#[derive(Clone, Debug)]
pub struct GoalSnapshot {
    pub id: Uuid,
    pub name: String,
    pub target_cents: Cents,
    pub saved_cents: Cents,
    pub previously_reached: bool,
}

#[derive(Clone, Debug)]
pub struct AccountSnapshot {
    pub id: Uuid,
    pub name: String,
    pub include_in_cash: bool,
    pub reported_balance_as_of: Option<DateTime<Utc>>,
    /// When a sync last refreshed the bank balance; keeps a synced account fresh.
    pub bank_balance_as_of: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct DeclineSnapshot {
    pub external_id: String,
    pub account_id: Uuid,
    pub account_name: String,
    pub occurred_at: DateTime<Utc>,
    pub amount_cents: Cents,
    pub descriptor: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SourceHealth {
    pub source: String,
    pub consecutive_failures: i32,
    pub last_error: Option<String>,
}

/// Everything `evaluate` looks at.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub now: DateTime<Utc>,
    pub today: NaiveDate,
    pub month: Month,
    pub settings: Settings,
    pub categories: Vec<CategoryView>,
    pub bills: Vec<Bill>,
    pub transactions: Vec<TxnSnapshot>,
    pub planned: Vec<PlannedSnapshot>,
    pub debts: Vec<DebtSnapshot>,
    pub goals: Vec<GoalSnapshot>,
    pub accounts: Vec<AccountSnapshot>,
    pub declines: Vec<DeclineSnapshot>,
    pub sources: Vec<SourceHealth>,
    pub low_point: Option<LowPoint>,
    pub review_pending_count: i64,
    pub review_pending_oldest: Option<DateTime<Utc>>,
}

fn cat(s: &Snapshot, id: Option<Uuid>) -> Option<&CategoryView> {
    id.and_then(|id| s.categories.iter().find(|c| c.id == id))
}

fn bill(s: &Snapshot, id: Option<Uuid>) -> Option<&Bill> {
    id.and_then(|id| s.bills.iter().find(|b| b.id == id))
}

pub fn evaluate(s: &Snapshot) -> Vec<AlertCandidate> {
    let mut out = Vec::new();
    let st = &s.settings;

    // ── per-transaction ──
    for t in &s.transactions {
        let directives = cat(s, t.category_id)
            .map(|c| NoteDirectives::parse(&c.note))
            .unwrap_or_default();
        let is_spend =
            t.amount_cents.0 < 0 && cat(s, t.category_id).is_none_or(|c| c.sign == Sign::Spend);

        if let Some(b) = bill(s, t.bill_id) {
            if b.status == BillStatus::Canceled {
                let mut a = AlertCandidate::new(
                    AlertType::CanceledBillCharged,
                    Severity::Critical,
                    format!("txn:{}", t.id),
                    format!("Canceled bill charged: {}", b.name),
                    format!(
                        "{} charged {} on {} but the bill is marked canceled.",
                        b.name,
                        t.amount_cents.abs(),
                        t.date
                    ),
                );
                a.amount_cents = Some(t.amount_cents.abs());
                a.transaction_id = Some(t.id);
                a.bill_id = Some(b.id);
                a.category_id = t.category_id;
                out.push(a);
                continue;
            }
            if let Some(drift) = t.bill_drift_cents {
                let tol = b
                    .amount_cents
                    .mul_div(st.bill_match.pct, 100)
                    .max(st.bill_match.min_cents);
                if drift > tol {
                    let mut a = AlertCandidate::new(
                        AlertType::BillAmountChanged,
                        Severity::Warn,
                        format!("txn:{}", t.id),
                        format!("{} charged {} more than usual", b.name, drift),
                        format!(
                            "{} posted {} on {}; the bill is {}. Update the bill if this is the new price.",
                            b.name,
                            t.amount_cents.abs(),
                            t.date,
                            b.amount_cents
                        ),
                    );
                    a.amount_cents = Some(drift);
                    a.transaction_id = Some(t.id);
                    a.bill_id = Some(b.id);
                    a.category_id = t.category_id;
                    a.context = serde_json::json!({ "bill_amount_cents": b.amount_cents, "posted_cents": t.amount_cents.abs() });
                    out.push(a);
                }
            }
            continue; // a bill match is never an unplanned charge
        }

        if is_spend
            && t.amount_cents.abs() >= st.large_charge_threshold_cents
            && t.planned_item_id.is_none()
            && !directives.suppress_large_charge()
        {
            let cname = cat(s, t.category_id)
                .map(|c| c.name.as_str())
                .unwrap_or("uncategorized");
            let mut a = AlertCandidate::new(
                AlertType::UnplannedLargeCharge,
                Severity::Warn,
                format!("txn:{}", t.id),
                format!(
                    "Unplanned charge: {} {}",
                    t.amount_cents.abs(),
                    t.description
                ),
                format!(
                    "{} for {} on {} ({}) has no planned item or bill. Threshold is {}.",
                    t.amount_cents.abs(),
                    t.description,
                    t.date,
                    cname,
                    st.large_charge_threshold_cents
                ),
            );
            a.amount_cents = Some(t.amount_cents.abs());
            a.transaction_id = Some(t.id);
            a.category_id = t.category_id;
            a.context = serde_json::json!({ "pending": t.pending });
            out.push(a);
        }
    }

    // ── bills not seen ──
    for b in crate::bills::overdue(&s.bills, s.today, &st.bill_match) {
        let late = (s.today - b.next_due).num_days();
        let mut a = AlertCandidate::new(
            AlertType::BillNotSeen,
            Severity::Info,
            format!("bill:{}:{}", b.id, b.next_due),
            format!("{} hasn't posted", b.name),
            format!(
                "{} ({}) was due {} and no matching charge has appeared ({} days).",
                b.name, b.amount_cents, b.next_due, late
            ),
        );
        a.amount_cents = Some(b.amount_cents);
        a.bill_id = Some(b.id);
        a.category_id = b.category_id;
        out.push(a);
    }

    // ── per-category ──
    let days_in_month = s.month.days_in_month() as i64;
    let day = s.today.day();
    for c in s.categories.iter().filter(|c| c.sign == Sign::Spend) {
        let d = NoteDirectives::parse(&c.note);
        if d.no_alerts {
            continue;
        }
        if c.plan_cents.0 > 0 && c.activity_cents > c.plan_cents {
            let over = c.activity_cents - c.plan_cents;
            let mut a = AlertCandidate::new(
                AlertType::CategoryBlown,
                Severity::Warn,
                format!("cat:{}:{}", c.id, s.month),
                format!("{} is over by {}", c.name, over),
                format!(
                    "{}: spent {} of {} planned for {} ({} over).",
                    c.name, c.activity_cents, c.plan_cents, s.month, over
                ),
            );
            a.amount_cents = Some(over);
            a.category_id = Some(c.id);
            a.context =
                serde_json::json!({ "plan_cents": c.plan_cents, "spent_cents": c.activity_cents });
            out.push(a);
        } else if c.is_everyday
            && c.plan_cents.0 > 0
            && day >= st.pace_alert.from_day
            && !d.suppress_pace()
        {
            let projected = c.activity_cents.mul_div(days_in_month, day as i64);
            let threshold = c.plan_cents.mul_div(st.pace_alert.ratio_pct, 100);
            if projected > threshold {
                let mut a = AlertCandidate::new(
                    AlertType::CategoryPace,
                    Severity::Info,
                    format!("pace:{}:{}", c.id, s.month),
                    format!("{} is on pace for {}", c.name, projected),
                    format!(
                        "{}: {} spent by day {} projects to {} vs {} planned.",
                        c.name, c.activity_cents, day, projected, c.plan_cents
                    ),
                );
                a.amount_cents = Some(projected);
                a.category_id = Some(c.id);
                a.context =
                    serde_json::json!({ "projected_cents": projected, "plan_cents": c.plan_cents });
                out.push(a);
            }
        }
    }

    // ── floor ──
    if let Some(lp) = &s.low_point
        && lp.low_point_cents < st.cash_floor_cents
    {
        let short = st.cash_floor_cents - lp.low_point_cents;
        let mut a = AlertCandidate::new(
            AlertType::FloorBreached,
            Severity::Critical,
            format!("floor:{}", s.month),
            format!(
                "Projected low point {} is under the {} floor",
                lp.low_point_cents, st.cash_floor_cents
            ),
            format!(
                "Cash {} − bills {} − everyday {} − planned {} = {} by {} ({} short of the floor).",
                lp.estimate_cents,
                lp.bills_due_cents,
                lp.everyday_expected_cents,
                lp.planned_items_cents,
                lp.low_point_cents,
                lp.next_payday,
                short
            ),
        );
        a.amount_cents = Some(short);
        a.context = serde_json::to_value(lp).unwrap_or_default();
        out.push(a);
    }

    // ── debts: redraw and milestones ──
    for d in &s.debts {
        if let Some(prev) = d.previous_balance_cents
            && d.balance_cents - prev > d.min_payment_cents
        {
            let up = d.balance_cents - prev;
            let mut a = AlertCandidate::new(
                AlertType::DebtRedraw,
                Severity::Warn,
                format!("debt:{}:{}", d.id, d.balance_as_of.date_naive()),
                format!("{} balance went up {}", d.name, up),
                format!(
                    "{}: {} → {} (+{}) as of {}.",
                    d.name,
                    prev,
                    d.balance_cents,
                    up,
                    d.balance_as_of.date_naive()
                ),
            );
            a.amount_cents = Some(up);
            a.debt_id = Some(d.id);
            out.push(a);
        }
        if d.balance_cents.0 == 0 && d.previous_balance_cents.is_some_and(|p| p.0 > 0) {
            let mut a = AlertCandidate::new(
                AlertType::Milestone,
                Severity::Info,
                format!("milestone:debt:{}:paid", d.id),
                format!("{} is paid off", d.name),
                format!(
                    "{} reached a zero balance as of {}.",
                    d.name,
                    d.balance_as_of.date_naive()
                ),
            );
            a.debt_id = Some(d.id);
            out.push(a);
        }
        if let (Some(now), Some(prev)) = (d.payoff_month, d.previous_payoff_month)
            && prev.months_until(now) > 1
        {
            let mut a = AlertCandidate::new(
                AlertType::Milestone,
                Severity::Info,
                format!("milestone:debt:{}:slip:{}", d.id, now),
                format!("{} payoff slipped to {}", d.name, now),
                format!(
                    "{}'s projected payoff moved from {} to {} ({} months later).",
                    d.name,
                    prev,
                    now,
                    prev.months_until(now)
                ),
            );
            a.debt_id = Some(d.id);
            out.push(a);
        }
    }

    // ── goals reached ──
    for g in &s.goals {
        if g.saved_cents >= g.target_cents && g.target_cents.0 > 0 && !g.previously_reached {
            let mut a = AlertCandidate::new(
                AlertType::Milestone,
                Severity::Info,
                format!("milestone:goal:{}", g.id),
                format!("Goal reached: {}", g.name),
                format!(
                    "{} hit its {} target ({} saved).",
                    g.name, g.target_cents, g.saved_cents
                ),
            );
            a.goal_id = Some(g.id);
            a.amount_cents = Some(g.saved_cents);
            out.push(a);
        }
    }

    // ── declines ──
    for dec in &s.declines {
        let mut a = AlertCandidate::new(
            AlertType::CardDeclined,
            Severity::Warn,
            format!("decline:{}", dec.external_id),
            format!("Card declined: {} at {}", dec.amount_cents, dec.descriptor),
            format!(
                "{} declined {} at {}{}.",
                dec.account_name,
                dec.amount_cents,
                dec.descriptor,
                dec.reason
                    .as_ref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default()
            ),
        );
        a.amount_cents = Some(dec.amount_cents);
        a.account_id = Some(dec.account_id);
        out.push(a);
    }

    // ── planned items not seen ──
    for p in s.planned.iter().filter(|p| p.outstanding) {
        let overdue = (s.today - p.date).num_days();
        if overdue > st.planned_match.days {
            let mut a = AlertCandidate::new(
                AlertType::PlannedItemMissing,
                Severity::Info,
                format!("planned:{}", p.id),
                format!("Planned item not seen: {}", p.item),
                format!(
                    "{} ({}) was planned for {} and hasn't posted ({} days).",
                    p.item, p.amount_cents, p.date, overdue
                ),
            );
            a.amount_cents = Some(p.amount_cents);
            a.planned_item_id = Some(p.id);
            out.push(a);
        }
    }

    // ── stale balances (the newer of reported and bank-synced counts) ──
    for acct in s.accounts.iter().filter(|a| a.include_in_cash) {
        let age = acct
            .reported_balance_as_of
            .into_iter()
            .chain(acct.bank_balance_as_of)
            .max()
            .map(|t| (s.now - t).num_days())
            .unwrap_or(i64::MAX);
        if age >= st.stale_balance_days {
            let mut a = AlertCandidate::new(
                AlertType::BalanceStale,
                Severity::Info,
                format!("stale:{}:{}", acct.id, s.today),
                format!("{} balance needs a fresh reading", acct.name),
                if age == i64::MAX {
                    format!("{} has never had a balance, reported or synced.", acct.name)
                } else {
                    format!("{}'s balance is {} days old.", acct.name, age)
                },
            );
            a.account_id = Some(acct.id);
            out.push(a);
        }
    }

    // ── sync health ──
    for src in &s.sources {
        if src.consecutive_failures >= st.sync_failure_threshold {
            out.push(AlertCandidate::new(
                AlertType::SyncFailed,
                Severity::Warn,
                format!("sync:{}", src.source),
                format!(
                    "{} sync has failed {} times",
                    src.source, src.consecutive_failures
                ),
                src.last_error
                    .clone()
                    .unwrap_or_else(|| "No error detail.".into()),
            ));
        }
    }

    // ── review backlog ──
    if s.review_pending_count > st.review_backlog.count
        && let Some(oldest) = s.review_pending_oldest
        && (s.now - oldest).num_days() >= st.review_backlog.days
    {
        out.push(AlertCandidate::new(
            AlertType::ReviewQueueBacklog,
            Severity::Info,
            format!("review:{}", s.today),
            format!("{} transactions waiting for review", s.review_pending_count),
            format!(
                "{} uncategorized transactions; the oldest has waited {} days.",
                s.review_pending_count,
                (s.now - oldest).num_days()
            ),
        ));
    }

    out
}

/// Is `now` inside quiet hours for this household?
pub fn in_quiet_hours(
    n: &NotificationSettings,
    tz: Tz,
    now: DateTime<Utc>,
    severity: Severity,
) -> bool {
    let Some(q) = &n.quiet_hours else {
        return false;
    };
    if severity == Severity::Critical && n.quiet_hours_override_critical {
        return false;
    }
    let local = now.with_timezone(&tz);
    use chrono::Timelike;
    q.contains(ClockTime {
        hour: local.hour(),
        minute: local.minute(),
    })
}

/// Should an un-acked, already-notified alert be nudged again?
pub fn nudge_due(
    nag: &NagSettings,
    notified_at: DateTime<Utc>,
    last_nudged_at: Option<DateTime<Utc>>,
    nudge_count: i32,
    snoozed_until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    if nudge_count >= nag.max_nudges {
        return false;
    }
    if let Some(s) = snoozed_until
        && now < s
    {
        return false;
    }
    let due_at = match last_nudged_at {
        None => notified_at + chrono::Duration::minutes(nag.first_after_minutes),
        Some(last) => last + chrono::Duration::minutes(nag.every_minutes),
    };
    now >= due_at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::CategoryView;
    use chrono::TimeZone;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn catv(name: &str, plan: i64, spent: i64, note: &str) -> CategoryView {
        CategoryView {
            id: Uuid::new_v4(),
            name: name.into(),
            sign: Sign::Spend,
            note: note.into(),
            is_everyday: true,
            bills_total_cents: Cents::ZERO,
            bills_over_plan_cents: Cents::ZERO,
            plan_cents: Cents::dollars(plan),
            plan_per_paycheck: false,
            per_paycheck_cents: Cents::ZERO,
            paychecks_in_month: 2,
            pay_schedule: None,
            actual_cents: Cents::dollars(-spent),
            activity_cents: Cents::dollars(spent),
            remaining_cents: Cents::dollars(plan - spent),
            used_bps: 0,
            next_phase: None,
        }
    }

    fn base() -> Snapshot {
        Snapshot {
            now: Utc.with_ymd_and_hms(2026, 3, 15, 18, 0, 0).unwrap(),
            today: d("2026-03-15"),
            month: "2026-03".parse().unwrap(),
            settings: Settings::default(),
            categories: vec![],
            bills: vec![],
            transactions: vec![],
            planned: vec![],
            debts: vec![],
            goals: vec![],
            accounts: vec![],
            declines: vec![],
            sources: vec![],
            low_point: None,
            review_pending_count: 0,
            review_pending_oldest: None,
        }
    }

    fn txn(desc: &str, amount: i64, cat: Option<Uuid>) -> TxnSnapshot {
        TxnSnapshot {
            id: Uuid::new_v4(),
            date: d("2026-03-14"),
            description: desc.into(),
            amount_cents: Cents::dollars(amount),
            category_id: cat,
            planned_item_id: None,
            bill_id: None,
            bill_drift_cents: None,
            pending: false,
        }
    }

    fn bill(name: &str, dollars: i64, due: &str, status: BillStatus) -> Bill {
        Bill {
            id: Uuid::new_v4(),
            name: name.into(),
            amount_cents: Cents::dollars(dollars),
            cadence_months: 1,
            per_paycheck: false,
            next_due: d(due),
            category_id: None,
            account_id: None,
            match_pattern: None,
            status,
        }
    }

    fn types(v: &[AlertCandidate]) -> Vec<AlertType> {
        v.iter().map(|a| a.alert_type).collect()
    }

    #[test]
    fn empty_snapshot_raises_nothing() {
        assert!(evaluate(&base()).is_empty());
    }

    #[test]
    fn large_charge_rules() {
        let mut s = base();
        let groceries = catv("Groceries", 600, 100, "");
        let quiet = catv("Gifts", 600, 100, "[no-large-charge]");
        let gid = groceries.id;
        let qid = quiet.id;
        s.categories = vec![groceries, quiet];
        let mut planned_txn = txn("Tire Shop", -400, Some(gid));
        planned_txn.planned_item_id = Some(Uuid::new_v4());
        let rent = bill("Rent", 1500, "2026-03-14", BillStatus::Active);
        let mut rent_txn = txn("RENT PAYMENT", -1500, Some(gid));
        rent_txn.bill_id = Some(rent.id);
        rent_txn.bill_drift_cents = Some(Cents::ZERO);
        s.bills = vec![rent];
        s.transactions = vec![
            txn("Sample Grocer", -150, Some(gid)), // ≥ $100 → alert
            txn("Sample Grocer", -99, Some(gid)),  // below → no
            txn("Gift Shop", -500, Some(qid)),     // suppressed by note
            planned_txn,                           // planned → no
            rent_txn,                              // bill match → no
            txn("Paycheck", 5000, None),           // inflow → no
            txn("Mystery", -250, None),            // uncategorized → alert
        ];
        let out = evaluate(&s);
        assert_eq!(
            types(&out),
            vec![
                AlertType::UnplannedLargeCharge,
                AlertType::UnplannedLargeCharge
            ]
        );
        assert_eq!(out[0].amount_cents, Some(Cents::dollars(150)));
        assert!(out[1].detail.contains("uncategorized"));
    }

    #[test]
    fn bill_alerts() {
        let mut s = base();
        let canceled = bill("Demo Video", 16, "2026-03-09", BillStatus::Canceled);
        let power = bill("Power", 160, "2026-03-14", BillStatus::Active);
        let late = bill("Water", 60, "2026-03-05", BillStatus::Active); // 10 days overdue
        let mut t1 = txn("DEMO VIDEO", -16, None);
        t1.bill_id = Some(canceled.id);
        t1.bill_drift_cents = Some(Cents::ZERO);
        let mut t2 = txn("POWER CO", -190, None); // $30 over on $160 → > 10% tolerance
        t2.bill_id = Some(power.id);
        t2.bill_drift_cents = Some(Cents::dollars(30));
        let mut t3 = txn("POWER CO", -165, None); // within tolerance
        t3.bill_id = Some(power.id);
        t3.bill_drift_cents = Some(Cents::dollars(5));
        s.bills = vec![canceled, power, late];
        s.transactions = vec![t1, t2, t3];
        let out = evaluate(&s);
        assert_eq!(
            types(&out),
            vec![
                AlertType::CanceledBillCharged,
                AlertType::BillAmountChanged,
                AlertType::BillNotSeen
            ]
        );
        assert_eq!(out[0].severity, Severity::Critical);
        assert_eq!(out[1].amount_cents, Some(Cents::dollars(30)));
        assert_eq!(out[2].title, "Water hasn't posted");
    }

    #[test]
    fn blown_and_pace() {
        let mut s = base(); // day 15 of 31
        s.categories = vec![
            catv("Over", 100, 120, ""),           // blown
            catv("Fast", 310, 200, ""),           // 200/15*31 = 413 > 341 → pace
            catv("Fine", 310, 150, ""),           // 310 → not > 341
            catv("Muted", 310, 300, "[no-pace]"), // suppressed
            catv("Silent", 100, 500, "[no-alerts]"),
            catv("Unplanned", 0, 50, ""), // plan 0 → nothing
        ];
        let out = evaluate(&s);
        assert_eq!(
            types(&out),
            vec![AlertType::CategoryBlown, AlertType::CategoryPace]
        );
        assert_eq!(out[0].amount_cents, Some(Cents::dollars(20)));
        assert_eq!(out[1].amount_cents, Some(Cents(41_333)));
        s.today = d("2026-03-05");
        let out = evaluate(&s);
        assert_eq!(types(&out), vec![AlertType::CategoryBlown]);
    }

    #[test]
    fn floor_debt_goal_decline_planned_stale_sync_review() {
        let mut s = base();
        s.low_point = Some(LowPoint {
            today: s.today,
            next_payday: d("2026-03-31"),
            days_until_payday: 16,
            estimate_cents: Cents::dollars(1000),
            bills_due_cents: Cents::dollars(400),
            everyday_expected_cents: Cents::dollars(300),
            planned_items_cents: Cents::ZERO,
            plan_remaining_cents: Cents::ZERO,
            low_point_cents: Cents::dollars(300),
            bills: vec![],
            everyday: vec![],
            planned: vec![],
            plan_remaining: vec![],
        });
        s.debts = vec![
            DebtSnapshot {
                id: Uuid::new_v4(),
                name: "Card".into(),
                balance_cents: Cents::dollars(2300),
                balance_as_of: s.now,
                previous_balance_cents: Some(Cents::dollars(2000)),
                min_payment_cents: Cents::dollars(75),
                payoff_month: Some("2027-11".parse().unwrap()),
                previous_payoff_month: Some("2027-08".parse().unwrap()),
            },
            DebtSnapshot {
                id: Uuid::new_v4(),
                name: "Loan".into(),
                balance_cents: Cents::ZERO,
                balance_as_of: s.now,
                previous_balance_cents: Some(Cents::dollars(120)),
                min_payment_cents: Cents::dollars(50),
                payoff_month: None,
                previous_payoff_month: None,
            },
        ];
        s.goals = vec![
            GoalSnapshot {
                id: Uuid::new_v4(),
                name: "Emergency fund".into(),
                target_cents: Cents::dollars(3000),
                saved_cents: Cents::dollars(3050),
                previously_reached: false,
            },
            GoalSnapshot {
                id: Uuid::new_v4(),
                name: "Already".into(),
                target_cents: Cents::dollars(100),
                saved_cents: Cents::dollars(200),
                previously_reached: true,
            },
        ];
        s.declines = vec![DeclineSnapshot {
            external_id: "ev_1".into(),
            account_id: Uuid::new_v4(),
            account_name: "Streaming card".into(),
            occurred_at: s.now,
            amount_cents: Cents(1599),
            descriptor: "Demo Streaming".into(),
            reason: Some("INSUFFICIENT_FUNDS".into()),
        }];
        s.planned = vec![
            PlannedSnapshot {
                id: Uuid::new_v4(),
                item: "Tires".into(),
                date: d("2026-03-01"),
                amount_cents: Cents::dollars(400),
                outstanding: true,
            },
            PlannedSnapshot {
                id: Uuid::new_v4(),
                item: "Recent".into(),
                date: d("2026-03-12"),
                amount_cents: Cents::dollars(40),
                outstanding: true,
            },
        ];
        s.accounts = vec![
            AccountSnapshot {
                id: Uuid::new_v4(),
                name: "Checking".into(),
                include_in_cash: true,
                reported_balance_as_of: Some(s.now - chrono::Duration::days(8)),
                bank_balance_as_of: None,
            },
            AccountSnapshot {
                id: Uuid::new_v4(),
                name: "Fresh".into(),
                include_in_cash: true,
                reported_balance_as_of: Some(s.now - chrono::Duration::days(1)),
                bank_balance_as_of: None,
            },
            AccountSnapshot {
                id: Uuid::new_v4(),
                name: "Card".into(),
                include_in_cash: false,
                reported_balance_as_of: None,
                bank_balance_as_of: None,
            },
            // Synced yesterday: fresh even though nobody has typed a balance in weeks.
            AccountSnapshot {
                id: Uuid::new_v4(),
                name: "Synced".into(),
                include_in_cash: true,
                reported_balance_as_of: Some(s.now - chrono::Duration::days(20)),
                bank_balance_as_of: Some(s.now - chrono::Duration::days(1)),
            },
        ];
        s.sources = vec![
            SourceHealth {
                source: "plaid".into(),
                consecutive_failures: 3,
                last_error: Some("ITEM_LOGIN_REQUIRED".into()),
            },
            SourceHealth {
                source: "privacy".into(),
                consecutive_failures: 1,
                last_error: None,
            },
        ];
        s.review_pending_count = 12;
        s.review_pending_oldest = Some(s.now - chrono::Duration::days(4));
        let out = evaluate(&s);
        assert_eq!(
            types(&out),
            vec![
                AlertType::FloorBreached,
                AlertType::DebtRedraw,
                AlertType::Milestone,
                AlertType::Milestone,
                AlertType::Milestone,
                AlertType::CardDeclined,
                AlertType::PlannedItemMissing,
                AlertType::BalanceStale,
                AlertType::SyncFailed,
                AlertType::ReviewQueueBacklog,
            ]
        );
        assert_eq!(out[0].amount_cents, Some(Cents::dollars(200)));
        assert_eq!(out[1].amount_cents, Some(Cents::dollars(300)));
        assert!(out[2].title.contains("slipped"));
        assert!(out[3].title.contains("paid off"));
        assert!(out[4].title.contains("Goal reached"));
        assert!(out[5].detail.contains("INSUFFICIENT_FUNDS"));
        assert_eq!(out[6].title, "Planned item not seen: Tires");
        assert!(out[7].detail.contains("8 days"));
        assert_eq!(out[8].dedupe_key, "sync:plaid");
    }

    #[test]
    fn quiet_hours_and_critical_override() {
        let n = NotificationSettings::default();
        let tz: Tz = "America/Chicago".parse().unwrap();
        let late = Utc.with_ymd_and_hms(2026, 7, 2, 4, 0, 0).unwrap(); // 23:00 CDT
        let noon = Utc.with_ymd_and_hms(2026, 7, 2, 17, 0, 0).unwrap();
        assert!(in_quiet_hours(&n, tz, late, Severity::Warn));
        assert!(!in_quiet_hours(&n, tz, late, Severity::Critical));
        assert!(!in_quiet_hours(&n, tz, noon, Severity::Warn));
        let none = NotificationSettings {
            quiet_hours: None,
            ..n
        };
        assert!(!in_quiet_hours(&none, tz, late, Severity::Info));
    }

    #[test]
    fn nudge_schedule() {
        let nag = NagSettings::default();
        let t0 = Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap();
        let min = |m: i64| t0 + chrono::Duration::minutes(m);
        assert!(!nudge_due(&nag, t0, None, 0, None, min(59)));
        assert!(nudge_due(&nag, t0, None, 0, None, min(60)));
        assert!(!nudge_due(&nag, t0, Some(min(60)), 1, None, min(299)));
        assert!(nudge_due(&nag, t0, Some(min(60)), 1, None, min(300)));
        assert!(!nudge_due(&nag, t0, Some(min(60)), 6, None, min(10_000)));
        assert!(!nudge_due(
            &nag,
            t0,
            Some(min(60)),
            1,
            Some(min(1000)),
            min(300)
        ));
        assert!(nudge_due(
            &nag,
            t0,
            Some(min(60)),
            1,
            Some(min(1000)),
            min(1000)
        ));
    }
}
