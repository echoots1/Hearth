//! Debt math: interest per day, payoff projection, and what-if deltas.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::calendar::Month;
use crate::money::Cents;
use crate::settings::{DebtStrategy, DebtStrategyKind};

/// Simulation horizon; beyond this a debt is reported as never paid off.
pub const MAX_MONTHS: u32 = 600;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtInput {
    pub id: Uuid,
    pub name: String,
    pub balance_cents: Cents,
    pub apr_bps: i64,
    pub min_payment_cents: Cents,
    /// Payment the plan actually makes each month; falls back to the minimum.
    pub planned_payment_cents: Option<Cents>,
    pub due_day: u32,
    pub custom_order: Option<i32>,
}

impl DebtInput {
    pub fn payment(&self) -> Cents {
        self.planned_payment_cents
            .unwrap_or(self.min_payment_cents)
            .max(self.min_payment_cents)
    }
}

/// Simple daily interest: `balance × apr / 365`, rounded half away from zero.
pub fn interest_per_day(balance: Cents, apr_bps: i64) -> Cents {
    balance.mul_div(apr_bps, 10_000 * 365)
}

/// One month of interest at `apr/12`.
pub fn monthly_interest(balance: Cents, apr_bps: i64) -> Cents {
    balance.mul_div(apr_bps, 10_000 * 12)
}

