//! Integer-cents money type.

use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// An amount of money in minor units (cents). Negative = money out.
///
/// `Cents` is `#[serde(transparent)]`, so it serializes as a plain integer in
/// JSON fields named `*_cents`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cents(pub i64);

impl Cents {
    pub const ZERO: Cents = Cents(0);

    pub const fn new(v: i64) -> Self {
        Cents(v)
    }

    /// Whole dollars → cents. Convenience for seeds and tests.
    pub const fn dollars(d: i64) -> Self {
        Cents(d * 100)
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    pub const fn abs(self) -> Self {
        Cents(self.0.abs())
    }

    pub fn max(self, other: Cents) -> Cents {
        Cents(self.0.max(other.0))
    }

    pub fn min(self, other: Cents) -> Cents {
        Cents(self.0.min(other.0))
    }

    /// Clamp at zero from below: `max(0, self)`.
    pub fn floor_zero(self) -> Cents {
        Cents(self.0.max(0))
    }

    /// Multiply by a rational `num / den`, rounding half away from zero.
    /// Panics if `den == 0`. All math stays in integers.
    pub fn mul_div(self, num: i64, den: i64) -> Cents {
        assert!(den != 0, "mul_div by zero");
        let n = self.0 as i128 * num as i128;
        let d = den as i128;
        let q = n / d;
        let r = n % d;
        // round half away from zero
        let adj = if (r.abs() * 2) >= d.abs() {
            if (n < 0) != (d < 0) { -1 } else { 1 }
        } else {
            0
        };
        Cents((q + adj) as i64)
    }

    /// Divide by an integer, rounding half away from zero.
    pub fn div_round(self, den: i64) -> Cents {
        self.mul_div(1, den)
    }

    /// Divide by an integer, rounding up (toward +∞) — used for set-asides
    /// where under-saving is the worse error.
    pub fn div_ceil(self, den: i64) -> Cents {
        assert!(den != 0, "div_ceil by zero");
        let q = self.0 / den;
        let r = self.0 % den;
        // Round toward +∞ when there is a remainder and the true quotient is positive.
        if r != 0 && ((r > 0) == (den > 0)) {
            Cents(q + 1)
        } else {
            Cents(q)
        }
    }

    /// Apply basis points: `self × bps / 10_000`, rounded half away from zero.
    pub fn apply_bps(self, bps: i64) -> Cents {
        self.mul_div(bps, 10_000)
    }

    /// Parse a decimal string ("12.50", "-3", "1,234.5", "$80", "(12.00)")
    /// into cents without touching floats. More than two decimals round
    /// half away from zero. `None` if it isn't a number.
    pub fn parse_decimal(text: &str) -> Option<Cents> {
        let t: String = text
            .trim()
            .chars()
            .filter(|c| !matches!(c, ',' | '$' | ' ' | '\u{a0}'))
            .collect();
        let (neg, body) = if let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')'))
        {
            (true, inner.to_string())
        } else if let Some(r) = t.strip_prefix('-').or_else(|| t.strip_prefix('\u{2212}')) {
            (true, r.to_string())
        } else {
            (false, t.strip_prefix('+').unwrap_or(&t).to_string())
        };
        if body.is_empty() {
            return None;
        }
        let (whole, frac) = match body.split_once('.') {
            Some((w, f)) => (w, f),
            None => (body.as_str(), ""),
        };
        if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if whole.is_empty() && frac.is_empty() {
            return None;
        }
        let whole: i64 = if whole.is_empty() {
            0
        } else {
            whole.parse().ok()?
        };
        let mut digits: Vec<u8> = frac.bytes().map(|b| b - b'0').collect();
        digits.resize(3, 0);
        let mut cents = whole.checked_mul(100)? + i64::from(digits[0]) * 10 + i64::from(digits[1]);
        if digits[2] >= 5 {
            cents += 1;
        }
        Some(Cents(if neg { -cents } else { cents }))
    }

    /// Format as a plain decimal string without currency symbol or grouping,
    /// e.g. `-1234.56`. Suitable for logs and notification text.
    pub fn to_decimal_string(self) -> String {
        let sign = if self.0 < 0 { "-" } else { "" };
        let a = self.0.abs();
        format!("{sign}{}.{:02}", a / 100, a % 100)
    }

    /// Format with a currency symbol and thousands grouping, e.g. `-$1,234.56`.
    pub fn format(self, symbol: &str) -> String {
        let sign = if self.0 < 0 { "-" } else { "" };
        let a = self.0.abs();
        let whole = a / 100;
        let frac = a % 100;
        let digits = whole.to_string();
        let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(ch);
        }
        format!("{sign}{symbol}{grouped}.{frac:02}")
    }

    /// Sum an iterator of `Cents`.
    pub fn sum<I: IntoIterator<Item = Cents>>(iter: I) -> Cents {
        iter.into_iter().fold(Cents::ZERO, |a, b| a + b)
    }
}

impl fmt::Debug for Cents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cents({})", self.to_decimal_string())
    }
}

impl fmt::Display for Cents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format("$"))
    }
}

