//! Matching incoming transactions to planned items.

use chrono::NaiveDate;
use uuid::Uuid;

use crate::money::Cents;
use crate::settings::PlannedMatch;

#[derive(Clone, Debug)]
pub struct PlannedCandidate {
    pub id: Uuid,
    pub date: NaiveDate,
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
    /// `status == planned && matched_transaction_id.is_none()`
    pub outstanding: bool,
}

#[derive(Clone, Debug)]
pub struct TxnForPlanned {
    pub date: NaiveDate,
    /// Signed; only outflows match planned spend.
    pub amount_cents: Cents,
    pub category_id: Option<Uuid>,
}

/// Amount tolerance: `max(pct% of planned amount, min_cents)`.
pub fn amount_tolerance(planned: Cents, cfg: &PlannedMatch) -> Cents {
    planned.mul_div(cfg.pct, 100).max(cfg.min_cents)
}

/// Pick the best outstanding planned item for a transaction, or `None`.
///
/// A candidate qualifies when: it is outstanding; the transaction is an
/// outflow; the category matches (or either side has none); the dates are
/// within `cfg.days`; and the amounts are within tolerance. Among qualifiers,
/// the smallest date distance wins, then the smallest amount distance.
pub fn find_match<'a>(
    txn: &TxnForPlanned,
    candidates: &'a [PlannedCandidate],
    cfg: &PlannedMatch,
) -> Option<&'a PlannedCandidate> {
    if txn.amount_cents.0 >= 0 {
        return None;
    }
    let spent = txn.amount_cents.abs();
    candidates
        .iter()
        .filter(|c| c.outstanding)
        .filter(|c| match (c.category_id, txn.category_id) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        })
        .filter_map(|c| {
            let dd = (txn.date - c.date).num_days().abs();
            if dd > cfg.days {
                return None;
            }
            let ad = (spent - c.amount_cents).abs();
            if ad > amount_tolerance(c.amount_cents, cfg) {
                return None;
            }
            Some((dd, ad.0, c))
        })
        .min_by_key(|(dd, ad, _)| (*dd, *ad))
        .map(|(_, _, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn cand(date: &str, dollars: i64, cat: Option<Uuid>) -> PlannedCandidate {
        PlannedCandidate {
            id: Uuid::new_v4(),
            date: d(date),
            amount_cents: Cents::dollars(dollars),
            category_id: cat,
            outstanding: true,
        }
    }

    #[test]
    fn tolerance() {
        let cfg = PlannedMatch::default(); // 10%, $5 floor
        assert_eq!(
            amount_tolerance(Cents::dollars(400), &cfg),
            Cents::dollars(40)
        );
        assert_eq!(
            amount_tolerance(Cents::dollars(20), &cfg),
            Cents::dollars(5)
        );
    }

    #[test]
    fn matching_rules() {
        let cfg = PlannedMatch::default();
        let cat = Uuid::new_v4();
        let other = Uuid::new_v4();
        let c_tires = cand("2026-03-18", 400, Some(cat));
        let c_far = cand("2026-03-01", 400, Some(cat));
        let c_other_cat = cand("2026-03-18", 380, Some(other));
        let c_any_cat = cand("2026-03-20", 395, None);
        let mut c_done = cand("2026-03-18", 400, Some(cat));
        c_done.outstanding = false;
        let cands = vec![
            c_far.clone(),
            c_other_cat.clone(),
            c_any_cat.clone(),
            c_done,
            c_tires.clone(),
        ];

        let t = TxnForPlanned {
            date: d("2026-03-19"),
            amount_cents: Cents::dollars(-410),
            category_id: Some(cat),
        };
        assert_eq!(find_match(&t, &cands, &cfg).unwrap().id, c_tires.id); // closest date among qualifiers

        let t_uncat = TxnForPlanned {
            category_id: None,
            ..t.clone()
        };
        // Uncategorized txn can match any category; tires, any_cat and other_cat are all 1 day away;
        // tires wins on amount distance (10 vs 15 vs 30).
        assert_eq!(find_match(&t_uncat, &cands, &cfg).unwrap().id, c_tires.id);

        let t_big = TxnForPlanned {
            amount_cents: Cents::dollars(-500),
            ..t.clone()
        };
        assert!(find_match(&t_big, &cands, &cfg).is_none()); // outside 10%

        let t_in = TxnForPlanned {
            amount_cents: Cents::dollars(400),
            ..t.clone()
        };
        assert!(find_match(&t_in, &cands, &cfg).is_none()); // inflow never matches

        let t_late = TxnForPlanned {
            date: d("2026-03-30"),
            ..t.clone()
        };
        assert!(find_match(&t_late, &cands, &cfg).is_none()); // > 5 days from everything
    }
}
