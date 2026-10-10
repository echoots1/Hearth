//! Household-timezone calendar helpers: `Month`, "today", day counts.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// A calendar month, `YYYY-MM`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month {
    year: i32,
    month: u32, // 1..=12
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid month {0:?}; expected YYYY-MM")]
pub struct MonthParseError(pub String);

impl Month {
    pub fn new(year: i32, month: u32) -> Option<Month> {
        (1..=12).contains(&month).then_some(Month { year, month })
    }

    pub fn of(date: NaiveDate) -> Month {
        Month {
            year: date.year(),
            month: date.month(),
        }
    }

    pub fn year(self) -> i32 {
        self.year
    }

    pub fn month(self) -> u32 {
        self.month
    }

    pub fn first_day(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, self.month, 1).expect("valid month")
    }

    pub fn last_day(self) -> NaiveDate {
        self.next().first_day().pred_opt().expect("valid date")
    }

    pub fn days_in_month(self) -> u32 {
        self.last_day().day()
    }

    pub fn next(self) -> Month {
        if self.month == 12 {
            Month {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Month {
                year: self.year,
                month: self.month + 1,
            }
        }
    }

    pub fn prev(self) -> Month {
        if self.month == 1 {
            Month {
                year: self.year - 1,
                month: 12,
            }
        } else {
            Month {
                year: self.year,
                month: self.month - 1,
            }
        }
    }

    pub fn add_months(self, n: i32) -> Month {
        let idx = self.year * 12 + (self.month as i32 - 1) + n;
        Month {
            year: idx.div_euclid(12),
            month: (idx.rem_euclid(12) + 1) as u32,
        }
    }

    /// Signed number of months from `self` to `other` (`other − self`).
    pub fn months_until(self, other: Month) -> i32 {
        (other.year * 12 + other.month as i32) - (self.year * 12 + self.month as i32)
    }

    /// Clamp a day-of-month (1–31) into this month, e.g. day 31 in February → 28/29.
    pub fn date_with_day_clamped(self, day: u32) -> NaiveDate {
        let d = day.clamp(1, self.days_in_month());
        NaiveDate::from_ymd_opt(self.year, self.month, d).expect("clamped day is valid")
    }

    pub fn contains(self, date: NaiveDate) -> bool {
        Month::of(date) == self
    }
}

impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

impl fmt::Debug for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Month({self})")
    }
}

impl FromStr for Month {
    type Err = MonthParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || MonthParseError(s.to_string());
        let (y, m) = s.split_once('-').ok_or_else(err)?;
        if y.len() != 4 || m.len() != 2 {
            return Err(err());
        }
        let year: i32 = y.parse().map_err(|_| err())?;
        let month: u32 = m.parse().map_err(|_| err())?;
        Month::new(year, month).ok_or_else(err)
    }
}

impl Serialize for Month {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Month {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// The current date in a household timezone for a given instant.
pub fn today_in(tz: Tz, now: DateTime<Utc>) -> NaiveDate {
    now.with_timezone(&tz).date_naive()
}

/// The household-local date of an instant (e.g. a reported-balance as-of).
pub fn local_date(tz: Tz, at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&tz).date_naive()
}

/// Build the UTC instant for a local wall-clock time; ambiguous/missing times
/// resolve to the earliest valid instant.
pub fn local_datetime_to_utc(tz: Tz, date: NaiveDate, hour: u32, minute: u32) -> DateTime<Utc> {
    let naive = date
        .and_hms_opt(hour, minute, 0)
        .expect("valid wall-clock time");
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) => t.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(a, _) => a.with_timezone(&Utc),
        chrono::LocalResult::None => {
            // DST gap: step forward an hour.
            let later = naive + chrono::Duration::hours(1);
            tz.from_local_datetime(&later)
                .earliest()
                .expect("time after a DST gap is valid")
                .with_timezone(&Utc)
        }
    }
}

/// Days from `from` to `to`, inclusive of `from`, exclusive of `to`; never negative.
pub fn days_between(from: NaiveDate, to: NaiveDate) -> i64 {
    (to - from).num_days().max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn month_parse_and_display() {
        let m: Month = "2026-02".parse().unwrap();
        assert_eq!(m.to_string(), "2026-02");
        assert!("2026-13".parse::<Month>().is_err());
        assert!("2026-2".parse::<Month>().is_err());
        assert!("202602".parse::<Month>().is_err());
    }

    #[test]
    fn month_arithmetic() {
        let m: Month = "2026-02".parse().unwrap();
        assert_eq!(m.days_in_month(), 28);
        assert_eq!(m.last_day(), d("2026-02-28"));
        assert_eq!(m.next().to_string(), "2026-03");
        assert_eq!(m.prev().to_string(), "2026-01");
        assert_eq!(m.add_months(11).to_string(), "2027-01");
        assert_eq!(m.add_months(-2).to_string(), "2025-12");
        assert_eq!(m.months_until("2027-01".parse().unwrap()), 11);
        assert_eq!(m.date_with_day_clamped(31), d("2026-02-28"));
        assert_eq!("2028-02".parse::<Month>().unwrap().days_in_month(), 29);
    }

    #[test]
    fn today_respects_timezone() {
        let tz: Tz = "America/Chicago".parse().unwrap();
        // 03:30 UTC on the 2nd is 22:30 on the 1st in Chicago (CDT).
        let now = Utc.with_ymd_and_hms(2026, 7, 2, 3, 30, 0).unwrap();
        assert_eq!(today_in(tz, now), d("2026-07-01"));
        assert_eq!(today_in(chrono_tz::UTC, now), d("2026-07-02"));
    }

    #[test]
    fn local_to_utc() {
        let tz: Tz = "America/Chicago".parse().unwrap();
        let t = local_datetime_to_utc(tz, d("2026-07-01"), 22, 0);
        assert_eq!(t, Utc.with_ymd_and_hms(2026, 7, 2, 3, 0, 0).unwrap());
    }

    #[test]
    fn days() {
        assert_eq!(days_between(d("2026-03-01"), d("2026-03-15")), 14);
        assert_eq!(days_between(d("2026-03-15"), d("2026-03-01")), 0);
    }
}