impl Add for Cents {
    type Output = Cents;
    fn add(self, rhs: Cents) -> Cents {
        Cents(self.0 + rhs.0)
    }
}
impl AddAssign for Cents {
    fn add_assign(&mut self, rhs: Cents) {
        self.0 += rhs.0;
    }
}
impl Sub for Cents {
    type Output = Cents;
    fn sub(self, rhs: Cents) -> Cents {
        Cents(self.0 - rhs.0)
    }
}
impl SubAssign for Cents {
    fn sub_assign(&mut self, rhs: Cents) {
        self.0 -= rhs.0;
    }
}
impl Neg for Cents {
    type Output = Cents;
    fn neg(self) -> Cents {
        Cents(-self.0)
    }
}
impl std::iter::Sum for Cents {
    fn sum<I: Iterator<Item = Cents>>(iter: I) -> Cents {
        Cents::sum(iter)
    }
}
#[cfg(feature = "sqlx")]
mod sqlx_impl {
    use super::Cents;
    use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef, Postgres};
    use sqlx::{Decode, Encode, Type, encode::IsNull, error::BoxDynError};

    impl Type<Postgres> for Cents {
        fn type_info() -> PgTypeInfo {
            <i64 as Type<Postgres>>::type_info()
        }
        fn compatible(ty: &PgTypeInfo) -> bool {
            <i64 as Type<Postgres>>::compatible(ty)
        }
    }
    impl<'q> Encode<'q, Postgres> for Cents {
        fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
            <i64 as Encode<'q, Postgres>>::encode_by_ref(&self.0, buf)
        }
    }
    impl<'r> Decode<'r, Postgres> for Cents {
        fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
            <i64 as Decode<'r, Postgres>>::decode(value).map(Cents)
        }
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Cents {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Cents".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "integer", "format": "int64", "description": "Money in integer cents; negative = out." })
    }
}

impl From<i64> for Cents {
    fn from(v: i64) -> Self {
        Cents(v)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_decimal_cases() {
        use super::Cents;
        assert_eq!(Cents::parse_decimal("12.50"), Some(Cents(1250)));
        assert_eq!(Cents::parse_decimal("-3"), Some(Cents(-300)));
        assert_eq!(Cents::parse_decimal("1,234.5"), Some(Cents(123_450)));
        assert_eq!(Cents::parse_decimal("$80"), Some(Cents(8000)));
        assert_eq!(Cents::parse_decimal("(12.00)"), Some(Cents(-1200)));
        assert_eq!(Cents::parse_decimal("0.005"), Some(Cents(1)));
        assert_eq!(Cents::parse_decimal(".5"), Some(Cents(50)));
        assert_eq!(Cents::parse_decimal("\u{2212}7.25"), Some(Cents(-725)));
        assert_eq!(Cents::parse_decimal("abc"), None);
        assert_eq!(Cents::parse_decimal(""), None);
        assert_eq!(Cents::parse_decimal("1.2.3"), None);
    }

    use super::*;

    #[test]
    fn formatting() {
        assert_eq!(Cents(0).format("$"), "$0.00");
        assert_eq!(Cents(5).format("$"), "$0.05");
        assert_eq!(Cents(123_456).format("$"), "$1,234.56");
        assert_eq!(Cents(-123_456).format("$"), "-$1,234.56");
        assert_eq!(Cents(100_000_000).format("$"), "$1,000,000.00");
        assert_eq!(Cents(-7).to_decimal_string(), "-0.07");
    }

    #[test]
    fn mul_div_rounds_half_away_from_zero() {
        assert_eq!(Cents(100).mul_div(1, 3), Cents(33));
        assert_eq!(Cents(200).mul_div(1, 3), Cents(67));
        assert_eq!(Cents(5).mul_div(1, 2), Cents(3));
        assert_eq!(Cents(-5).mul_div(1, 2), Cents(-3));
        assert_eq!(Cents(1_000).apply_bps(2499), Cents(250)); // 24.99% of $10 = $2.499 → $2.50
        assert_eq!(Cents(1_000).apply_bps(0), Cents(0));
    }

    #[test]
    fn div_ceil_rounds_up() {
        assert_eq!(Cents(100).div_ceil(3), Cents(34));
        assert_eq!(Cents(99).div_ceil(3), Cents(33));
        assert_eq!(Cents(120_000).div_ceil(12), Cents(10_000));
    }

    #[test]
    fn arithmetic_and_sum() {
        let v = [Cents(10), Cents(-4), Cents(1)];
        assert_eq!(v.iter().copied().sum::<Cents>(), Cents(7));
        assert_eq!(Cents(-3).floor_zero(), Cents::ZERO);
        assert_eq!(Cents(3).floor_zero(), Cents(3));
        assert_eq!(-Cents(3), Cents(-3));
    }

    #[test]
    fn serde_is_transparent() {
        #[derive(Serialize, Deserialize)]
        struct Row {
            amount_cents: Cents,
        }
        let r: Row = serde_json::from_str(r#"{"amount_cents": -1250}"#).unwrap();
        assert_eq!(r.amount_cents, Cents(-1250));
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"amount_cents":-1250}"#
        );
    }
}
