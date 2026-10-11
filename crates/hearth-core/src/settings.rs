//! Household settings that the money math depends on.
//!
//! Stored as one JSON value per key in the `settings` table; the server loads
//! them into this struct. Every field has a default so a fresh household works
//! with no rows. Field names are the setting keys.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::calendar::Month;
use crate::money::Cents;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub pay_schedule: PaySchedule,
    pub cash_floor_cents: Cents,
    pub large_charge_threshold_cents: Cents,
    pub planned_match: PlannedMatch,
    pub pace_alert: PaceAlert,
    pub stale_balance_days: i64,
    pub review_backlog: ReviewBacklog,
    pub sync_failure_threshold: i32,
    pub debt_strategy: DebtStrategy,
    pub notifications: NotificationSettings,
    pub nag: NagSettings,
    pub assistant: AssistantSettings,
    pub bill_match: BillMatch,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            pay_schedule: PaySchedule::default(),
            cash_floor_cents: Cents::dollars(500),
            large_charge_threshold_cents: Cents::dollars(100),
            planned_match: PlannedMatch::default(),
            pace_alert: PaceAlert::default(),
            stale_balance_days: 7,
            review_backlog: ReviewBacklog::default(),
            sync_failure_threshold: 3,
            debt_strategy: DebtStrategy::default(),
            notifications: NotificationSettings::default(),
            nag: NagSettings::default(),
            assistant: AssistantSettings::default(),
            bill_match: BillMatch::default(),
        }
    }
}

/// Tolerances for matching a posted transaction to a recurring bill.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BillMatch {
    /// ± days around `next_due`.
    pub days: i64,
    /// Percent tolerance on the amount before "bill amount changed" fires.
    pub pct: i64,
    /// Absolute tolerance floor.
    pub min_cents: Cents,
    /// Days past due before "bill not seen" fires.
    pub not_seen_after_days: i64,
}

impl Default for BillMatch {
    fn default() -> Self {
        BillMatch {
            days: 4,
            pct: 10,
            min_cents: Cents::dollars(3),
            not_seen_after_days: 5,
        }
    }
}

/// Who initiates assistant work.
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantMode {
    /// Claude app + MCP only; Hearth never calls Claude.
    Pull,
    /// Hearth calls the Claude API with the instance owner's key.
    Api,
}

/// How much the assistant may do on its own in an area.
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Autonomy {
    /// Writes become proposals a person approves.
    Backseat,
    /// Writes apply immediately, logged with undo.
    Driver,
}

/// The write areas the autonomy switch distinguishes.
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyArea {
    CategorizingNotes,
    PlansBills,
    DebtsGoals,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssistantSettings {
    pub mode: AssistantMode,
    /// Global position; `None` in an override means "use global".
    pub autonomy: Autonomy,
    pub categorizing_notes: Option<Autonomy>,
    pub plans_bills: Option<Autonomy>,
    pub debts_goals: Option<Autonomy>,
    /// API mode only.
    pub chat_model: String,
    pub pass_model: String,
    pub monthly_cap_cents: Cents,
    /// Run the assistant pass at this local time (API mode).
    pub pass_time: ClockTime,
}

impl Default for AssistantSettings {
    fn default() -> Self {
        AssistantSettings {
            mode: AssistantMode::Pull,
            autonomy: Autonomy::Backseat,
            categorizing_notes: Some(Autonomy::Driver),
            plans_bills: None,
            debts_goals: None,
            chat_model: "claude-opus-5-5".into(),
            pass_model: "claude-haiku-5-5".into(),
            monthly_cap_cents: Cents::dollars(10),
            pass_time: ClockTime {
                hour: 6,
                minute: 30,
            },
        }
    }
}

impl AssistantSettings {
    /// Effective position for an area.
    pub fn autonomy_for(&self, area: AutonomyArea) -> Autonomy {
        let override_ = match area {
            AutonomyArea::CategorizingNotes => self.categorizing_notes,
            AutonomyArea::PlansBills => self.plans_bills,
            AutonomyArea::DebtsGoals => self.debts_goals,
        };
        override_.unwrap_or(self.autonomy)
    }
}

/// When paychecks arrive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaySchedule {
    Weekly {
        anchor: NaiveDate,
    },
    Biweekly {
        anchor: NaiveDate,
    },
    /// Two paydays a month, e.g. `[1, 15]` or `[15, 31]` (31 = last day).
    Semimonthly {
        days: [u32; 2],
    },
    Monthly {
        day: u32,
    },
}

