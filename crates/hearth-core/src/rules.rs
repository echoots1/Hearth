//! Rule matching, description normalization, fingerprints, and note directives.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::money::Cents;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    Contains,
    Regex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Categorize,
    Ignore,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub id: Uuid,
    pub priority: i32,
    pub match_type: MatchType,
    pub pattern: String,
    pub account_id: Option<Uuid>,
    pub amount_min_cents: Option<Cents>,
    pub amount_max_cents: Option<Cents>,
    pub action: RuleAction,
    pub category_id: Option<Uuid>,
    pub set_note: Option<String>,
    pub is_active: bool,
}

#[derive(Clone, Debug)]
pub struct TxnForRules {
    pub account_id: Uuid,
    pub description: String,
    pub amount_cents: Cents,
}

/// Lowercase, collapse whitespace, trim. Applied before every match and
/// before fingerprinting so "SAMPLE  GROCER #123" and "sample grocer #123"
/// are the same thing.
pub fn normalize(description: &str) -> String {
    description
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Validate a pattern for its match type (regexes must compile).
pub fn validate_pattern(match_type: MatchType, pattern: &str) -> Result<(), String> {
    if pattern.trim().is_empty() {
        return Err("pattern is empty".into());
    }
    if match_type == MatchType::Regex {
        regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn pattern_matches(rule: &Rule, normalized: &str) -> bool {
    match rule.match_type {
        MatchType::Contains => normalized.contains(&normalize(&rule.pattern)),
        MatchType::Regex => regex::RegexBuilder::new(&rule.pattern)
            .case_insensitive(true)
            .size_limit(1 << 20)
            .build()
            .map(|re| re.is_match(normalized))
            .unwrap_or(false),
    }
}

/// Does `rule` match `txn`? Amount bounds apply to `abs(amount)`.
pub fn rule_matches(rule: &Rule, txn: &TxnForRules) -> bool {
    if !rule.is_active {
        return false;
    }
    if let Some(acct) = rule.account_id
        && acct != txn.account_id
    {
        return false;
    }
    let abs = txn.amount_cents.abs();
    if let Some(min) = rule.amount_min_cents
        && abs < min
    {
        return false;
    }
    if let Some(max) = rule.amount_max_cents
        && abs > max
    {
        return false;
    }
    pattern_matches(rule, &normalize(&txn.description))
}

/// The first matching rule in priority order (lower `priority` first; ties by id).
pub fn first_match<'a>(rules: &'a [Rule], txn: &TxnForRules) -> Option<&'a Rule> {
    let mut sorted: Vec<&Rule> = rules.iter().filter(|r| r.is_active).collect();
    sorted.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
    sorted.into_iter().find(|r| rule_matches(r, txn))
}

/// Suggest a `contains` pattern from a description: the normalized text with
/// trailing digit/reference noise stripped (e.g. "sample grocer #1234 03/14" → "sample grocer").
pub fn suggest_pattern(description: &str) -> String {
    let n = normalize(description);
    let words: Vec<&str> = n
        .split(' ')
        .take_while(|w| !w.chars().any(|c| c.is_ascii_digit()) && !w.starts_with('#'))
        .collect();
    let s = words.join(" ");
    if s.is_empty() { n } else { s }
}

/// Dedup fingerprint for sources without stable ids:
/// `sha256(account_id | date | amount_cents | normalize(description))`, hex.
pub fn fingerprint(account_id: Uuid, date: NaiveDate, amount: Cents, description: &str) -> String {
    let mut h = Sha256::new();
    h.update(account_id.as_bytes());
    h.update(b"|");
    h.update(date.to_string().as_bytes());
    h.update(b"|");
    h.update(amount.0.to_string().as_bytes());
    h.update(b"|");
    h.update(normalize(description).as_bytes());
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// Literal directives jobs honor in a category note. Everything else in the
/// note is for the AI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteDirectives {
    /// `[no-alerts]`: suppress every category-scoped alert.
    pub no_alerts: bool,
    /// `[no-large-charge]`: don't raise `unplanned_large_charge` for this category.
    pub no_large_charge: bool,
    /// `[no-pace]`: don't raise `category_pace` for this category.
    pub no_pace: bool,
}

impl NoteDirectives {
    pub fn parse(note: &str) -> NoteDirectives {
        let mut d = NoteDirectives::default();
        for tok in note.split_whitespace() {
            match tok.to_ascii_lowercase().as_str() {
                "[no-alerts]" => d.no_alerts = true,
                "[no-large-charge]" => d.no_large_charge = true,
                "[no-pace]" => d.no_pace = true,
                _ => {}
            }
        }
        d
    }

    pub fn suppress_large_charge(self) -> bool {
        self.no_alerts || self.no_large_charge
    }

    pub fn suppress_pace(self) -> bool {
        self.no_alerts || self.no_pace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(priority: i32, mt: MatchType, pattern: &str) -> Rule {
        Rule {
            id: Uuid::new_v4(),
            priority,
            match_type: mt,
            pattern: pattern.into(),
            account_id: None,
            amount_min_cents: None,
            amount_max_cents: None,
            action: RuleAction::Categorize,
            category_id: Some(Uuid::new_v4()),
            set_note: None,
            is_active: true,
        }
    }

    fn txn(desc: &str, amount: i64) -> TxnForRules {
        TxnForRules {
            account_id: Uuid::new_v4(),
            description: desc.into(),
            amount_cents: Cents(amount),
        }
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize("  SAMPLE   Grocer #123 "), "sample grocer #123");
    }

    #[test]
    fn contains_and_regex() {
        let r = rule(10, MatchType::Contains, "Sample GROCER");
        assert!(rule_matches(&r, &txn("SAMPLE GROCER #4521", -4200)));
        assert!(!rule_matches(&r, &txn("Example Fuel", -4200)));
        let r = rule(10, MatchType::Regex, r"^demo (stream|music)ing\b");
        assert!(rule_matches(&r, &txn("Demo Streaming 1234", -1599)));
        assert!(!rule_matches(&r, &txn("not demo streaming", -1599)));
        let bad = rule(10, MatchType::Regex, "(unclosed");
        assert!(!rule_matches(&bad, &txn("anything", 1)));
        assert!(validate_pattern(MatchType::Regex, "(unclosed").is_err());
        assert!(validate_pattern(MatchType::Contains, "   ").is_err());
        assert!(validate_pattern(MatchType::Contains, "ok").is_ok());
    }

    #[test]
    fn constraints() {
        let acct = Uuid::new_v4();
        let mut r = rule(1, MatchType::Contains, "transfer");
        r.account_id = Some(acct);
        r.amount_min_cents = Some(Cents(10_000));
        r.amount_max_cents = Some(Cents(50_000));
        let mut t = txn("Online Transfer", -20_000);
        assert!(!rule_matches(&r, &t)); // wrong account
        t.account_id = acct;
        assert!(rule_matches(&r, &t));
        t.amount_cents = Cents(-5_000);
        assert!(!rule_matches(&r, &t)); // below min
        t.amount_cents = Cents(60_000);
        assert!(!rule_matches(&r, &t)); // above max
        r.is_active = false;
        t.amount_cents = Cents(-20_000);
        assert!(!rule_matches(&r, &t));
    }

    #[test]
    fn priority_order() {
        let specific = rule(5, MatchType::Contains, "sample grocer pharmacy");
        let general = rule(50, MatchType::Contains, "sample grocer");
        let rules = vec![general.clone(), specific.clone()];
        assert_eq!(
            first_match(&rules, &txn("Sample Grocer Pharmacy", -1))
                .unwrap()
                .id,
            specific.id
        );
        assert_eq!(
            first_match(&rules, &txn("Sample Grocer 22", -1))
                .unwrap()
                .id,
            general.id
        );
        assert!(first_match(&rules, &txn("Unknown", -1)).is_none());
    }

    #[test]
    fn pattern_suggestion() {
        assert_eq!(
            suggest_pattern("SAMPLE GROCER #1234 03/14"),
            "sample grocer"
        );
        assert_eq!(suggest_pattern("Demo Streaming"), "demo streaming");
        assert_eq!(suggest_pattern("7-Eleven 1234"), "7-eleven 1234"); // nothing before digits → whole
    }

    #[test]
    fn fingerprint_is_stable_and_normalized() {
        let a = Uuid::new_v4();
        let d: NaiveDate = "2026-03-14".parse().unwrap();
        let f1 = fingerprint(a, d, Cents(-4200), "Sample  Grocer");
        let f2 = fingerprint(a, d, Cents(-4200), "sample grocer");
        let f3 = fingerprint(a, d, Cents(-4201), "sample grocer");
        assert_eq!(f1, f2);
        assert_ne!(f1, f3);
        assert_eq!(f1.len(), 64);
    }

    #[test]
    fn note_directives() {
        let d = NoteDirectives::parse("Shared card. [no-large-charge] Ask before flagging.");
        assert!(d.suppress_large_charge());
        assert!(!d.suppress_pace());
        let d = NoteDirectives::parse("[NO-ALERTS]");
        assert!(d.suppress_pace() && d.suppress_large_charge());
        assert_eq!(
            NoteDirectives::parse("nothing here"),
            NoteDirectives::default()
        );
    }
}
