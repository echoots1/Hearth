# Hearth — Architecture

**Hearth — household money, on plan.**

This document describes how Hearth is put together: the stack, the process
model, the modules, the jobs, the integrations, the MCP surface, and the
money math. The entity-level detail lives in [DATA-MODEL.md](./DATA-MODEL.md).
Anything that would change the data model and is not yet decided lives in
[OPEN-QUESTIONS.md](./OPEN-QUESTIONS.md).

## 1. Goals and non-goals

Hearth replaces a budgeting spreadsheet plus a pile of hand-built automations.
It keeps the spreadsheet's shape (budget categories, transactions, rules,
planned items, sinking fund, debts, a running balance, alerts) and adds:

- deterministic jobs that pull transactions, categorize, dedup, evaluate the
  plan, and raise/nag alerts;
- an MCP server that exposes the whole model to an AI assistant so judgment
  calls (odd transactions, what an alert means, "can we buy X?") happen there,
  not in the app;
- a mobile-first web UI whose primary screen is the category view.

Non-goals (v1): multi-tenancy, investment tracking, bill pay / money movement,
receipt capture, forecasting beyond "next paycheck", budgeting for anything
other than a single household.

## 2. Stack

Decided (after review): **Rust backend, TypeScript UI**, one Docker image.
This is final; the trade-offs were weighed once and are not revisited.

| Concern | Choice | Why (briefly) |
| --- | --- | --- |
| Backend language | **Rust** (stable, 2024 edition) | Fast, small, single static binary; a home server runs it comfortably alongside everything else. Strong typing keeps money math honest. |
| HTTP | **Axum** on **Tokio** | Mainstream async stack; serves the JSON API, the MCP endpoint, and the built UI from one process. |
| Database | **PostgreSQL 16** via **sqlx** (runtime-checked queries, embedded SQL migrations) | Plain SQL in the repo, no ORM magic; migrations run at startup from the binary. |
| MCP | **rmcp** (official Rust MCP SDK), Streamable HTTP transport mounted in Axum, bearer API key | First-class connector for Claude, no sidecar process. |
| Plaid / Privacy.com | Hand-written **reqwest** clients with serde models (no official Rust SDKs) | Small, explicit surface: a handful of endpoints each. Each client is a trait so tests use a fake. |
| Jobs | Tokio tasks on intervals + Postgres advisory locks + a `job_runs` table | No external cron; safe if two containers ever run. |
| Notifications | ntfy (reqwest) and SMTP (**lettre**) behind a `Notifier` trait | Pluggable; both optional. |
| Auth | argon2id password hashes, DB-backed sessions in an httpOnly cookie (tower-cookies); API keys hashed with SHA-256; AES-256-GCM for secrets at rest | Small and auditable. OIDC later. |
| Validation | serde + validator on request bodies; `schemars` JSON Schemas for MCP tool inputs | One definition per boundary. |
| Money / time | `i64` cents everywhere; `chrono` + `chrono-tz` for household-timezone dates | Never floats; a `Cents` newtype makes mixing units a compile error. |
| UI | **Vite + React + TypeScript**, Tailwind CSS, TanStack Query | Mobile-first SPA built to static files and embedded into the binary with `rust-embed`, so the image is still one binary. |
| Tests | `cargo test` for the pure core (no DB); `sqlx::test` integration tests against Postgres; Vitest + Playwright smoke for the UI | Money math first. |
| Packaging | Multi-stage `Dockerfile` (node build → cargo build with `cargo-chef` layer caching → `debian:bookworm-slim` runtime) + `docker-compose.yml` (app + Postgres) | Home-server friendly, runs behind any reverse proxy. |

Everything integration-specific (Plaid, Privacy.com, ntfy, SMTP, OIDC) is
optional and enabled only when its env vars are present. `.env.example` lists
every setting with a comment; no secrets are committed.

## 3. Process model

One container, one Rust binary (`hearth`), one port (`PORT`, default `3000`):