impl Default for PaySchedule {
    fn default() -> Self {
        PaySchedule::Semimonthly { days: [1, 15] }
    }
}

impl PaySchedule {
    /// The first payday strictly after `today`.
    ///
    /// "Strictly after" because a paycheck landing today is either already in
    /// the reported balance or about to be; either way the *next* gap to cover
    /// ends at the following payday.
    pub fn next_payday(&self, today: NaiveDate) -> NaiveDate {
        match self {
            PaySchedule::Weekly { anchor } => next_periodic(*anchor, 7, today),
            PaySchedule::Biweekly { anchor } => next_periodic(*anchor, 14, today),
            PaySchedule::Semimonthly { days } => {
                let mut m = Month::of(today);
                for _ in 0..3 {
                    let mut cands: Vec<NaiveDate> =
                        days.iter().map(|d| m.date_with_day_clamped(*d)).collect();
                    cands.sort();
                    if let Some(d) = cands.into_iter().find(|d| *d > today) {
                        return d;
                    }
                    m = m.next();
                }
                unreachable!("a semimonthly schedule always has a payday within 3 months")
            }
            PaySchedule::Monthly { day } => {
                let m = Month::of(today);
                let this = m.date_with_day_clamped(*day);
                if this > today {
                    this
                } else {
                    m.next().date_with_day_clamped(*day)
                }
            }
        }
    }
}

fn next_periodic(anchor: NaiveDate, period_days: i64, today: NaiveDate) -> NaiveDate {
    let diff = (today - anchor).num_days();
    // number of whole periods from anchor to today (floor), then step one past.
    let k = diff.div_euclid(period_days) + 1;
    anchor + chrono::Duration::days(k * period_days)
}

/// Tolerances for matching an incoming transaction to a planned item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlannedMatch {
    /// ± days around the planned date.
    pub days: i64,
    /// Percent tolerance on the amount (10 = 10%).
    pub pct: i64,
    /// Absolute tolerance floor on the amount.
    pub min_cents: Cents,
}

impl Default for PlannedMatch {
    fn default() -> Self {
        PlannedMatch {
            days: 5,
            pct: 10,
            min_cents: Cents::dollars(5),
        }
    }
}

/// When to raise `category_pace`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaceAlert {
    /// Projected month-end spend as a percent of plan that trips the alert (110 = 110%).
    pub ratio_pct: i64,
    /// Don't evaluate before this day of month (too noisy early on).
    pub from_day: u32,
}

