//! CSV import: map columns once per bank, preview with duplicate detection,
//! import as a batch, roll a batch back.

use chrono::NaiveDate;
use hearth_core::money::Cents;
use hearth_core::rules::fingerprint;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::db::models::ImportBatch;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
    /// Column names (header) or `col1`, `col2`… when there is no header.
    pub date: String,
    pub description: String,
    /// One signed amount column, or separate debit/credit columns.
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub debit: Option<String>,
    #[serde(default)]
    pub credit: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    /// `auto`, or a chrono format like `%m/%d/%Y`.
    #[serde(default = "auto")]
    pub date_format: String,
    /// Flip the sign of the amount column (banks that write charges as positive).
    #[serde(default)]
    pub negate: bool,
    #[serde(default = "yes")]
    pub has_header: bool,
}

fn auto() -> String {
    "auto".into()
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub mapping: Mapping,
    #[serde(default)]
    pub account_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParsedRow {
    pub line: usize,
    pub date: Option<NaiveDate>,
    pub description: String,
    pub amount_cents: Option<Cents>,
    pub external_id: Option<String>,
    pub duplicate: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub headers: Vec<String>,
    pub rows: Vec<ParsedRow>,
    pub total: usize,
    pub ok: usize,
    pub duplicates: usize,
    pub errors: usize,
    pub truncated: bool,
}

const DATE_FORMATS: &[&str] = &[
    "%Y-%m-%d",
    "%m/%d/%Y",
    "%m/%d/%y",
    "%Y/%m/%d",
    "%d/%m/%Y",
    "%m-%d-%Y",
    "%d-%m-%Y",
    "%b %d, %Y",
    "%d %b %Y",
    "%B %d, %Y",
    "%Y%m%d",
];

pub fn parse_date(s: &str, format: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if format != "auto" {
        return NaiveDate::parse_from_str(s, format).ok();
    }
    // ISO with a time part first.
    if s.len() >= 10
        && let Ok(d) = NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d")
    {
        return Some(d);
    }
    DATE_FORMATS
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(s, f).ok())
}

/// Read the headers and all records as string vectors.
pub fn read(
    text: &str,
    has_header: bool,
) -> std::result::Result<(Vec<String>, Vec<Vec<String>>), String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let mut rows: Vec<Vec<String>> = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let v: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
        if v.iter().all(|c| c.is_empty()) {
            continue;
        }
        rows.push(v);
    }
    if rows.is_empty() {
        return Err("the file has no rows".into());
    }
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let headers: Vec<String> = if has_header {
        let h = rows.remove(0);
        (0..width)
            .map(|i| {
                h.get(i)
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("col{}", i + 1))
            })
            .collect()
    } else {
        (0..width).map(|i| format!("col{}", i + 1)).collect()
    };
    Ok((headers, rows))
}

fn col<'a>(headers: &[String], row: &'a [String], name: &str) -> Option<&'a str> {
    let idx = headers.iter().position(|h| h.eq_ignore_ascii_case(name))?;
    row.get(idx).map(|s| s.as_str())
}

pub fn parse_rows(headers: &[String], rows: &[Vec<String>], m: &Mapping) -> Vec<ParsedRow> {
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let line = i + if m.has_header { 2 } else { 1 };
            let date_text = col(headers, r, &m.date).unwrap_or("");
            let date = parse_date(date_text, &m.date_format);
            let description = col(headers, r, &m.description)
                .unwrap_or("")
                .trim()
                .to_string();
            let amount = if let Some(a) = &m.amount {
                col(headers, r, a)
                    .and_then(Cents::parse_decimal)
                    .map(|c| if m.negate { -c } else { c })
            } else {
                let debit = m
                    .debit
                    .as_ref()
                    .and_then(|c| col(headers, r, c))
                    .and_then(Cents::parse_decimal);
                let credit = m
                    .credit
                    .as_ref()
                    .and_then(|c| col(headers, r, c))
                    .and_then(Cents::parse_decimal);
                match (debit, credit) {
                    (Some(d), _) if d.0 != 0 => Some(-d.abs()),
                    (_, Some(c)) if c.0 != 0 => Some(c.abs()),
                    (Some(_), _) | (_, Some(_)) => Some(Cents::ZERO),
                    _ => None,
                }
            };
            let external_id = m
                .external_id
                .as_ref()
                .and_then(|c| col(headers, r, c))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let error = if date.is_none() {
                Some(format!("can't read date {date_text:?}"))
            } else if description.is_empty() {
                Some("empty description".into())
            } else if amount.is_none() {
                Some("can't read amount".into())
            } else if amount == Some(Cents::ZERO) {
                Some("zero amount".into())
            } else {
                None
            };
            ParsedRow {
                line,
                date,
                description,
                amount_cents: amount,
                external_id,
                duplicate: false,
                error,
            }
        })
        .collect()
}