/// Order debts per strategy. Ties break by name for determinism.
pub fn strategy_order<'a>(debts: &'a [DebtInput], strategy: &DebtStrategy) -> Vec<&'a DebtInput> {
    let mut v: Vec<&DebtInput> = debts.iter().collect();
    match strategy.kind {
        DebtStrategyKind::Avalanche => {
            v.sort_by(|a, b| b.apr_bps.cmp(&a.apr_bps).then_with(|| a.name.cmp(&b.name)))
        }
        DebtStrategyKind::Snowball => v.sort_by(|a, b| {
            a.balance_cents
                .cmp(&b.balance_cents)
                .then_with(|| a.name.cmp(&b.name))
        }),
        DebtStrategyKind::Custom => {
            let pos = |d: &DebtInput| {
                strategy
                    .custom_order
                    .iter()
                    .position(|id| *id == d.id)
                    .map(|p| p as i64)
                    .or(d.custom_order.map(|o| o as i64))
                    .unwrap_or(i64::MAX)
            };
            v.sort_by(|a, b| pos(a).cmp(&pos(b)).then_with(|| a.name.cmp(&b.name)));
        }
    }
    v
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebtProjection {
    pub debt_id: Uuid,
    pub name: String,
    pub starting_balance_cents: Cents,
    pub apr_bps: i64,
    pub interest_per_day_cents: Cents,
    pub monthly_payment_cents: Cents,
    /// Month the balance reaches zero; `None` = not within the horizon.
    pub payoff_month: Option<Month>,
    pub months_to_payoff: Option<u32>,
    pub total_interest_cents: Cents,
    pub total_paid_cents: Cents,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projection {
    pub start_month: Month,
    pub strategy: DebtStrategyKind,
    pub extra_payment_cents: Cents,
    /// Month the last debt is paid off.
    pub debt_free_month: Option<Month>,
    pub total_interest_cents: Cents,
    pub total_monthly_payment_cents: Cents,
    pub debts: Vec<DebtProjection>,
}

/// Month-by-month simulation.
///
/// Each month, every open debt accrues `apr/12` interest, then receives its
/// planned payment. The strategy's first open debt also receives
/// `extra_payment` plus the payments freed up by debts already paid off
/// (the snowball/avalanche rollover). Payments never exceed the balance.
pub fn project(debts: &[DebtInput], strategy: &DebtStrategy, start_month: Month) -> Projection {
    let order = strategy_order(debts, strategy);
    let n = order.len();
    let mut balances: Vec<Cents> = order.iter().map(|d| d.balance_cents).collect();
    let mut interest: Vec<Cents> = vec![Cents::ZERO; n];
    let mut paid: Vec<Cents> = vec![Cents::ZERO; n];
    let mut payoff: Vec<Option<u32>> = vec![None; n];
    for (i, d) in order.iter().enumerate() {
        if d.balance_cents.0 <= 0 {
            payoff[i] = Some(0);
        }
    }

    let mut month_idx: u32 = 0;
    while payoff.iter().any(|p| p.is_none()) && month_idx < MAX_MONTHS {
        month_idx += 1;
        // Interest first.
        for (i, d) in order.iter().enumerate() {
            if payoff[i].is_none() {
                let int = monthly_interest(balances[i], d.apr_bps);
                balances[i] += int;
                interest[i] += int;
            }
        }
        // Freed-up payments from paid-off debts roll into the pool.
        let mut pool = strategy.extra_payment_cents;
        for (i, d) in order.iter().enumerate() {
            if payoff[i].is_some() {
                pool += d.payment();
            }
        }
        // Regular payments.
        for (i, d) in order.iter().enumerate() {
            if payoff[i].is_none() {
                let pay = d.payment().min(balances[i]);
                balances[i] -= pay;
                paid[i] += pay;
                if balances[i].0 <= 0 {
                    payoff[i] = Some(month_idx);
                    pool += d.payment() - pay; // leftover of this month's payment rolls on
                }
            }
        }
        // Pool goes to the first open debt in order; overflow cascades.
        for i in 0..n {
            if pool.0 <= 0 {
                break;
            }
            if payoff[i].is_none() {
                let pay = pool.min(balances[i]);
                balances[i] -= pay;
                paid[i] += pay;
                pool -= pay;
                if balances[i].0 <= 0 {
                    payoff[i] = Some(month_idx);
                }
            }
        }
        // Guard: a debt whose payment doesn't cover interest never ends.
        if order.iter().enumerate().all(|(i, d)| {
            payoff[i].is_some()
                || d.payment() + strategy.extra_payment_cents
                    <= monthly_interest(balances[i], d.apr_bps)
        }) && payoff.iter().any(|p| p.is_none())
        {
            break;
        }
    }

    let debts_out: Vec<DebtProjection> = order
        .iter()
        .enumerate()
        .map(|(i, d)| DebtProjection {
            debt_id: d.id,
            name: d.name.clone(),
            starting_balance_cents: d.balance_cents,
            apr_bps: d.apr_bps,
            interest_per_day_cents: interest_per_day(d.balance_cents, d.apr_bps),
            monthly_payment_cents: d.payment(),
            // Payment k is made in start_month + (k − 1); k = 0 means already paid.
            payoff_month: payoff[i].map(|k| start_month.add_months((k as i32 - 1).max(0))),
            months_to_payoff: payoff[i],
            total_interest_cents: interest[i],
            total_paid_cents: paid[i],
        })
        .collect();

    let debt_free_month = if debts_out.iter().all(|d| d.payoff_month.is_some()) {
        debts_out.iter().filter_map(|d| d.payoff_month).max()
    } else {
        None
    };

    Projection {
        start_month,
        strategy: strategy.kind,
        extra_payment_cents: strategy.extra_payment_cents,
        debt_free_month,
        total_interest_cents: debts_out.iter().map(|d| d.total_interest_cents).sum(),
        total_monthly_payment_cents: debts_out.iter().map(|d| d.monthly_payment_cents).sum(),
        debts: debts_out,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelayCost {
    pub debt_id: Uuid,
    pub amount_cents: Cents,
    /// Interest the charge itself accrues per day at the debt's APR.
    pub interest_per_day_cents: Cents,
    /// Change in total interest over the whole payoff.
    pub extra_interest_cents: Cents,
    /// Change in payoff timing for the debt, in days (months × days-in-those-months).
    pub payoff_delay_days: i64,
    pub baseline_payoff_month: Option<Month>,
    pub new_payoff_month: Option<Month>,
}

/// "If we put `amount` on this debt today, what does it cost?"
///
/// Re-runs the projection with the amount added to the debt's balance and
/// reports the deltas. `None` payoff on either side yields a delay of 0 and
/// the raw interest delta.
pub fn delay_cost(
    debts: &[DebtInput],
    strategy: &DebtStrategy,
    start_month: Month,
    debt_id: Uuid,
    amount: Cents,
) -> Option<DelayCost> {
    let target = debts.iter().find(|d| d.id == debt_id)?;
    let baseline = project(debts, strategy, start_month);
    let mut altered: Vec<DebtInput> = debts.to_vec();
    for d in &mut altered {
        if d.id == debt_id {
            d.balance_cents += amount;
        }
    }
    let after = project(&altered, strategy, start_month);
    let b = baseline.debts.iter().find(|d| d.debt_id == debt_id)?;
    let a = after.debts.iter().find(|d| d.debt_id == debt_id)?;
    let delay_days = match (b.payoff_month, a.payoff_month) {
        (Some(bm), Some(am)) => {
            let bd: NaiveDate = bm.date_with_day_clamped(target.due_day);
            let ad: NaiveDate = am.date_with_day_clamped(target.due_day);
            (ad - bd).num_days()
        }
        _ => 0,
    };
    Some(DelayCost {
        debt_id,
        amount_cents: amount,
        interest_per_day_cents: interest_per_day(amount, target.apr_bps),
        extra_interest_cents: after.total_interest_cents - baseline.total_interest_cents,
        payoff_delay_days: delay_days,
        baseline_payoff_month: b.payoff_month,
        new_payoff_month: a.payoff_month,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> Month {
        s.parse().unwrap()
    }

    fn debt(name: &str, balance: i64, apr_bps: i64, min: i64, planned: Option<i64>) -> DebtInput {
        DebtInput {
            id: Uuid::new_v4(),
            name: name.into(),
            balance_cents: Cents::dollars(balance),
            apr_bps,
            min_payment_cents: Cents::dollars(min),
            planned_payment_cents: planned.map(Cents::dollars),
            due_day: 15,
            custom_order: None,
        }
    }

    #[test]
    fn interest_math() {
        // $1,000 at 24.99% → 249.90/yr → 0.6847/day → 68¢
        assert_eq!(interest_per_day(Cents::dollars(1000), 2499), Cents(68));
        assert_eq!(monthly_interest(Cents::dollars(1000), 2499), Cents(2083));
        assert_eq!(interest_per_day(Cents::dollars(1000), 0), Cents::ZERO);
    }

    #[test]
    fn zero_apr_payoff_is_simple_division() {
        let d = debt("Phone", 1200, 0, 100, None);
        let p = project(&[d], &DebtStrategy::default(), m("2026-01"));
        assert_eq!(p.debts[0].months_to_payoff, Some(12));
        assert_eq!(p.debts[0].payoff_month, Some(m("2026-12")));
        assert_eq!(p.debts[0].total_interest_cents, Cents::ZERO);
        assert_eq!(p.debts[0].total_paid_cents, Cents::dollars(1200));
        assert_eq!(p.debt_free_month, Some(m("2026-12")));
    }

    #[test]
    fn avalanche_orders_by_apr_and_rolls_payments() {
        let low = debt("Car", 5000, 500, 200, None);
        let high = debt("Card", 2000, 2500, 50, None);
        let strategy = DebtStrategy {
            extra_payment_cents: Cents::dollars(100),
            ..Default::default()
        };
        let p = project(&[low.clone(), high.clone()], &strategy, m("2026-01"));
        assert_eq!(p.debts[0].name, "Card");
        let card = &p.debts[0];
        let car = &p.debts[1];
        assert!(card.payoff_month.unwrap() < car.payoff_month.unwrap());
        // With rollover, the car is paid faster than on its own.
        let alone = project(
            std::slice::from_ref(&low),
            &DebtStrategy::default(),
            m("2026-01"),
        );
        assert!(car.months_to_payoff.unwrap() < alone.debts[0].months_to_payoff.unwrap());
        assert!(p.total_interest_cents.0 > 0);
        assert_eq!(p.total_monthly_payment_cents, Cents::dollars(250));
    }

    #[test]
    fn snowball_orders_by_balance() {
        let a = debt("Big", 5000, 2500, 100, None);
        let b = debt("Small", 500, 500, 25, None);
        let s = DebtStrategy {
            kind: DebtStrategyKind::Snowball,
            ..Default::default()
        };
        let p = project(&[a, b], &s, m("2026-01"));
        assert_eq!(p.debts[0].name, "Small");
    }

    #[test]
    fn custom_order_uses_setting_then_field() {
        let mut a = debt("A", 100, 100, 10, None);
        let mut b = debt("B", 100, 100, 10, None);
        a.custom_order = Some(2);
        b.custom_order = Some(1);
        let s = DebtStrategy {
            kind: DebtStrategyKind::Custom,
            ..Default::default()
        };
        assert_eq!(strategy_order(&[a.clone(), b.clone()], &s)[0].name, "B");
        let s2 = DebtStrategy {
            kind: DebtStrategyKind::Custom,
            custom_order: vec![a.id, b.id],
            ..Default::default()
        };
        assert_eq!(strategy_order(&[a, b], &s2)[0].name, "A");
    }

    #[test]
    fn payment_below_interest_never_pays_off() {
        let d = debt("Trap", 10_000, 3000, 10, None); // $250/mo interest vs $10 payment
        let p = project(&[d], &DebtStrategy::default(), m("2026-01"));
        assert_eq!(p.debts[0].payoff_month, None);
        assert_eq!(p.debt_free_month, None);
    }

    #[test]
    fn delay_cost_reports_positive_deltas() {
        let d = debt("Card", 3000, 2499, 75, Some(300));
        let id = d.id;
        let dc = delay_cost(
            &[d],
            &DebtStrategy::default(),
            m("2026-01"),
            id,
            Cents::dollars(600),
        )
        .unwrap();
        assert_eq!(dc.interest_per_day_cents, Cents(41));
        assert!(dc.extra_interest_cents.0 > 0);
        assert!(dc.payoff_delay_days > 0);
        assert!(dc.new_payoff_month.unwrap() > dc.baseline_payoff_month.unwrap());
        assert!(delay_cost(&[], &DebtStrategy::default(), m("2026-01"), id, Cents(1)).is_none());
    }
}