impl Default for PaceAlert {
    fn default() -> Self {
        PaceAlert {
            ratio_pct: 110,
            from_day: 10,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewBacklog {
    pub count: i64,
    pub days: i64,
}

impl Default for ReviewBacklog {
    fn default() -> Self {
        ReviewBacklog { count: 10, days: 3 }
    }
}

#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebtStrategyKind {
    /// Highest APR first.
    Avalanche,
    /// Smallest balance first.
    Snowball,
    /// `debts.custom_order` / `custom_order` list.
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DebtStrategy {
    pub kind: DebtStrategyKind,
    /// Extra paid each month on top of planned payments, to the first debt in order.
    pub extra_payment_cents: Cents,
    pub custom_order: Vec<Uuid>,
}

impl Default for DebtStrategy {
    fn default() -> Self {
        DebtStrategy {
            kind: DebtStrategyKind::Avalanche,
            extra_payment_cents: Cents::ZERO,
            custom_order: vec![],
        }
    }
}

/// `HH:MM` wall-clock time in the household timezone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClockTime {
    pub hour: u32,
    pub minute: u32,
}

impl ClockTime {
    pub fn minutes(self) -> u32 {
        self.hour * 60 + self.minute
    }
}

impl std::fmt::Display for ClockTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

impl std::str::FromStr for ClockTime {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (h, m) = s
            .split_once(':')
            .ok_or_else(|| format!("invalid time {s:?}; expected HH:MM"))?;
        let hour: u32 = h.parse().map_err(|_| format!("invalid hour in {s:?}"))?;
        let minute: u32 = m.parse().map_err(|_| format!("invalid minute in {s:?}"))?;
        if hour > 23 || minute > 59 {
            return Err(format!("time {s:?} out of range"));
        }
        Ok(ClockTime { hour, minute })
    }
}

impl Serialize for ClockTime {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ClockTime {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    pub start: ClockTime,
    pub end: ClockTime,
}

impl QuietHours {
    /// Is `t` inside the window? Handles windows that cross midnight.
    pub fn contains(&self, t: ClockTime) -> bool {
        let (s, e, x) = (self.start.minutes(), self.end.minutes(), t.minutes());
        if s <= e {
            x >= s && x < e
        } else {
            x >= s || x < e
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub quiet_hours: Option<QuietHours>,
    pub quiet_hours_override_critical: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        NotificationSettings {
            quiet_hours: Some(QuietHours {
                start: ClockTime {
                    hour: 22,
                    minute: 0,
                },
                end: ClockTime { hour: 7, minute: 0 },
            }),
            quiet_hours_override_critical: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NagSettings {
    pub first_after_minutes: i64,
    pub every_minutes: i64,
    pub max_nudges: i32,
}

impl Default for NagSettings {
    fn default() -> Self {
        NagSettings {
            first_after_minutes: 60,
            every_minutes: 240,
            max_nudges: 6,
        }
    }
}

/// Which day of the month a schedule-less thing recurs; helper for seeds/tests.
pub fn day_of(date: NaiveDate) -> u32 {
    date.day()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn defaults_roundtrip_json() {
        let s = Settings::default();
        let j = serde_json::to_value(&s).unwrap();
        let back: Settings = serde_json::from_value(j).unwrap();
        assert_eq!(s, back);
        // Partial JSON fills the rest with defaults.
        let partial: Settings = serde_json::from_str(r#"{"cash_floor_cents": 12345}"#).unwrap();
        assert_eq!(partial.cash_floor_cents, Cents(12345));
        assert_eq!(partial.nag, NagSettings::default());
    }

    #[test]
    fn autonomy_defaults() {
        let a = AssistantSettings::default();
        assert_eq!(a.mode, AssistantMode::Pull);
        assert_eq!(
            a.autonomy_for(AutonomyArea::CategorizingNotes),
            Autonomy::Driver
        );
        assert_eq!(a.autonomy_for(AutonomyArea::PlansBills), Autonomy::Backseat);
        assert_eq!(a.autonomy_for(AutonomyArea::DebtsGoals), Autonomy::Backseat);
        let j = serde_json::to_value(&a).unwrap();
        assert_eq!(j["mode"], "pull");
        assert_eq!(j["categorizing_notes"], "driver");
    }

    #[test]
    fn biweekly_next_payday() {
        let s = PaySchedule::Biweekly {
            anchor: d("2026-01-02"),
        };
        assert_eq!(s.next_payday(d("2026-01-02")), d("2026-01-16")); // payday today → next
        assert_eq!(s.next_payday(d("2026-01-03")), d("2026-01-16"));
        assert_eq!(s.next_payday(d("2026-01-15")), d("2026-01-16"));
        assert_eq!(
            s.next_payday(d("2025-12-20")),
            d("2025-12-19") + chrono::Duration::days(14)
        ); // before anchor
    }

    #[test]
    fn weekly_next_payday() {
        let s = PaySchedule::Weekly {
            anchor: d("2026-03-06"),
        };
        assert_eq!(s.next_payday(d("2026-03-10")), d("2026-03-13"));
    }

    #[test]
    fn semimonthly_next_payday() {
        let s = PaySchedule::Semimonthly { days: [15, 31] };
        assert_eq!(s.next_payday(d("2026-02-10")), d("2026-02-15"));
        assert_eq!(s.next_payday(d("2026-02-15")), d("2026-02-28")); // 31 clamps
        assert_eq!(s.next_payday(d("2026-02-28")), d("2026-03-15"));
        let s = PaySchedule::Semimonthly { days: [1, 15] };
        assert_eq!(s.next_payday(d("2026-01-20")), d("2026-02-01"));
    }

    #[test]
    fn monthly_next_payday() {
        let s = PaySchedule::Monthly { day: 31 };
        assert_eq!(s.next_payday(d("2026-01-31")), d("2026-02-28"));
        assert_eq!(s.next_payday(d("2026-01-01")), d("2026-01-31"));
    }

    #[test]
    fn quiet_hours_crossing_midnight() {
        let q: QuietHours = serde_json::from_str(r#"{"start":"22:00","end":"07:00"}"#).unwrap();
        assert!(q.contains("23:30".parse().unwrap()));
        assert!(q.contains("03:00".parse().unwrap()));
        assert!(!q.contains("07:00".parse().unwrap()));
        assert!(!q.contains("12:00".parse().unwrap()));
        let day = QuietHours {
            start: "09:00".parse().unwrap(),
            end: "17:00".parse().unwrap(),
        };
        assert!(day.contains("12:00".parse().unwrap()));
        assert!(!day.contains("20:00".parse().unwrap()));
        assert!("24:00".parse::<ClockTime>().is_err());
    }
}