/// Mark rows that already exist on the account (same fingerprint or external id).
async fn mark_duplicates(ctx: &Ctx, account_id: Uuid, rows: &mut [ParsedRow]) -> Result<()> {
    let fps: Vec<String> = rows
        .iter()
        .filter(|r| r.error.is_none())
        .filter_map(|r| {
            Some(fingerprint(
                account_id,
                r.date?,
                r.amount_cents?,
                &r.description,
            ))
        })
        .collect();
    let existing: Vec<(String,)> = sqlx::query_as(
        "select fingerprint from transactions where account_id = $1 and deleted_at is null and fingerprint = any($2)",
    )
    .bind(account_id)
    .bind(&fps)
    .fetch_all(&ctx.pool)
    .await?;
    let set: std::collections::HashSet<String> = existing.into_iter().map(|e| e.0).collect();
    let mut seen = std::collections::HashSet::new();
    for r in rows.iter_mut() {
        if r.error.is_some() {
            continue;
        }
        let fp = fingerprint(
            account_id,
            r.date.unwrap_or_default(),
            r.amount_cents.unwrap_or(Cents::ZERO),
            &r.description,
        );
        if set.contains(&fp) || !seen.insert(fp) {
            r.duplicate = true;
        }
    }
    Ok(())
}

pub async fn preview(ctx: &Ctx, account_id: Uuid, text: &str, m: &Mapping) -> Result<Preview> {
    super::accounts::get(ctx, account_id).await?;
    let (headers, rows) = read(text, m.has_header).map_err(invalid)?;
    let mut parsed = parse_rows(&headers, &rows, m);
    mark_duplicates(ctx, account_id, &mut parsed).await?;
    let total = parsed.len();
    let duplicates = parsed.iter().filter(|r| r.duplicate).count();
    let errors = parsed.iter().filter(|r| r.error.is_some()).count();
    let truncated = parsed.len() > 500;
    parsed.truncate(500);
    Ok(Preview {
        headers,
        rows: parsed,
        total,
        ok: total - duplicates - errors,
        duplicates,
        errors,
        truncated,
    })
}

/// Import every clean, non-duplicate row as one batch.
pub async fn commit(
    ctx: &Ctx,
    actor: &Actor,
    account_id: Uuid,
    filename: &str,
    text: &str,
    m: &Mapping,
) -> Result<ImportBatch> {
    super::accounts::get(ctx, account_id).await?;
    let (headers, rows) = read(text, m.has_header).map_err(invalid)?;
    let mut parsed = parse_rows(&headers, &rows, m);
    mark_duplicates(ctx, account_id, &mut parsed).await?;
    let batch = sqlx::query_as::<_, ImportBatch>(
        "insert into import_batches (household_id, account_id, filename, mapping, row_count, created_by) values ($1, $2, $3, $4, $5, $6) returning *",
    )
    .bind(ctx.household_id)
    .bind(account_id)
    .bind(filename)
    .bind(serde_json::to_value(m).map_err(anyhow::Error::from)?)
    .bind(parsed.len() as i32)
    .bind(match actor {
        Actor::User { id, .. } => Some(*id),
        _ => None,
    })
    .fetch_one(&ctx.pool)
    .await?;
    let mut inserted = 0i32;
    let mut duplicates = 0i32;
    for r in parsed.iter().filter(|r| r.error.is_none()) {
        if r.duplicate {
            duplicates += 1;
            continue;
        }
        let (Some(date), Some(amount)) = (r.date, r.amount_cents) else {
            continue;
        };
        match super::transactions::insert_csv_row(
            ctx,
            account_id,
            batch.id,
            date,
            &r.description,
            amount,
            r.external_id.as_deref(),
        )
        .await?
        {
            Some(_) => inserted += 1,
            None => duplicates += 1,
        }
    }
    let batch = sqlx::query_as::<_, ImportBatch>(
        "update import_batches set inserted_count = $2, duplicate_count = $3 where id = $1 returning *",
    )
    .bind(batch.id)
    .bind(inserted)
    .bind(duplicates)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "import.csv",
        "import_batches",
        batch.id,
        None::<&()>,
        Some(&batch),
    )
    .await?;
    Ok(batch)
}