```
                 ┌────────────────────────────── hearth (Axum on Tokio) ──────────────────────────────┐
  browser ──────►│  /            embedded SPA (rust-embed)                                              │
  browser ──────►│  /api/*       JSON API, session cookie                 ──┐                           │
  Claude (MCP) ─►│  /mcp         Streamable HTTP (rmcp), bearer API key    ├─► core services ─► sqlx ──►│──► Postgres
  ntfy/email ───►│  /api/alerts/:id/ack?token=…  signed ack links          │                           │
                 │  scheduler (tokio tasks) ─► jobs ───────────────────────┘                           │
                 └────────────────────────────────────────────────────────────────────────────────────┘
                            │                         │                       │
                            ▼                         ▼                       ▼
                        Plaid API               Privacy.com API        ntfy / SMTP
```

- **Core services** (`crates/hearth-server/src/services/*`) take a `PgPool`
  (or a transaction) and return typed data. The JSON API handlers, the MCP
  tools, and the jobs all call the same services. Nothing in a handler or a
  job writes SQL directly.
- **Pure math** lives in `crates/hearth-core` with no database or async
  dependencies, so it is trivially unit-tested and reusable.
- **The scheduler** is started from `main` after migrations; `JOBS_ENABLED=false`
  turns it off so a second container can run as a dedicated worker later.
- **Reverse proxy**: `X-Forwarded-*` headers are trusted only when
  `TRUST_PROXY=true`. Session cookies are `Secure` when `APP_URL` is https.
- Startup order: parse env → connect pool → run migrations → seed settings
  defaults → start scheduler → serve.

## 4. Repository layout

Cargo workspace plus a `web/` package.

```
.
├── Cargo.toml                 workspace
├── crates/
│   ├── hearth-core/           pure domain types and money math (no DB, no async)
│   │   └── src/
│   │       ├── money.rs       Cents newtype, formatting, rounding rules
│   │       ├── calendar.rs    household-TZ today, Month (YYYY-MM), pay schedules
│   │       ├── budget.rs      effective plan, rollups, category view
│   │       ├── balance.rs     running estimate, projected low point
│   │       ├── sinking.rs     set-aside, should-have, shortfall
│   │       ├── debts.rs       interest/day, payoff projection, delay cost
│   │       ├── rules.rs       rule matching, description normalization, fingerprint
│   │       ├── planned.rs     planned-item matcher
│   │       ├── alerts.rs      alert evaluation (snapshot in → alerts out), nag timing, quiet hours
│   │       └── csv.rs         CSV row parsing with a column mapping
│   └── hearth-server/         the binary
│       ├── migrations/        sqlx SQL migrations (embedded)
│       └── src/
│           ├── main.rs        env, pool, migrate, scheduler, router
│           ├── config.rs      typed env parsing
│           ├── db/            sqlx row types and repositories, one module per table group
│           ├── services/      budget, transactions, rules, planned, sinking, debts, balance, alerts, questions, accounts, settings, audit
│           ├── api/           Axum handlers: auth, budget, transactions, review, planned, sinking, debts, balance, alerts, questions, accounts, settings, import, plaid, jobs, health
│           ├── auth/          passwords, sessions, api keys, extractors
│           ├── mcp/           rmcp server, one file per tool group
│           ├── jobs/          scheduler, advisory lock, each job
│           ├── integrations/  plaid/, privacy/, notify/ (ntfy, smtp)
│           └── assets.rs      rust-embed of web/dist
├── web/                       Vite + React + TS + Tailwind SPA
│   └── src/ (routes: budget, transactions, review, planned, sinking, debts, alerts, settings, login, setup)
├── docs/                      ARCHITECTURE.md, DATA-MODEL.md, OPEN-QUESTIONS.md, MCP.md
├── Dockerfile, docker-compose.yml, .env.example
├── .github/workflows/ci.yml
├── README.md, CONTRIBUTING.md, LICENSE (MIT)
```

## 5. Conventions that everything obeys

