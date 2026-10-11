//! `hearth seed`: an obviously fake demo household, built through the services
//! layer so the audit log, the autonomy gate, proposals and actions all look
//! the way real use produces them.
//!
//! Nothing here is real: "Demo Household", `owner@example.com`, "Sample
//! Grocer". Dates are relative to today so the demo is always live. The
//! command refuses to run on a database that already has users unless
//! `--force` wipes it first.

use anyhow::{Context as _, bail};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use hearth_core::alerts::{AlertCandidate, AlertType, Severity};
use hearth_core::calendar::Month;
use hearth_core::money::Cents;
use hearth_core::settings::{AutonomyArea, DebtStrategy, DebtStrategyKind, PaySchedule};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::passwords;
use crate::config::Config;
use crate::db::models::{
    AccountType, BillStatus, CategorySign, DebtKind, PlannedStatus, RuleAction, RuleMatchType,
};
use crate::services::{
    self, Actor, Ctx, accounts, bills, budget, debts, goals, planned, questions, rules, sinking,
    transactions,
};

pub const OWNER_EMAIL: &str = "owner@example.com";
pub const MEMBER_EMAIL: &str = "member@example.com";
pub const PASSWORD: &str = "demo-password";

/// Tables that hold household data, in an order `truncate … cascade` is happy with.
const DATA_TABLES: &[&str] = &[
    "audit_log",
    "job_runs",
    "card_events",
    "notifications",
    "push_subscriptions",
    "assistant_usage",
    "assistant_actions",
    "questions",
    "alerts",
    "balance_snapshots",
    "goals",
    "debts",
    "sinking_funds",
    "transactions",
    "planned_items",
    "import_batches",
    "rules",
    "bill_splits",
    "recurring_bills",
    "category_plans",
    "categories",
    "category_groups",
    "accounts",
    "plaid_items",
    "secrets",
    "settings",
    "api_keys",
    "sessions",
    "users",
    "households",
];

/// Deterministic pseudo-randomness so the demo is the same on every machine.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    /// A value in `lo..=hi` dollars, in cents.
    fn dollars(&mut self, lo: i64, hi: i64) -> Cents {
        let span = (hi - lo + 1) as u64;
        let d = lo + (self.next() % span) as i64;
        let c = (self.next() % 100) as i64;
        Cents::new(d * 100 + c)
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
}