pub async fn batches(ctx: &Ctx) -> Result<Vec<ImportBatch>> {
    Ok(sqlx::query_as::<_, ImportBatch>(
        "select * from import_batches where household_id = $1 order by created_at desc limit 50",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

/// Soft-delete every row the batch inserted.
pub async fn rollback(ctx: &Ctx, actor: &Actor, id: Uuid) -> Result<ImportBatch> {
    let before = sqlx::query_as::<_, ImportBatch>(
        "select * from import_batches where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)?;
    if before.rolled_back_at.is_some() {
        return Err(invalid("this import was already rolled back"));
    }
    sqlx::query("update transactions set deleted_at = now(), updated_at = now() where import_batch_id = $1 and deleted_at is null")
        .bind(id)
        .execute(&ctx.pool)
        .await?;
    let after = sqlx::query_as::<_, ImportBatch>(
        "update import_batches set rolled_back_at = now() where id = $1 returning *",
    )
    .bind(id)
    .fetch_one(&ctx.pool)
    .await?;
    super::audit::record(
        &ctx.pool,
        ctx.household_id,
        actor,
        "import.rollback",
        "import_batches",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    Ok(after)
}

pub async fn presets(ctx: &Ctx) -> Result<Vec<Preset>> {
    let v = super::settings::get(ctx, "csv_presets")
        .await
        .unwrap_or(serde_json::Value::Array(vec![]));
    Ok(serde_json::from_value(v).unwrap_or_default())
}

pub async fn save_preset(ctx: &Ctx, actor: &Actor, p: Preset) -> Result<Vec<Preset>> {
    if p.name.trim().is_empty() {
        return Err(invalid("preset name is required"));
    }
    let mut list = presets(ctx).await?;
    list.retain(|x| !x.name.eq_ignore_ascii_case(&p.name));
    list.push(p);
    list.sort_by_key(|x| x.name.to_lowercase());
    super::settings::set(
        ctx,
        actor,
        "csv_presets",
        serde_json::to_value(&list).map_err(anyhow::Error::from)?,
    )
    .await?;
    Ok(list)
}

pub async fn delete_preset(ctx: &Ctx, actor: &Actor, name: &str) -> Result<Vec<Preset>> {
    let mut list = presets(ctx).await?;
    list.retain(|x| !x.name.eq_ignore_ascii_case(name));
    super::settings::set(
        ctx,
        actor,
        "csv_presets",
        serde_json::to_value(&list).map_err(anyhow::Error::from)?,
    )
    .await?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(amount: Option<&str>) -> Mapping {
        Mapping {
            date: "Date".into(),
            description: "Description".into(),
            amount: amount.map(str::to_string),
            debit: if amount.is_none() {
                Some("Debit".into())
            } else {
                None
            },
            credit: if amount.is_none() {
                Some("Credit".into())
            } else {
                None
            },
            external_id: None,
            date_format: "auto".into(),
            negate: false,
            has_header: true,
        }
    }

    #[test]
    fn parses_signed_amount_column() {
        let text = "Date,Description,Amount\n10/03/2026,SAMPLE GROCER,-42.10\n2026-10-04,\"PAYROLL, INC\",\"1,848.00\"\n";
        let (h, rows) = read(text, true).unwrap();
        let p = parse_rows(&h, &rows, &mapping(Some("Amount")));
        assert_eq!(
            p[0].date,
            Some(NaiveDate::from_ymd_opt(2026, 10, 3).unwrap())
        );
        assert_eq!(p[0].amount_cents, Some(Cents(-4210)));
        assert_eq!(p[1].description, "PAYROLL, INC");
        assert_eq!(p[1].amount_cents, Some(Cents(184_800)));
        assert!(p.iter().all(|r| r.error.is_none()));
    }

    #[test]
    fn parses_debit_credit_columns_and_flags_errors() {
        let text = "Date,Description,Debit,Credit\n10/03/2026,FUEL,38.11,\n10/04/2026,REFUND,,12.00\nnot a date,X,1,\n";
        let (h, rows) = read(text, true).unwrap();
        let p = parse_rows(&h, &rows, &mapping(None));
        assert_eq!(p[0].amount_cents, Some(Cents(-3811)));
        assert_eq!(p[1].amount_cents, Some(Cents(1200)));
        assert!(p[2].error.as_deref().unwrap().contains("date"));
    }

    #[test]
    fn no_header_uses_col_names() {
        let text = "2026-10-01,A,1.00\n";
        let (h, rows) = read(text, false).unwrap();
        assert_eq!(h, vec!["col1", "col2", "col3"]);
        let m = Mapping {
            date: "col1".into(),
            description: "col2".into(),
            amount: Some("col3".into()),
            debit: None,
            credit: None,
            external_id: None,
            date_format: "auto".into(),
            negate: true,
            has_header: false,
        };
        let p = parse_rows(&h, &rows, &m);
        assert_eq!(p[0].amount_cents, Some(Cents(-100)));
    }
}