- **Money is integer cents.** Column type `bigint`; Rust type `Cents(i64)`
  newtype in `hearth-core::money`; JSON fields named `*_cents` carry plain
  integers. Never a float, never a string with a decimal point past the HTTP
  boundary. Formatting to `$1,234.56` happens only in the UI and in
  notification text.
- **Sign convention:** negative = money out, positive = money in. Applies to
  transactions, and to "actual" on categories. A spend category's plan is a
  positive number; its `spent` is `-sum(amounts)`.
- **Dates:** calendar dates are ISO `YYYY-MM-DD` strings in the household's
  timezone (`households.timezone`, IANA) and `chrono::NaiveDate` in Rust.
  Months are `YYYY-MM` strings (`Month` newtype). Instants (created_at,
  as-of timestamps, nudge times) are `timestamptz` / `DateTime<Utc>`.
  "Today" is always computed in the household timezone, never the server's.
- **Single household.** There is a `households` row so that nothing is
  hard-coded, but every query assumes exactly one. Multi-tenant safety is out
  of scope.
- **Writes are audited.** Every mutating service call records an `audit_log`
  row with the actor (user, API key, or job) and before/after snapshots.
- **Jobs detect, the AI decides.** Jobs never categorize ambiguous
  transactions, never auto-resolve alerts, never change plan amounts. They
  create review-queue entries, alerts, and pending questions. All judgment
  flows through MCP (or a human in the UI).