pub async fn run(config: Config, pool: PgPool, force: bool) -> anyhow::Result<()> {
    crate::db::migrate(&pool).await?;
    let users: i64 = sqlx::query_scalar("select count(*) from users")
        .fetch_one(&pool)
        .await?;
    if users > 0 {
        if !force {
            bail!("this database already has users; pass --force to wipe it and seed the demo");
        }
        // Table names come from the constant list above, not from input.
        let stmt = format!("truncate {} cascade", DATA_TABLES.join(", "));
        sqlx::query(sqlx::AssertSqlSafe(stmt))
            .execute(&pool)
            .await?;
        tracing::warn!("wiped existing data (--force)");
    }

    let state = AppState::new(config, pool.clone()).await?;
    let system = Actor::System;
    let mut ctx = state.ctx().await?;

    // ── Household and people ──
    services::household::update(
        &ctx,
        &system,
        services::household::UpdateHousehold {
            name: Some("Demo Household".into()),
            timezone: Some("America/Chicago".into()),
            currency: None,
        },
    )
    .await?;
    ctx = state.ctx().await?; // pick up the timezone
    let hash = passwords::hash(PASSWORD).context("hashing the demo password")?;
    let owner_id: Uuid = sqlx::query_scalar(
        "insert into users (household_id, email, display_name, password_hash, role) values ($1, $2, 'Demo Owner', $3, 'owner') returning id",
    )
    .bind(ctx.household_id)
    .bind(OWNER_EMAIL)
    .bind(&hash)
    .fetch_one(&pool)
    .await?;
    sqlx::query(
        "insert into users (household_id, email, display_name, password_hash, role) values ($1, $2, 'Demo Member', $3, 'member')",
    )
    .bind(ctx.household_id)
    .bind(MEMBER_EMAIL)
    .bind(&hash)
    .execute(&pool)
    .await?;
    let owner = Actor::User {
        id: owner_id,
        name: "Demo Owner".into(),
    };
    let claude = Actor::Assistant {
        key_id: None,
        name: "claude".into(),
    };

    let today = ctx.today();
    let month = Month::of(today);
    let mut rng = Lcg(20_261_011);

    // ── Settings ──
    services::settings::set(
        &ctx,
        &owner,
        "pay_schedule",
        serde_json::to_value(PaySchedule::Semimonthly { days: [1, 15] })?,
    )
    .await?;
    services::settings::set(&ctx, &owner, "cash_floor_cents", json!(50_000)).await?;

    // ── Accounts ──
    let checking = account(&ctx, &owner, "Demo Checking", AccountType::Checking, "1234").await?;
    let savings = account(&ctx, &owner, "Demo Savings", AccountType::Savings, "5678").await?;
    let card = account(
        &ctx,
        &owner,
        "Demo Credit Card",
        AccountType::CreditCard,
        "4242",
    )
    .await?;
    let vcard = account(
        &ctx,
        &owner,
        "Demo Virtual Card",
        AccountType::VirtualCard,
        "0007",
    )
    .await?;

    // ── Groups and categories ──
    let g_income = group(&ctx, &owner, "Income", 0).await?;
    let g_housing = group(&ctx, &owner, "Housing", 1).await?;
    let g_everyday = group(&ctx, &owner, "Everyday", 2).await?;
    let g_bills = group(&ctx, &owner, "Bills", 3).await?;
    let g_debt = group(&ctx, &owner, "Debt & savings", 4).await?;

    let c_paycheck = category(
        &ctx,
        &owner,
        g_income,
        "Paycheck",
        CategorySign::Income,
        false,
        None,
    )
    .await?;
    let c_other_income = category(
        &ctx,
        &owner,
        g_income,
        "Other income",
        CategorySign::Income,
        false,
        None,
    )
    .await?;
    let c_rent = category(
        &ctx,
        &owner,
        g_housing,
        "Rent",
        CategorySign::Spend,
        false,
        Some(145_000),
    )
    .await?;
    let c_electric = category(
        &ctx,
        &owner,
        g_housing,
        "Electric",
        CategorySign::Spend,
        false,
        Some(12_000),
    )
    .await?;
    let c_water = category(
        &ctx,
        &owner,
        g_housing,
        "Water & trash",
        CategorySign::Spend,
        false,
        Some(5_500),
    )
    .await?;
    let c_internet = category(
        &ctx,
        &owner,
        g_housing,
        "Internet",
        CategorySign::Spend,
        false,
        Some(6_500),
    )
    .await?;
    let c_groceries = category(
        &ctx,
        &owner,
        g_everyday,
        "Groceries",
        CategorySign::Spend,
        true,
        Some(80_000),
    )
    .await?;
    let c_fuel = category(
        &ctx,
        &owner,
        g_everyday,
        "Fuel",
        CategorySign::Spend,
        true,
        Some(22_000),
    )
    .await?;
    let c_dining = category(
        &ctx,
        &owner,
        g_everyday,
        "Dining out",
        CategorySign::Spend,
        true,
        Some(15_000),
    )
    .await?;
    let c_household = category(
        &ctx,
        &owner,
        g_everyday,
        "Household",
        CategorySign::Spend,
        true,
        Some(20_000),
    )
    .await?;
    let c_streaming = category(
        &ctx,
        &owner,
        g_bills,
        "Streaming",
        CategorySign::Spend,
        false,
        Some(1_200),
    )
    .await?;
    let c_phone = category(
        &ctx,
        &owner,
        g_bills,
        "Phone",
        CategorySign::Spend,
        false,
        Some(7_000),
    )
    .await?;
    let c_insurance = category(
        &ctx,
        &owner,
        g_bills,
        "Car insurance",
        CategorySign::Spend,
        false,
        Some(5_400),
    )
    .await?;
    let c_gym = category(
        &ctx,
        &owner,
        g_bills,
        "Gym",
        CategorySign::Spend,
        false,
        Some(0),
    )
    .await?;
    let c_card_payment = category(
        &ctx,
        &owner,
        g_debt,
        "Credit card payment",
        CategorySign::Spend,
        false,
        Some(25_000),
    )
    .await?;
    let c_emergency = category(
        &ctx,
        &owner,
        g_debt,
        "Emergency fund",
        CategorySign::Spend,
        false,
        Some(25_000),
    )
    .await?;

    // The plan phases: groceries started lower three months ago and steps up after the holidays.
    budget::set_plan(
        &ctx,
        &owner,
        c_groceries,
        month.add_months(-3),
        Cents::dollars(700),
        None,
        None,
    )
    .await?;
    budget::set_plan(
        &ctx,
        &owner,
        c_groceries,
        month.add_months(-1),
        Cents::dollars(800),
        None,
        None,
    )
    .await?;
    budget::set_plan(
        &ctx,
        &owner,
        c_groceries,
        month.add_months(3),
        Cents::dollars(900),
        Some("After the move".into()),
        None,
    )
    .await?;
    budget::set_note(
        &ctx,
        &owner,
        c_groceries,
        "Warehouse-store runs count here even when the receipt has household stuff on it. Don't flag a big run before a holiday. [no-large-charge]".into(),
    )
    .await?;
    budget::set_note(
        &ctx,
        &owner,
        c_dining,
        "Friday takeout is fine; flag anything over $60.".into(),
    )
    .await?;

    // ── Recurring bills ──
    let b_rent = bill(
        &ctx,
        &owner,
        "Demo Property Mgmt",
        145_000,
        1,
        next_due(today, 1),
        c_rent,
        checking,
        Some("demo property"),
    )
    .await?;
    let b_electric = bill(
        &ctx,
        &owner,
        "Sample Electric Co",
        11_800,
        1,
        next_due(today, 12),
        c_electric,
        checking,
        Some("sample electric"),
    )
    .await?;
    let b_internet = bill(
        &ctx,
        &owner,
        "Example Fiber",
        6_500,
        1,
        next_due(today, 20),
        c_internet,
        card,
        Some("example fiber"),
    )
    .await?;
    let b_video = bill(
        &ctx,
        &owner,
        "Demo Video",
        1_199,
        1,
        next_due(today, 14),
        c_streaming,
        vcard,
        Some("demo video"),
    )
    .await?;
    let b_phone = bill(
        &ctx,
        &owner,
        "Sample Wireless",
        7_000,
        1,
        next_due(today, 8),
        c_phone,
        card,
        Some("sample wireless"),
    )
    .await?;
    let b_gym = bill(
        &ctx,
        &owner,
        "Example Fitness",
        3_500,
        1,
        next_due(today, 3),
        c_gym,
        card,
        Some("example fitness"),
    )
    .await?;
    bills::set_status(&ctx, &owner, b_gym, BillStatus::Canceled).await?;
    let _b_water = bill(
        &ctx,
        &owner,
        "City Water & Trash (demo)",
        16_500,
        3,
        next_due(today, 25),
        c_water,
        checking,
        Some("city water"),
    )
    .await?;
    let _b_insurance = bill(
        &ctx,
        &owner,
        "Sample Mutual Auto",
        64_800,
        12,
        month.add_months(5).date_with_day_clamped(9),
        c_insurance,
        checking,
        Some("sample mutual"),
    )
    .await?;

    // ── Rules ──
    rule(&ctx, &owner, "sample grocer", c_groceries).await?;
    rule(&ctx, &owner, "example fuel", c_fuel).await?;
    rule(&ctx, &owner, "demo video", c_streaming).await?;
    rule(&ctx, &owner, "corner cafe", c_dining).await?;

    // ── Ninety days of transactions ──
    let start = today - Duration::days(90);
    let mut d = start;
    let mut uncategorized: Vec<Uuid> = Vec::new();
    let mut warehouse_last: Option<Uuid> = None;
    while d <= today {
        let dom = d.day();
        if dom == 1 || dom == 15 {
            txn(
                &ctx,
                &owner,
                checking,
                d,
                "SAMPLE EMPLOYER PAYROLL",
                265_000,
                Some(c_paycheck),
            )
            .await?;
        }
        if dom == 1 {
            let t = txn(
                &ctx,
                &owner,
                checking,
                d,
                "DEMO PROPERTY MGMT RENT",
                -145_000,
                Some(c_rent),
            )
            .await?;
            transactions::set_bill_match(&ctx, t, Some(b_rent), Some(Cents::new(0)), Some(c_rent))
                .await?;
        }
        if dom == 12 {
            let amt = rng.dollars(104, 131);
            let t = txn(
                &ctx,
                &owner,
                checking,
                d,
                "SAMPLE ELECTRIC CO",
                -amt.0,
                Some(c_electric),
            )
            .await?;
            transactions::set_bill_match(
                &ctx,
                t,
                Some(b_electric),
                Some(Cents::new(amt.0 - 11_800)),
                Some(c_electric),
            )
            .await?;
        }
        if dom == 20 {
            let t = txn(
                &ctx,
                &owner,
                card,
                d,
                "EXAMPLE FIBER",
                -6_500,
                Some(c_internet),
            )
            .await?;
            transactions::set_bill_match(
                &ctx,
                t,
                Some(b_internet),
                Some(Cents::new(0)),
                Some(c_internet),
            )
            .await?;
        }
        if dom == 14 {
            let t = txn(
                &ctx,
                &owner,
                vcard,
                d,
                "DEMO VIDEO",
                -1_199,
                Some(c_streaming),
            )
            .await?;
            transactions::set_bill_match(
                &ctx,
                t,
                Some(b_video),
                Some(Cents::new(0)),
                Some(c_streaming),
            )
            .await?;
        }
        if dom == 8 {
            let t = txn(
                &ctx,
                &owner,
                card,
                d,
                "SAMPLE WIRELESS",
                -7_000,
                Some(c_phone),
            )
            .await?;
            transactions::set_bill_match(
                &ctx,
                t,
                Some(b_phone),
                Some(Cents::new(0)),
                Some(c_phone),
            )
            .await?;
        }
        if dom == 3 && d < today - Duration::days(40) {
            let t = txn(
                &ctx,
                &owner,
                card,
                d,
                "EXAMPLE FITNESS",
                -3_500,
                Some(c_gym),
            )
            .await?;
            transactions::set_bill_match(&ctx, t, Some(b_gym), Some(Cents::new(0)), Some(c_gym))
                .await?;
        }
        if dom == 21 {
            txn(
                &ctx,
                &owner,
                checking,
                d,
                "PAYMENT TO DEMO CREDIT CARD",
                -25_000,
                Some(c_card_payment),
            )
            .await?;
            txn(
                &ctx,
                &owner,
                card,
                d,
                "PAYMENT RECEIVED - THANK YOU",
                25_000,
                Some(c_card_payment),
            )
            .await?;
        }
        if dom == 2 {
            txn(
                &ctx,
                &owner,
                checking,
                d,
                "TRANSFER TO DEMO SAVINGS",
                -25_000,
                Some(c_emergency),
            )
            .await?;
            txn(
                &ctx,
                &owner,
                savings,
                d,
                "TRANSFER FROM DEMO CHECKING",
                25_000,
                Some(c_emergency),
            )
            .await?;
        }
        // Everyday spending
        if rng.chance(45) {
            let amt = rng.dollars(28, 120);
            txn(
                &ctx,
                &owner,
                checking,
                d,
                "SAMPLE GROCER #112",
                -amt.0,
                Some(c_groceries),
            )
            .await?;
        }
        if d.weekday().number_from_monday() == 6 && rng.chance(80) {
            let amt = rng.dollars(36, 64);
            txn(
                &ctx,
                &owner,
                card,
                d,
                "EXAMPLE FUEL 0881",
                -amt.0,
                Some(c_fuel),
            )
            .await?;
        }
        if d.weekday().number_from_monday() == 5 && rng.chance(70) {
            let amt = rng.dollars(22, 58);
            txn(&ctx, &owner, card, d, "CORNER CAFE", -amt.0, Some(c_dining)).await?;
        }
        if dom == 7 {
            let amt = rng.dollars(150, 225);
            warehouse_last = Some(
                txn(
                    &ctx,
                    &owner,
                    card,
                    d,
                    "DEMO WAREHOUSE CLUB",
                    -amt.0,
                    Some(c_household),
                )
                .await?,
            );
        }
        d += Duration::days(1);
    }

    // The review queue: recent charges nothing matched.
    for (days_ago, acct, desc, cents) in [
        (1, card, "SQ *UNKNOWN VENDOR 4821", -1_850),
        (2, checking, "FARM STAND", -3_400),
        (3, card, "HARDWARE STORE 220", -8_410),
        (5, vcard, "DEMO PHARMACY", -2_395),
    ] {
        let t = txn(
            &ctx,
            &owner,
            acct,
            today - Duration::days(days_ago),
            desc,
            cents,
            None,
        )
        .await?;
        uncategorized.push(t);
    }
    // One pending charge and one from the other side of the family's income.
    transactions::create_manual(
        &ctx,
        &owner,
        transactions::CreateManual {
            account_id: checking,
            date: today,
            description: "SAMPLE GROCER #112".into(),
            amount_cents: Cents::new(-8_640),
            category_id: Some(c_groceries),
            note: None,
            pending: Some(true),
            planned_item_id: None,
        },
    )
    .await?;
    txn(
        &ctx,
        &owner,
        checking,
        today - Duration::days(9),
        "MARKETPLACE PAYOUT (demo)",
        12_000,
        Some(c_other_income),
    )
    .await?;

    // Claude, in Driver mode for categorizing, took one off the queue and left an undo.
    let corner_market = txn(
        &ctx,
        &owner,
        checking,
        today - Duration::days(6),
        "CORNER MARKET",
        -2_375,
        None,
    )
    .await?;
    let args = json!({ "id": corner_market, "category_id": c_groceries });
    services::assistant::gate(
        &ctx,
        &claude,
        services::assistant::WriteIntent {
            area: AutonomyArea::CategorizingNotes,
            tool: "transactions.categorize".into(),
            args: args.clone(),
            summary: "Corner Market → Groceries".into(),
            entity: Some("transactions".into()),
            entity_id: Some(corner_market.to_string()),
            transaction_id: Some(corner_market),
            bill_id: None,
        },
        || async {
            let before = transactions::get(&ctx, corner_market).await?;
            let after =
                transactions::categorize(&ctx, &claude, corner_market, Some(c_groceries), None)
                    .await?;
            Ok((
                after.clone(),
                serde_json::to_value(&before).ok(),
                serde_json::to_value(&after).ok(),
                Some(questions::Proposal {
                    tool: "transactions.categorize".into(),
                    args: json!({ "id": corner_market, "category_id": null }),
                    summary: "Clear the category on Corner Market".into(),
                }),
            ))
        },
    )
    .await?;

    // ── Planned items ──
    let tires = planned::create(
        &ctx,
        &owner,
        planned::CreatePlanned {
            date: today + Duration::days(6),
            item: "New tires".into(),
            amount_cents: Cents::dollars(480),
            category_id: Some(c_household),
            decided_by: Some("Demo Owner".into()),
            note: Some("Quoted at the shop; pay from checking.".into()),
        },
    )
    .await?;
    let _ = tires;
    let gift = planned::create(
        &ctx,
        &owner,
        planned::CreatePlanned {
            date: today - Duration::days(16),
            item: "Birthday gift".into(),
            amount_cents: Cents::dollars(60),
            category_id: Some(c_household),
            decided_by: Some("Demo Member".into()),
            note: None,
        },
    )
    .await?;
    let gift_txn = txn(
        &ctx,
        &owner,
        card,
        today - Duration::days(15),
        "EXAMPLE TOY SHOP",
        -5_875,
        Some(c_household),
    )
    .await?;
    transactions::link_planned(&ctx, &owner, gift_txn, Some(gift.id)).await?;
    let concert = planned::create(
        &ctx,
        &owner,
        planned::CreatePlanned {
            date: today + Duration::days(20),
            item: "Concert tickets".into(),
            amount_cents: Cents::dollars(140),
            category_id: Some(c_dining),
            decided_by: Some("Demo Owner".into()),
            note: None,
        },
    )
    .await?;
    planned::set_status(
        &ctx,
        &owner,
        concert.id,
        PlannedStatus::Dropped,
        Some("Sold out.".into()),
    )
    .await?;

    // ── Sinking fund, debts, goal ──
    let fund = sinking::ensure_fund(&ctx).await?;
    sinking::set_balance(&ctx, &owner, fund.id, Cents::dollars(410), None).await?;

    let card_debt = debts::upsert(
        &ctx,
        &owner,
        debts::UpsertDebt {
            id: None,
            name: "Demo Credit Card".into(),
            kind: Some(DebtKind::CreditCard),
            balance_cents: Cents::new(324_018),
            apr_bps: 2_499,
            min_payment_cents: Cents::dollars(85),
            planned_payment_cents: Some(Cents::dollars(250)),
            due_day: 21,
            account_id: Some(card),
            custom_order: Some(1),
            note: None,
        },
    )
    .await?;
    let auto_loan = debts::upsert(
        &ctx,
        &owner,
        debts::UpsertDebt {
            id: None,
            name: "Demo Auto Loan".into(),
            kind: Some(DebtKind::Loan),
            balance_cents: Cents::new(890_000),
            apr_bps: 640,
            min_payment_cents: Cents::dollars(265),
            planned_payment_cents: None,
            due_day: 10,
            account_id: None,
            custom_order: Some(2),
            note: Some("36 payments left at the minimum.".into()),
        },
    )
    .await?;
    debts::set_strategy(
        &ctx,
        &owner,
        DebtStrategy {
            kind: DebtStrategyKind::Avalanche,
            extra_payment_cents: Cents::dollars(100),
            custom_order: vec![card_debt.id, auto_loan.id],
        },
    )
    .await?;

    let goal = goals::create(
        &ctx,
        &owner,
        goals::CreateGoal {
            name: "Emergency fund".into(),
            target_cents: Cents::dollars(5_000),
            target_date: Some(month.add_months(10).first_day()),
            account_id: Some(savings),
            category_id: Some(c_emergency),
            saved_cents: Some(Cents::dollars(2_150)),
            note: Some("Three months of bills.".into()),
        },
    )
    .await?;
    let _ = goal;

    // ── Balances ──
    let two_days_ago = Utc::now() - Duration::days(2);
    accounts::set_reported_balance(
        &ctx,
        &owner,
        checking,
        Cents::new(421_055),
        Some(two_days_ago),
    )
    .await?;
    accounts::set_reported_balance(
        &ctx,
        &owner,
        savings,
        Cents::dollars(6_800),
        Some(two_days_ago),
    )
    .await?;
    accounts::set_reported_balance(&ctx, &owner, card, Cents::new(-324_018), Some(two_days_ago))
        .await?;
    accounts::set_reported_balance(&ctx, &owner, vcard, Cents::new(0), Some(two_days_ago)).await?;

    // ── Alerts, a question, a proposal, and a pass summary ──
    let mut pace = AlertCandidate {
        alert_type: AlertType::CategoryPace,
        severity: Severity::Warn,
        title: "Dining out is ahead of plan".into(),
        detail: "Spent 68% of the plan with 55% of the month gone.".into(),
        amount_cents: None,
        dedupe_key: format!("pace:{c_dining}:{month}"),
        transaction_id: None,
        category_id: Some(c_dining),
        bill_id: None,
        debt_id: None,
        account_id: None,
        planned_item_id: None,
        goal_id: None,
        context: json!({ "seed": true }),
    };
    pace.context = json!({ "seed": true, "ratio_pct": 68 });
    let large = AlertCandidate {
        alert_type: AlertType::UnplannedLargeCharge,
        severity: Severity::Warn,
        title: "Large charge: Demo Warehouse Club".into(),
        detail: "No planned item matches this charge.".into(),
        amount_cents: Some(Cents::new(-21_212)),
        dedupe_key: format!(
            "large:{}",
            warehouse_last.map(|u| u.to_string()).unwrap_or_default()
        ),
        transaction_id: warehouse_last,
        category_id: Some(c_household),
        bill_id: None,
        debt_id: None,
        account_id: Some(card),
        planned_item_id: None,
        goal_id: None,
        context: json!({ "seed": true }),
    };
    services::alerts::upsert_candidates(&ctx, "seed", &[pace, large]).await?;

    questions::ask(
        &ctx,
        &claude,
        questions::AskQuestion {
            question: "Was the $84.10 at Hardware Store for the house or for the rental?".into(),
            options: Some(json!([
                { "key": "household", "label": "Household" },
                { "key": "rental", "label": "Rental (new category)" }
            ])),
            transaction_id: uncategorized.get(2).copied(),
            alert_id: None,
            planned_item_id: None,
            bill_id: None,
            context: None,
            notify: Some(false),
        },
    )
    .await?;
    questions::propose(
        &ctx,
        &claude,
        questions::ProposeInput {
            area: crate::db::models::AutonomyArea::PlansBills,
            proposal: questions::Proposal {
                tool: "budget.set_plan".into(),
                args: json!({ "category_id": c_groceries, "amount_cents": 90_000 }),
                summary: "Groceries plan $800 → $900".into(),
            },
            question: "Groceries has run over plan three months in a row. Raise the plan to $900 from this month?".into(),
            transaction_id: None,
            bill_id: None,
            context: Some(json!({ "months_over": 3 })),
        },
    )
    .await?;
    services::assistant::write_summary(
        &ctx,
        &claude,
        "Matched 5 bills and categorized Corner Market as Groceries. Four charges still need a category; I asked about the hardware store one. Proposed raising the Groceries plan to $900.".into(),
    )
    .await?;

    tracing::info!(
        owner = OWNER_EMAIL,
        member = MEMBER_EMAIL,
        password = PASSWORD,
        "demo household seeded"
    );
    Ok(())
}

