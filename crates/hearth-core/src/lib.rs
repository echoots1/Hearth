//! Hearth core: domain types and money math.
//!
//! This crate has no database, network, or async dependencies. Everything in
//! it is a pure function over plain values so it can be unit-tested
//! exhaustively and shared by the API, the MCP tools, and the jobs.
//!
//! Conventions (see `docs/ARCHITECTURE.md` §5):
//! - money is [`money::Cents`], an `i64` newtype; negative = out, positive = in;
//! - calendar dates are `chrono::NaiveDate` in the household timezone;
//! - months are [`calendar::Month`] (`YYYY-MM`).

pub mod calendar;
pub mod money;