- **Nothing personal in the repo.** Seed data is obviously fake ("Demo
  Checking", "Example Bank", "Sample Grocer").

## 6. Domain modules

### 6.1 Budget (categories, groups, plan phases)

- Categories belong to a group and have a `sign` (`income` | `spend`), a
  free-text `note` that the AI reads for standing instructions, flags that
  drive the math (`is_everyday`, `due_day`), and sort order.
- A plan is a list of **phases** per category: `(starts_month, amount_cents,
  label)`. The effective plan for month `M` is the phase with the greatest
  `starts_month <= M`. A category with no phase at or before `M` has plan 0.
- The category view for month `M` returns, per category: `plan`, `actual`
  (signed sum), `spent`/`received` (unsigned), `remaining = plan − spent` for
  spend categories, and the same for the **next phase change** if one exists
  after `M`, so "this month" and "after the change" sit side by side.
- Group and household totals: planned income, planned spend, "left to
  assign" (zero-based check), actual income, actual spend, net.

### 6.2 Transactions and rules

- A transaction always belongs to an account. CSV imports are mapped to an
  account by the user at import time.
- **Dedup** happens at insert time:
  1. If the source gives a stable id (`plaid`, `privacy`), the unique index on
     `(source, external_id)` wins.
  2. Otherwise (`csv`, `manual`), a `fingerprint` = sha256 of
     `account_id | date | amount_cents | normalized(description)` is computed
     and a unique index on `(account_id, fingerprint)` rejects duplicates.
  3. Cross-source dedup (a Privacy.com charge later appearing in the bank
     feed) is handled by `transfer`/`shadow` linking: see OPEN-QUESTIONS Q3.
- **Rules** are ordered by `priority`. Each has a `match_type`
  (`contains` | `regex`), a `pattern` applied to the normalized description,
  and optional constraints (`account_id`, `amount_min_cents`,
  `amount_max_cents`). First match sets `category_id` and records
  `rule_id` on the transaction. No match → `review_status = 'pending'` and
  the transaction appears in the review queue.
- Re-running rules is explicit (`transactions.apply_rules` MCP tool / UI
  button) and only touches uncategorized rows unless `force` is set.
- A user or the AI can create a rule *from* a transaction; the service builds
  the pattern from the normalized description.

### 6.3 Planned items

- `planned_items` are approved-ahead one-offs: date, item, amount, category,
  status (`planned` | `done` | `dropped`), who decided, note.
- The **matcher** (part of the sync job) links an incoming transaction to a
  planned item when category matches (or item has none), the date is within
  ±N days (default 5), and the amount is within a tolerance (default 10% or
  $5, whichever is larger). A match sets `planned_items.status = 'done'` and
  `transactions.planned_item_id`, and suppresses the "unplanned large charge"
  alert for that transaction.
- Unmatched planned items whose date has passed by more than N days raise a
  low-severity "planned item not seen" alert.

### 6.4 Sinking fund

- One or more `sinking_funds`, each with a `funded_balance_cents` and
  `balance_as_of` (user-reported, like the running balance) and an optional
  linked account.
- Items: name, amount, `due_month`, `cycle_months` (12 for annual, 6, 3, 1,
  or `null` for one-off), category (so the bill posts against the plan when
  it hits).
- Math (pure, tested):
  - `monthly_set_aside(item) = ceil(amount / cycle_months)`; one-offs spread
    over the months from creation to due month.
  - `should_have_saved(item, month) = amount × months_elapsed_in_cycle /
    cycle_months`, where the cycle starts the month after the previous due
    month.
  - `fund.should_have = Σ should_have_saved`; `fund.shortfall = max(0,
    should_have − funded_balance)`.
- The sinking-fund monthly set-aside is surfaced as a read-only "suggested
  plan" for the category the fund is tied to; it does not write plan phases.

### 6.5 Debts

- Fields: balance, `balance_as_of`, APR in basis points (`apr_bps`,
  integer; 24.99% = 2499), minimum payment, due day, planned payment,
  optional linked account, kind, note.
- Household-level strategy: `avalanche` | `snowball` | `custom` order, plus
  `extra_payment_cents` applied to the first debt in strategy order.
- Math (pure, tested):
  - `interest_per_day = round(balance × apr_bps / 10000 / 365)`.
  - Payoff projection simulates month by month: interest accrues at
    `apr_bps/12`, payment applies, overflow rolls to the next debt in order.
    Output: payoff month per debt, total interest, and the schedule.
  - What-if helpers: `delay_cost(debt, amount_cents, days)` returns
    `{ extra_interest_cents, payoff_delay_days }` so "this delays payoff by X
    days / costs $Y" is a single call.

### 6.6 Running balance and projected low point

- Each account that `include_in_running_balance` has a user-reported
  `reported_balance_cents` + `reported_balance_as_of` (timestamp), and a
  possibly-stale `bank_balance_cents` + `bank_balance_as_of` from Plaid.
- `running_estimate(account) = reported_balance + Σ amount of transactions
  where (date > as_of_date) or pending`, with `as_of_date` the household-TZ
  date of `reported_balance_as_of`. Rationale and the edge case are in
  OPEN-QUESTIONS Q4.
- Household estimate = Σ over included accounts (credit cards are normally
  excluded; their effect shows up when the payment posts from checking).
- **Projected low point to next paycheck:**
  ```
  low_point = running_estimate
            − bills_due_before_paycheck
            − everyday_expected_spend
            − planned_items_before_paycheck
  ```
  - `next_paycheck` comes from the household `pay_schedule` setting
    (`weekly`/`biweekly` with an anchor date, `semimonthly` with two days, or
    `monthly` with one day).
  - `bills_due_before_paycheck` = Σ over spend categories with a `due_day`
    between today and the paycheck date of `max(0, plan − spent_this_month)`.
  - `everyday_expected_spend` = Σ over `is_everyday` categories of
    `pace × days_until_paycheck`, where `pace = max(spent_this_month /
    days_elapsed, plan / days_in_month)` — never lower than the plan's own
    daily rate so a quiet first week doesn't hide the month's real cost.
  - `planned_items_before_paycheck` = Σ `planned` items dated before the
    paycheck that have no matched transaction.
- The UI shows: estimate, as-of age ("reported 3 days ago"), bank balance
  and its age, next paycheck date, projected low point, and the four
  components so the number is explainable.

### 6.7 Alerts

Alerts are created only by jobs and only from deterministic rules. Each
alert type has a **dedupe key** so re-running a job doesn't duplicate it;
re-raising a done alert with the same key creates a new one only if the
underlying fact changed (e.g. the amount).

| Type | Trigger | Dedupe key | Default severity |
| --- | --- | --- | --- |
| `unplanned_large_charge` | spend transaction with `abs(amount) >= large_charge_threshold` and no matched planned item and category not flagged `note: "no large-charge alerts"` | `txn:<id>` | warn |
| `canceled_service_charged` | transaction matches a `canceled_merchants` rule (rule with `action = 'flag_canceled'`) | `txn:<id>` | critical |
| `category_blown` | `spent > plan` for a spend category in the current month | `cat:<id>:<month>` | warn |
| `category_pace` | projected month-end spend (at current pace) > plan × 1.1 and day-of-month ≥ 10 | `cat:<id>:<month>` | info |
| `floor_breached` | projected low point < `cash_floor_cents` | `floor:<month>` (amount refreshes detail) | critical |
| `debt_redraw` | a debt's balance (via linked account) increased by more than its minimum payment since last sync | `debt:<id>:<as_of>` | warn |
| `card_declined` | Privacy.com decline event | `decline:<external_id>` | warn |
| `planned_item_missing` | planned item overdue by > N days with no match | `planned:<id>` | info |
| `balance_stale` | reported balance older than `stale_balance_days` | `stale:<account_id>:<date>` | info |
| `sync_failed` | a source failed `sync_failure_threshold` runs in a row | `sync:<source>` | warn |
| `review_queue_backlog` | > N transactions pending review older than M days | `review:<date>` | info |

Lifecycle: `open → notified → ack → done`. `ack` means a human (or the AI
on their behalf) has seen it; `done` adds a `resolution` text. Acknowledging
never edits budget data; it only touches the alert row. Alerts can be acked
from the UI, via MCP, via the signed link in an email, or via an ntfy action
button that hits `/api/alerts/:id/ack?token=…` (a per-alert HMAC, no login
required).

Category `note` text is read by the AI, not by jobs — **except** for a tiny
set of documented directives that jobs honor literally so "don't flag this"
works without the AI in the loop: `[no-alerts]`, `[no-large-charge]`,
`[no-pace]`. They are matched as exact tokens.

### 6.8 Pending questions

`questions` is the channel for the AI to ask the humans something and record
the answer. A question has free text, optional structured `options`, an
optional subject (transaction / alert / planned item), and a status (`open`
| `answered` | `dismissed`). The AI creates them via MCP; the UI shows them
on the review screen and at the top of the budget screen when open; humans
answer in the UI or by replying to the notification. The notify job can
deliver open questions through the same channels as alerts, subject to quiet
hours, and the answer is readable through MCP. The job never interprets
answers.

## 7. Jobs

A single scheduler loop ticks every `SCHEDULER_TICK_SECONDS` (default 30)
and starts any job whose interval has elapsed. Each job:

1. takes a Postgres advisory lock keyed on the job name (skips if held);
2. inserts a `job_runs` row (`running`);
3. runs; writes a JSON `summary` and `ok`/`failed`;
4. releases the lock.

| Job | Default interval | Does |
| --- | --- | --- |
| `sync:plaid` | 60 min (`PLAID_SYNC_MINUTES`) | `/transactions/sync` with stored cursor per item; upserts transactions (pending → posted transitions update in place by `external_id`); refreshes `bank_balance_*`. |
| `sync:privacy` | 10 min (`PRIVACY_SYNC_MINUTES`) | Pulls transactions and declines since last watermark; upserts transactions for `settled`/`pending`, writes `card_events` for declines. |
| `process` | runs after any sync, and every 5 min | Applies rules to uncategorized rows, runs the planned-item matcher, queues unmatched rows for review, recomputes nothing (estimates are computed on read). |
| `evaluate` | 15 min (`EVALUATE_MINUTES`) | Evaluates every alert rule in §6.7 for the current month and upserts alerts by dedupe key. Pure functions over a snapshot; safe to re-run. |
| `notify` | 1 min | Sends `open` alerts (→ `notified`), re-nudges `notified` alerts per the nag schedule, sends open questions; respects quiet hours. |
| `housekeeping` | daily | Prunes `job_runs` older than 90 days, expired sessions, delivered notification rows older than 180 days. |

Jobs are deterministic: same DB state in → same alerts out. They never call
an LLM. Intervals are env-configurable; a job can be triggered on demand
from the settings page or via the `jobs.run` MCP tool.

## 8. Integrations

### Plaid
- Link flow: `/api/plaid/link-token` → Plaid Link in the browser →
  `/api/plaid/exchange` stores the `access_token` **encrypted at rest** with
  `APP_ENCRYPTION_KEY` (AES-256-GCM) in `plaid_items`. Accounts returned by
  Plaid are created as `accounts` with `source = 'plaid'`.
- Transactions via `/transactions/sync` with the per-item cursor;
  `added`/`modified`/`removed` are applied in order. Removed → soft-delete
  (`deleted_at`) so audit stays intact.
- Balances via the sync payload (not `/accounts/balance/get`, which costs
  money on paid tiers); stored as `bank_balance_*` and always labeled by
  age in the UI.
- Sandbox/development/production selected by `PLAID_ENV`.

### Privacy.com
- API key in `PRIVACY_API_KEY`; sandbox by `PRIVACY_ENV`.
- Each card becomes an `accounts` row (`type = 'virtual_card'`, `source =
  'privacy'`, `external_id = card token`, `include_in_running_balance =
  false` by default since the funding account is the real one).
- Approved transactions are inserted as transactions on the card account;
  declines go to `card_events` and raise `card_declined`.
- Cross-source shadow with the funding bank: see OPEN-QUESTIONS Q3.

### CSV import
- Upload → preview with a column mapping (date, description, amount or
  debit/credit columns, optional id) → commit. Presets for common layouts
  are stored in `settings.csv_presets`. Dedup by fingerprint (§6.2). Every
  import creates an `import_batches` row so a bad import can be rolled back
  (soft-delete every row in the batch).

### Notifications
- `Notifier` interface: `send(message: OutboundMessage): Promise<{ externalId? }>`.
  Implementations: `ntfy` (topic + optional auth token, action buttons for
  ack/done), `smtp` (nodemailer; ack via signed link). Both optional; zero
  configured channels means alerts stay `open` and the UI shows a banner.
- Quiet hours (`notifications.quiet_hours = { start: "22:00", end: "07:00" }`
  in household TZ): nothing is sent; `critical` alerts can override via
  `notifications.quiet_hours_override_critical`.
- Nag schedule: first nudge `nag.first_after_minutes` after `notified`, then
  every `nag.every_minutes` until `ack`, capped at `nag.max_nudges`. Each
  nudge increments `nudge_count` and sets `last_nudged_at`.

## 9. MCP server

- Endpoint: `/mcp` (rmcp Streamable HTTP service mounted in the Axum router;
  stateless sessions). Auth: `Authorization: Bearer <api key>` checked by an
  Axum middleware before the request reaches rmcp; keys are created in
  Settings, stored hashed, shown once.
- Tool naming: `<domain>.<verb>`; inputs are serde structs with `schemars`
  JSON Schemas; money fields are `*_cents` integers; dates are ISO strings;
  every list tool takes `limit`/`cursor`.
- Tools are thin wrappers over the server's services, so the UI and the AI
  can never disagree about a number.

Planned tool surface (full reference will be in `docs/MCP.md`):

| Group | Read | Write |
| --- | --- | --- |
| `budget` | `budget.get_month` (category view + totals + next phase), `budget.get_category`, `budget.list_phases` | `budget.set_plan` (phase upsert), `budget.set_category_note`, `budget.create_category`, `budget.update_category` |
| `transactions` | `transactions.list` (filters: month, category, account, uncategorized, pending, text), `transactions.get`, `transactions.review_queue` | `transactions.categorize`, `transactions.set_note`, `transactions.create_manual`, `transactions.apply_rules`, `transactions.link_planned` |
| `rules` | `rules.list` | `rules.create`, `rules.update`, `rules.delete`, `rules.create_from_transaction` |
| `planned` | `planned.list` | `planned.create`, `planned.update`, `planned.set_status` |
| `sinking` | `sinking.get` (items, set-aside, should-have, shortfall) | `sinking.upsert_item`, `sinking.set_balance` |
| `debts` | `debts.list`, `debts.projection`, `debts.delay_cost` | `debts.upsert`, `debts.set_balance` |
| `balance` | `balance.get` (per account + household estimate, ages, next paycheck, projected low point with components) | `balance.set_reported` |
| `alerts` | `alerts.list` (by status/severity/type), `alerts.get` | `alerts.ack`, `alerts.resolve`, `alerts.snooze` |
| `questions` | `questions.list` | `questions.ask`, `questions.answer`, `questions.dismiss` |
| `accounts` | `accounts.list` | `accounts.update` (name, include flag) |
| `jobs` | `jobs.status` (last runs per job) | `jobs.run` |
| `household` | `household.get_settings`, `household.today` (date, month, days left, next paycheck) | `household.update_settings` (allow-listed keys) |

**The "can we buy X?" procedure** maps onto three reads and no writes:

1. **Cash check** — `balance.get` → is `projected_low_point − X ≥ cash_floor`?
2. **Category check** — `budget.get_month` → does the category have
   `remaining ≥ X` (or, if the purchase spans months, the next phase)?
3. **Plan check** — `planned.list` for the item; `debts.delay_cost` if it
   would be financed; `sinking.get` if it is a known non-monthly bill.
4. Answer YES / NOT NOW / NO; on YES, optionally `planned.create` so the
   charge is pre-approved when it posts.

Each step is one tool call with a fully explained number, so a Claude skill
can implement the procedure bluntly and reproducibly.

## 10. Web UI

Mobile-first; every screen works one-handed on a phone and widens gracefully.

| Screen | Content |
| --- | --- |
| **Budget** (home) | Top card: running estimate, as-of age, next paycheck, projected low point (tap to expand components). Then groups → categories with plan / spent / left and a progress bar; income group first; "after <phase change>" toggle; open alerts and questions badge. Tap a category → its transactions for the month, its note, its phases. |
| **Transactions** | List for a month with filters; inline categorize; **Review queue** tab for `pending` review with one-tap categorize + "make a rule". CSV import entry point. |
| **Planned** | Upcoming and past planned items, status chips, add form. |
| **Sinking fund** | Items with due month, set-aside, should-have vs funded, shortfall; set balance. |
| **Debts** | Cards with balance, APR, min, due day, interest/day, payoff date; strategy picker; what-if box. |
| **Alerts** | Open / notified / acked / done tabs; ack + resolve with a note. |
| **Settings** | Household (timezone, pay schedule, cash floor, thresholds, quiet hours, nag), users, API keys, accounts (include in balance, reported balance), integrations status, jobs (last runs, run now), CSV presets. |

The SPA fetches from the JSON API with TanStack Query; the API calls the
same services as MCP. Routes are code-split so the budget screen loads first
and fast on a phone.

## 11. Auth and security

- First run: if no users exist, `/setup` creates the first user (owner).
- Local auth: email + password (argon2id). Sessions in `sessions` (random
  32-byte id, 30-day sliding expiry), cookie `hearth_session`, `HttpOnly`,
  `SameSite=Lax`, `Secure` on https.
- Roles: `owner` (manage users, API keys, integrations) and `member`.
- API keys: `hearth_<prefix>_<secret>`; only the sha256 hash is stored.
  Keys can be scoped `read` or `read_write`; MCP write tools require
  `read_write`.
- Secrets at rest (Plaid access tokens) are encrypted with
  `APP_ENCRYPTION_KEY`. Rotating the key requires a documented re-encrypt
  script.
- Ack links use a per-alert HMAC (`APP_SECRET`) with a 7-day expiry.
- Rate limiting on `/api/auth/login` and `/mcp` (tower `governor`-style
  in-memory token bucket; fine for one household).
- OIDC later: `oidc_subjects` table mapping provider+subject → user; the
  `users.password_hash` becomes nullable at that point (noted in
  OPEN-QUESTIONS Q8).

## 12. Configuration

All configuration is environment variables parsed once into a typed
`Config` struct in `config.rs`; the app refuses to start with a clear message
if something required is missing or malformed. Behavioral settings a user would change
from the UI (thresholds, quiet hours, pay schedule) live in the `settings`
table with env defaults used only to seed them on first run.

`.env.example` groups: app (`APP_URL`, `APP_SECRET`, `APP_ENCRYPTION_KEY`,
`TRUST_PROXY`, `LOG_LEVEL`), database (`DATABASE_URL`), jobs
(`JOBS_ENABLED`, intervals), Plaid (`PLAID_CLIENT_ID`, `PLAID_SECRET`,
`PLAID_ENV`, `PLAID_PRODUCTS`, `PLAID_COUNTRY_CODES`), Privacy
(`PRIVACY_API_KEY`, `PRIVACY_ENV`), ntfy (`NTFY_URL`, `NTFY_TOPIC`,
`NTFY_TOKEN`), SMTP (`SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`, `SMTP_PASS`,
`SMTP_FROM`, `ALERT_EMAIL_TO`), OIDC (reserved, unused in v1).

## 13. Build, run, deploy

- `Dockerfile` (multi-stage): `node:22-alpine` builds `web/dist`;
  `rust:1-bookworm` with `cargo-chef` builds the release binary with the SPA
  embedded; `debian:bookworm-slim` runtime with just the binary and CA
  certificates, running as a non-root user. The binary runs pending
  migrations on start.
- `docker-compose.yml`: `app` (built from `Dockerfile`, env from `.env`) and
  `db` (`postgres:16-alpine`, named volume). Healthcheck on `/api/health`.
- Local development: `docker compose up db`, `cargo run -p hearth-server`
  (serves the API and, in dev, proxies to or serves the Vite dev server),
  `npm --prefix web run dev`.
- `cargo run -p hearth-server -- seed` loads the demo household (fake
  accounts, ~3 months of fake transactions, categories, rules, planned items,
  a sinking fund, two debts, a couple of alerts).
- CI (GitHub Actions): `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test` with a Postgres service, `npm ci && npm run lint && npm run
  build` in `web/`, and `docker build`. No releases or tags until features
  are close; main always builds.

## 14. Testing strategy

- **Pure money math first** (`crates/hearth-core`, plain `#[test]`s, no DB):
  rollups and effective plan, running estimate, projected low point (each
  component separately and combined), debt interest/day, payoff projection
  and delay cost, sinking-fund set-aside and should-have, rule matching,
  dedup fingerprint, planned-item matching, alert evaluation (table-driven:
  snapshot in → alerts out), nag schedule timing, quiet hours, CSV parsing.
- **Service tests** (`#[sqlx::test]`, each test in its own fresh database)
  for dedup indexes, pending→posted upserts, CSV import rollback, alert
  dedupe keys, session expiry.
- **MCP contract test**: every tool's schema serializes and a smoke call
  against the seeded DB returns the same numbers as the service.
- **Integration clients**: Plaid and Privacy.com clients are traits with a
  recorded-fixture fake; the real clients are exercised only manually
  against sandbox.
- UI: Vitest for the small amount of client logic (money formatting, date
  helpers) and a few Playwright smoke flows (login, categorize from the
  review queue, ack an alert).

## 15. Delivery order (from the build prompt, unchanged)

1. Scaffold (Cargo workspace, Axum skeleton, sqlx, Vite SPA, Docker, env, CI, lint).
2. Data model + migrations.
3. Seed.
4. MCP server with the budget/transactions/balance/alerts/questions tools.
5. Category view (home screen) + transactions/review UI.
6. Jobs (scheduler, process, evaluate, notify-with-no-channels).
7. Plaid.
8. Privacy.com.
9. Notifications (ntfy, SMTP, quiet hours, nag).

The vertical slice after step 5 — manual transactions + categories + running
balance + MCP — is usable on its own and is the first thing to ship.