/// The next occurrence of `day` on or after today.
fn next_due(today: NaiveDate, day: u32) -> NaiveDate {
    let m = Month::of(today);
    let this = m.date_with_day_clamped(day);
    if this >= today {
        this
    } else {
        m.next().date_with_day_clamped(day)
    }
}

async fn account(
    ctx: &Ctx,
    actor: &Actor,
    name: &str,
    t: AccountType,
    mask: &str,
) -> anyhow::Result<Uuid> {
    let a = accounts::create(
        ctx,
        actor,
        accounts::CreateAccount {
            name: name.into(),
            account_type: t,
            source: None,
            external_id: None,
            plaid_item_id: None,
            institution_label: Some("Demo Bank".into()),
            mask: Some(mask.into()),
            include_in_cash: None,
        },
    )
    .await?;
    Ok(a.id)
}

async fn group(ctx: &Ctx, actor: &Actor, name: &str, order: i32) -> anyhow::Result<Uuid> {
    Ok(budget::create_group(
        ctx,
        actor,
        budget::CreateGroup {
            name: name.into(),
            sort_order: Some(order),
        },
    )
    .await?
    .id)
}

async fn category(
    ctx: &Ctx,
    actor: &Actor,
    group_id: Uuid,
    name: &str,
    sign: CategorySign,
    everyday: bool,
    plan_cents: Option<i64>,
) -> anyhow::Result<Uuid> {
    Ok(budget::create_category(
        ctx,
        actor,
        budget::CreateCategory {
            group_id,
            name: name.into(),
            sign: Some(sign),
            note: None,
            is_everyday: Some(everyday),
            sort_order: None,
            plan_cents: plan_cents.map(Cents::new),
        },
    )
    .await?
    .id)
}

