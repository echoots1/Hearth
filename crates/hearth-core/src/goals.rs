//! Savings goal projections.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::calendar::Month;
use crate::money::Cents;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalInput {
    pub id: Uuid,
    pub name: String,
    pub target_cents: Cents,
    pub saved_cents: Cents,
    pub target_date: Option<NaiveDate>,
    /// Monthly contribution (the plan of the goal's category), if any.
    pub monthly_cents: Cents,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalProjection {
    pub goal_id: Uuid,
    pub name: String,
    pub target_cents: Cents,
    pub saved_cents: Cents,
    pub remaining_cents: Cents,
    pub monthly_cents: Cents,
    /// Percent saved in basis points (10_000 = done).
    pub progress_bps: i64,
    /// Months until reached at the current contribution; `None` if nothing is being contributed.
    pub months_to_reach: Option<u32>,
    /// Month the goal is reached at the current pace.
    pub reach_month: Option<Month>,
    pub target_date: Option<NaiveDate>,
    /// Contribution per month needed to reach the target by `target_date`.
    pub needed_monthly_cents: Option<Cents>,
    /// At the current pace, will the target date be met?
    pub on_track: Option<bool>,
}

pub fn project(g: &GoalInput, today: NaiveDate) -> GoalProjection {
    let remaining = (g.target_cents - g.saved_cents).floor_zero();
    let progress_bps = if g.target_cents.0 > 0 {
        g.saved_cents
            .mul_div(10_000, g.target_cents.0)
            .0
            .clamp(0, 10_000)
    } else {
        10_000
    };
    let this_month = Month::of(today);
    let months_to_reach = if remaining.0 == 0 {
        Some(0)
    } else if g.monthly_cents.0 > 0 {
        Some(remaining.div_ceil(g.monthly_cents.0).0 as u32)
    } else {
        None
    };
    let reach_month = months_to_reach.map(|m| this_month.add_months(m as i32));
    let needed_monthly = g.target_date.map(|d| {
        let months = this_month.months_until(Month::of(d)).max(1) as i64;
        remaining.div_ceil(months)
    });
    let on_track = match (g.target_date, reach_month) {
        (Some(d), Some(r)) => Some(r <= Month::of(d)),
        (Some(_), None) => Some(remaining.0 == 0),
        _ => None,
    };
    GoalProjection {
        goal_id: g.id,
        name: g.name.clone(),
        target_cents: g.target_cents,
        saved_cents: g.saved_cents,
        remaining_cents: remaining,
        monthly_cents: g.monthly_cents,
        progress_bps,
        months_to_reach,
        reach_month,
        target_date: g.target_date,
        needed_monthly_cents: needed_monthly,
        on_track,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn goal(target: i64, saved: i64, monthly: i64, date: Option<&str>) -> GoalInput {
        GoalInput {
            id: Uuid::new_v4(),
            name: "Emergency fund".into(),
            target_cents: Cents::dollars(target),
            saved_cents: Cents::dollars(saved),
            target_date: date.map(d),
            monthly_cents: Cents::dollars(monthly),
        }
    }

    #[test]
    fn pace_and_reach() {
        let p = project(&goal(3000, 1000, 250, None), d("2026-10-11"));
        assert_eq!(p.remaining_cents, Cents::dollars(2000));
        assert_eq!(p.progress_bps, 3333);
        assert_eq!(p.months_to_reach, Some(8));
        assert_eq!(p.reach_month.unwrap().to_string(), "2027-06");
        assert_eq!(p.on_track, None);
    }

    #[test]
    fn target_date_needed_and_on_track() {
        // 2000 remaining, target in 5 months → need 400/mo; at 250/mo (8 months) not on track.
        let p = project(&goal(3000, 1000, 250, Some("2027-03-01")), d("2026-10-11"));
        assert_eq!(p.needed_monthly_cents, Some(Cents::dollars(400)));
        assert_eq!(p.on_track, Some(false));
        let p = project(&goal(3000, 1000, 500, Some("2027-03-01")), d("2026-10-11"));
        assert_eq!(p.on_track, Some(true));
    }

    #[test]
    fn done_and_no_contribution() {
        let p = project(&goal(1000, 1200, 0, Some("2027-01-01")), d("2026-10-11"));
        assert_eq!(p.remaining_cents, Cents::ZERO);
        assert_eq!(p.months_to_reach, Some(0));
        assert_eq!(p.progress_bps, 10_000);
        assert_eq!(p.on_track, Some(true));
        let p = project(&goal(1000, 100, 0, None), d("2026-10-11"));
        assert_eq!(p.months_to_reach, None);
        assert_eq!(p.reach_month, None);
    }
}