#[allow(clippy::too_many_arguments)]
async fn bill(
    ctx: &Ctx,
    actor: &Actor,
    name: &str,
    amount_cents: i64,
    cadence: i16,
    next_due: NaiveDate,
    category_id: Uuid,
    account_id: Uuid,
    pattern: Option<&str>,
) -> anyhow::Result<Uuid> {
    Ok(bills::add(
        ctx,
        actor,
        bills::AddBill {
            name: name.into(),
            amount_cents: Cents::new(amount_cents),
            cadence_months: Some(cadence),
            next_due,
            category_id: Some(category_id),
            account_id: Some(account_id),
            match_pattern: pattern.map(str::to_string),
            note: None,
        },
    )
    .await?
    .id)
}

async fn rule(ctx: &Ctx, actor: &Actor, pattern: &str, category_id: Uuid) -> anyhow::Result<()> {
    rules::create(
        ctx,
        actor,
        rules::CreateRule {
            name: None,
            pattern: pattern.into(),
            match_type: Some(RuleMatchType::Contains),
            priority: None,
            account_id: None,
            amount_min_cents: None,
            amount_max_cents: None,
            action: Some(RuleAction::Categorize),
            category_id: Some(category_id),
            set_note: None,
            apply_now: Some(false),
        },
    )
    .await?;
    Ok(())
}

async fn txn(
    ctx: &Ctx,
    actor: &Actor,
    account_id: Uuid,
    date: NaiveDate,
    description: &str,
    amount_cents: i64,
    category_id: Option<Uuid>,
) -> anyhow::Result<Uuid> {
    Ok(transactions::create_manual(
        ctx,
        actor,
        transactions::CreateManual {
            account_id,
            date,
            description: description.into(),
            amount_cents: Cents::new(amount_cents),
            category_id,
            note: None,
            pending: None,
            planned_item_id: None,
        },
    )
    .await?
    .id)
}
