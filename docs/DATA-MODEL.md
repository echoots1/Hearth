# Hearth — Data Model

PostgreSQL 16, managed by sqlx with SQL migrations in
`crates/hearth-server/migrations/` (embedded in the binary and run at
startup). This document is the source of truth for the schema; the Rust row
types in `crates/hearth-server/src/db/` must match it, and a change here is a
migration.

Conventions (see ARCHITECTURE §5):

- Primary keys are `id uuid default gen_random_uuid()`.
- Money columns end in `_cents` and are `bigint`, mapping directly to the
  Rust `Cents(i64)` newtype. Never `numeric`, never `float`.
- Signed amounts: negative = out, positive = in.
- Calendar dates are `date` columns (`YYYY-MM-DD` strings at the boundary),
  in the household timezone. Months are `char(7)` `YYYY-MM` columns named
  `*_month`. Instants are `timestamptz` columns named `*_at`.
- Every table has `created_at timestamptz default now()`; mutable tables also
  have `updated_at` maintained by the service layer.
- Enums are Postgres enums (listed in §2) so bad values can't be stored; each maps to a Rust enum with `#[sqlx(type_name = ...)]`.
- Soft delete (`deleted_at`) only where history matters: transactions.
- `household_id` appears on top-level tables so nothing is hard-coded, but
  v1 has exactly one household and no row-level isolation.

## 1. Entity overview

```
households ─┬─ users ─── sessions
            ├─ api_keys
            ├─ settings (k/v)
            ├─ accounts ─┬─ transactions ─── (category, rule, planned_item, import_batch)
            │            ├─ balance_snapshots
            │            ├─ card_events
            │            └─ plaid_items (1 item : n accounts)
            ├─ category_groups ─── categories ─── category_plans (phases)
            ├─ rules
            ├─ planned_items
            ├─ sinking_funds ─── sinking_fund_items
            ├─ debts
            ├─ alerts ─── notifications
            ├─ questions
            ├─ import_batches
            ├─ job_runs
            └─ audit_log
```

## 2. Enums

| Enum | Values |
| --- | --- |
| `user_role` | `owner`, `member` |
| `api_key_scope` | `read`, `read_write` |
| `account_type` | `checking`, `savings`, `credit_card`, `virtual_card`, `cash`, `loan`, `other` |
| `account_source` | `plaid`, `privacy`, `manual` |
| `transaction_source` | `plaid`, `privacy`, `csv`, `manual` |
| `review_status` | `none`, `pending`, `resolved` |
| `category_sign` | `income`, `spend` |
| `rule_match_type` | `contains`, `regex` |
| `rule_action` | `categorize`, `flag_canceled`, `ignore` |
| `planned_status` | `planned`, `done`, `dropped` |
| `debt_kind` | `credit_card`, `loan`, `mortgage`, `other` |
| `alert_type` | `unplanned_large_charge`, `canceled_service_charged`, `category_blown`, `category_pace`, `floor_breached`, `debt_redraw`, `card_declined`, `planned_item_missing`, `balance_stale`, `sync_failed`, `review_queue_backlog` |
| `alert_severity` | `info`, `warn`, `critical` |
| `alert_status` | `open`, `notified`, `ack`, `done` |
| `notification_channel` | `ntfy`, `smtp` |
| `notification_status` | `sent`, `failed` |
| `question_status` | `open`, `answered`, `dismissed` |
| `card_event_result` | `declined`, `approved` |
| `job_run_status` | `running`, `ok`, `failed`, `skipped` |
| `actor_kind` | `user`, `api_key`, `job`, `system` |

## 3. Tables

### households

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| name | text | "Our household" |
| timezone | text | IANA, e.g. `America/Chicago`. All "today" math uses this. |
| currency | char(3) | `USD`. Display only; all math is in minor units. |
| created_at | timestamptz | |

### users

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| email | citext unique | |
| display_name | text | |
| password_hash | text | argon2id. Nullable later for OIDC-only users (OPEN-QUESTIONS Q8). |
| role | user_role | |
| created_at, updated_at | timestamptz | |

### sessions

| Column | Type | Notes |
| --- | --- | --- |
| id | text pk | 32 random bytes, base64url; the cookie value. |
| user_id | uuid fk → users (cascade) | |
| expires_at | timestamptz | sliding 30 days |
| created_at | timestamptz | |

Index: `(user_id)`, `(expires_at)` for housekeeping.

### api_keys

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| name | text | "Claude desktop" |
| key_prefix | text | first 8 chars after `hearth_`, shown in UI |
| key_hash | text unique | sha256 of the full key |
| scope | api_key_scope | |
| created_by | uuid fk → users | |
| last_used_at | timestamptz null | |
| revoked_at | timestamptz null | |
| created_at | timestamptz | |

### settings

Key/value with a typed registry in code (`services/settings.rs`, serde
structs validated on read and write). One row per key per household.

| Column | Type | Notes |
| --- | --- | --- |
| household_id | uuid fk | pk part |
| key | text | pk part |
| value | jsonb | |
| updated_at | timestamptz | |

Registered keys and defaults (the Rust struct is `hearth_core::settings::Settings`;
every field has a default so a fresh household works with no rows):

| Key | Default | Used by |
| --- | --- | --- |
| `pay_schedule` | `{ "kind": "semimonthly", "days": [1, 15] }` (also `weekly`/`biweekly` with `anchor`, `monthly` with `day`) | projected low point |
| `cash_floor_cents` | `50000` | floor_breached, "can we buy" |
| `large_charge_threshold_cents` | `10000` | unplanned_large_charge |
| `planned_match` | `{ "days": 5, "pct": 10, "min_cents": 500 }` | planned-item matcher |
| `pace_alert` | `{ "ratio_pct": 110, "from_day": 10 }` (integer percent, never a float) | category_pace |
| `stale_balance_days` | `7` | balance_stale |
| `review_backlog` | `{ "count": 10, "days": 3 }` | review_queue_backlog |
| `sync_failure_threshold` | `3` | sync_failed |
| `debt_strategy` | `{ "kind": "avalanche", "extra_payment_cents": 0, "custom_order": [] }` | debts |
| `notifications` | `{ "quiet_hours": { "start": "22:00", "end": "07:00" }, "quiet_hours_override_critical": true }` | notify job |
| `nag` | `{ "first_after_minutes": 60, "every_minutes": 240, "max_nudges": 6 }` | notify job |
| `csv_presets` | `[]` (server-only, not part of the core struct) | CSV import |

### accounts

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| name | text | user-facing, editable ("Demo Checking") |
| type | account_type | |
| source | account_source | |
| external_id | text null | Plaid `account_id` or Privacy card token |
| plaid_item_id | uuid fk null → plaid_items | |
| institution_label | text null | free text; from Plaid or user |
| mask | text null | last 4 |
| currency | char(3) | |
| is_active | bool | archived accounts keep their transactions |
| include_in_running_balance | bool | default true for checking/savings/cash, false otherwise |
| reported_balance_cents | bigint null | user-reported |
| reported_balance_as_of | timestamptz null | |
| bank_balance_cents | bigint null | from Plaid, possibly stale |
| bank_balance_as_of | timestamptz null | |
| created_at, updated_at | timestamptz | |

Unique: `(source, external_id)` where `external_id is not null`.

### balance_snapshots

History of both kinds of balance, so "what did we think the balance was on
the 3rd" is answerable and the running estimate can be back-tested.

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| account_id | uuid fk (cascade) | |
| kind | text check in (`reported`, `bank`) | |
| balance_cents | bigint | |
| as_of | timestamptz | |
| recorded_by | actor_kind + actor_id (see audit) | |
| created_at | timestamptz | |

Index: `(account_id, kind, as_of desc)`.

### plaid_items

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| item_id | text unique | Plaid item id |
| access_token_enc | text | AES-256-GCM, base64 (iv ‖ tag ‖ ciphertext) |
| institution_id | text null | |
| institution_name | text null | |
| sync_cursor | text null | `/transactions/sync` cursor |
| status | text | `ok`, `login_required`, `error` |
| last_error | text null | |
| last_synced_at | timestamptz null | |
| consecutive_failures | integer default 0 | drives `sync_failed` |
| created_at, updated_at | timestamptz | |

### category_groups

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| name | text | "Income", "Housing", "Everyday", "Debt", "Savings" |
| sort_order | integer | |
| created_at, updated_at | timestamptz | |

Unique: `(household_id, name)`.

### categories

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| group_id | uuid fk → category_groups | |
| name | text | |
| sign | category_sign | |
| note | text default '' | free text for the AI; jobs honor only `[no-alerts]`, `[no-large-charge]`, `[no-pace]` tokens |
| is_everyday | bool default false | included in the everyday-spend pace |
| due_day | smallint null (1–31) | fixed bill due day; drives bills-before-paycheck |
| sort_order | integer | |
| is_archived | bool default false | |
| created_at, updated_at | timestamptz | |

Unique: `(household_id, name)` where not archived.

### category_plans (plan phases)

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| category_id | uuid fk (cascade) | |
| starts_month | char(7) | `YYYY-MM`; the phase applies from this month until the next phase |
| amount_cents | bigint | ≥ 0; the plan for a month, unsigned (sign comes from the category) |
| label | text null | "after move", "new mortgage" |
| note | text null | |
| created_at, updated_at | timestamptz | |

Unique: `(category_id, starts_month)`.
Effective plan for month `M` = row with max `starts_month <= M`; none → 0.

### transactions

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| account_id | uuid fk → accounts | |
| date | date | posting date (or authorized date while pending), household TZ |
| description | text | cleaned/display name; editable |
| original_description | text | as received; never edited |
| amount_cents | bigint | signed |
| category_id | uuid fk null → categories | |
| source | transaction_source | |
| external_id | text null | Plaid `transaction_id`, Privacy transaction token |
| fingerprint | text null | sha256 for csv/manual dedup (ARCHITECTURE §6.2) |
| pending | bool default false | |
| note | text default '' | |
| planned_item_id | uuid fk null → planned_items | set by matcher or by hand |
| rule_id | uuid fk null → rules | which rule categorized it |
| review_status | review_status default `none` | `pending` = in review queue |
| import_batch_id | uuid fk null → import_batches | |
| shadow_of_transaction_id | uuid fk null → transactions | cross-source duplicate link (OPEN-QUESTIONS Q3); shadows are excluded from rollups and estimates |
| raw | jsonb null | provider payload (trimmed) for debugging |
| deleted_at | timestamptz null | soft delete (Plaid `removed`, import rollback) |
| created_at, updated_at | timestamptz | |

Indexes and constraints:

- unique `(source, external_id)` where `external_id is not null`
- unique `(account_id, fingerprint)` where `fingerprint is not null`
- `(household_id, date desc)`, `(category_id, date)`, `(review_status) where review_status = 'pending'`, `(planned_item_id)`
- check: `source in ('plaid','privacy') → external_id is not null`

Pending → posted: Plaid sends a new `transaction_id` for the posted row with
`pending_transaction_id` pointing at the old one. The sync job updates the
existing row in place (new `external_id`, `pending = false`, final amount
and date) rather than inserting, so category and notes survive. Privacy
uses a stable token per transaction and just changes status.

### rules

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| name | text | |
| priority | integer | lower runs first |
| match_type | rule_match_type | |
| pattern | text | `contains` is case-insensitive substring on normalized description; `regex` is JS regex with `i` flag, validated on save |
| account_id | uuid fk null | restrict to one account |
| amount_min_cents | bigint null | on `abs(amount)` |
| amount_max_cents | bigint null | |
| action | rule_action default `categorize` | `flag_canceled` raises `canceled_service_charged`; `ignore` marks the row reviewed with no category (e.g. transfers) |
| category_id | uuid fk null | required when action = categorize |
| set_note | text null | appended to transaction note on match |
| is_active | bool default true | |
| hit_count | integer default 0 | |
| last_hit_at | timestamptz null | |
| created_at, updated_at | timestamptz | |

### planned_items

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| date | date | expected date |
| item | text | |
| amount_cents | bigint | unsigned expected spend |
| category_id | uuid fk null | |
| status | planned_status | |
| decided_by | text | free text (a name, "both", "Claude") — not an fk so the AI can be recorded |
| note | text default '' | |
| matched_transaction_id | uuid fk null → transactions | |
| created_at, updated_at | timestamptz | |

Index: `(household_id, status, date)`.

### sinking_funds

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| name | text | "Annual bills" |
| account_id | uuid fk null | where the money sits, informational |
| category_id | uuid fk null | the budget category the monthly set-aside is planned under |
| funded_balance_cents | bigint default 0 | user-reported |
| balance_as_of | timestamptz null | |
| created_at, updated_at | timestamptz | |

### sinking_fund_items

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| fund_id | uuid fk (cascade) | |
| name | text | "Car registration" |
| amount_cents | bigint | |
| due_month | char(7) | next due month; advanced by `cycle_months` when a matching transaction posts or by hand |
| cycle_months | smallint null | 12, 6, 3, 1; null = one-off |
| category_id | uuid fk null | where the bill posts |
| note | text default '' | |
| created_at, updated_at | timestamptz | |

### debts

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| name | text | |
| kind | debt_kind | |
| balance_cents | bigint | current balance (positive = owed) |
| balance_as_of | timestamptz | |
| apr_bps | integer | 24.99% → 2499 |
| min_payment_cents | bigint | |
| planned_payment_cents | bigint null | what the plan actually pays; defaults to min |
| due_day | smallint (1–31) | |
| account_id | uuid fk null | linked Plaid account; enables `debt_redraw` and auto balance updates |
| custom_order | integer null | for `debt_strategy.kind = custom` |
| is_closed | bool default false | |
| note | text default '' | |
| created_at, updated_at | timestamptz | |

Debt balances also get `balance_snapshots`-style history: for simplicity the
`balance_snapshots` table takes `debt_id` as an alternative to `account_id`
(exactly one non-null; check constraint).

### alerts

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| type | alert_type | |
| severity | alert_severity | |
| title | text | one line, used as the notification title |
| detail | text | human-readable explanation with numbers |
| amount_cents | bigint null | the amount in question |
| source | text | job name that raised it |
| dedupe_key | text | see ARCHITECTURE §6.7 |
| status | alert_status default `open` | |
| resolution | text null | set on `done` |
| transaction_id | uuid fk null | |
| category_id | uuid fk null | |
| debt_id | uuid fk null | |
| account_id | uuid fk null | |
| planned_item_id | uuid fk null | |
| context | jsonb null | structured extra (e.g. plan, spent, pace) |
| first_seen_at | timestamptz | |
| last_seen_at | timestamptz | refreshed each evaluate run while the condition holds |
| notified_at | timestamptz null | |
| last_nudged_at | timestamptz null | |
| nudge_count | integer default 0 | |
| snoozed_until | timestamptz null | notify job skips until then |
| acked_by | text null | user display name or "Claude" or "ntfy" |
| acked_at | timestamptz null | |
| resolved_at | timestamptz null | |
| created_at, updated_at | timestamptz | |

Unique: `(household_id, dedupe_key)` where `status <> 'done'` — one live
alert per fact; a resolved one can be re-raised as a new row.

### notifications

Delivery log; one row per attempt.

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| alert_id | uuid fk null (cascade) | |
| question_id | uuid fk null (cascade) | |
| channel | notification_channel | |
| kind | text | `initial`, `nudge`, `question` |
| status | notification_status | |
| external_id | text null | ntfy message id / SMTP message id |
| error | text null | |
| sent_at | timestamptz | |

Check: exactly one of `alert_id`, `question_id`.

### questions

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| asked_by | text | "Claude", a user name, or "system" |
| question | text | |
| options | jsonb null | `[{ "key": "groceries", "label": "Groceries" }, …]`; free-text answers always allowed |
| transaction_id | uuid fk null | |
| alert_id | uuid fk null | |
| planned_item_id | uuid fk null | |
| context | jsonb null | anything the asker wants to remember |
| status | question_status default `open` | |
| answer | text null | |
| answered_by | text null | |
| answered_at | timestamptz null | |
| notify | bool default true | whether the notify job should push it |
| created_at, updated_at | timestamptz | |

Index: `(household_id, status, created_at)`.

### card_events

Privacy.com declines (and, optionally, approvals for audit) — not money
movement, so not transactions.

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| account_id | uuid fk → accounts (the virtual card) | |
| external_id | text unique | |
| occurred_at | timestamptz | |
| amount_cents | bigint | unsigned attempted amount |
| descriptor | text | merchant string |
| result | card_event_result | |
| reason | text null | provider decline reason |
| raw | jsonb null | |
| created_at | timestamptz | |

### import_batches

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| household_id | uuid fk | |
| account_id | uuid fk | |
| filename | text | |
| mapping | jsonb | the column mapping used |
| row_count | integer | |
| inserted_count | integer | |
| duplicate_count | integer | |
| created_by | uuid fk → users | |
| rolled_back_at | timestamptz null | soft-deletes every transaction in the batch |
| created_at | timestamptz | |

### job_runs

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid pk | |
| job | text | `sync:plaid`, `process`, … |
| status | job_run_status | |
| started_at | timestamptz | |
| finished_at | timestamptz null | |
| summary | jsonb null | counts: fetched, inserted, updated, matched, alerts_raised, … |
| error | text null | |
| triggered_by | text | `schedule`, `manual:<user>`, `mcp:<key prefix>` |

Index: `(job, started_at desc)`.

### audit_log

| Column | Type | Notes |
| --- | --- | --- |
| id | bigserial pk | |
| household_id | uuid fk | |
| actor_kind | actor_kind | |
| actor_id | text null | user id, api key id, or job name |
| action | text | `transaction.categorize`, `budget.set_plan`, `alert.ack`, … |
| entity | text | table name |
| entity_id | text | |
| before | jsonb null | |
| after | jsonb null | |
| at | timestamptz default now() | |

Index: `(entity, entity_id, at desc)`, `(at desc)`.

## 4. Derived values (never stored)

These are computed on read by `src/core` from the tables above. Storing them
would create a second source of truth that jobs would have to keep in sync.

| Value | Inputs | Definition (see ARCHITECTURE §6) |
| --- | --- | --- |
| effective plan | category_plans | phase with max `starts_month <= M` |
| category actual / spent / remaining | transactions (not deleted, not shadow), category | `actual = Σ amount` in month; `spent = −actual` for spend; `remaining = plan − spent` |
| left to assign | categories + plans | planned income − planned spend for the month |
| running estimate | accounts, transactions | `reported_balance + Σ amount where date > as_of_date or pending` |
| next paycheck | settings.pay_schedule, today | first schedule date ≥ today |
| projected low point | above + categories + planned_items | estimate − bills due − everyday pace × days − planned items |
| sinking set-aside / should-have / shortfall | sinking_fund_items, funds | §6.4 |
| debt interest/day, payoff schedule, delay cost | debts, settings.debt_strategy | §6.5 |
| alert conditions | everything | §6.7; the `evaluate` job materializes them into `alerts` rows because they need lifecycle state |

## 5. Seed data (obviously fake)

The seed creates one household ("Demo Household", `America/Chicago`), two
users (`owner@example.com` / `member@example.com`, password `demo-password`),
three accounts ("Demo Checking", "Demo Savings", "Demo Credit Card"),
groups Income / Housing / Everyday / Bills / Debt / Savings with ~15
categories, two plan phases for "Rent/Mortgage" (a change three months
out), ~90 days of generated transactions against "Sample Grocer", "Example
Fuel", "Demo Streaming" etc., a handful of rules, three planned items, one
sinking fund with three items, two debts, and two open alerts. Amounts are
round, plausible, and unrelated to any real person.

## 6. Migration policy

- Schema changes land as a new `migrations/<timestamp>_<name>.sql` plus the
  matching row-type change, in the same commit, with the docs updated.
- Additive changes are preferred; destructive changes need a data migration
  in the same file and a note in the PR.
- The binary runs pending migrations on start (`sqlx::migrate!`) before
  serving traffic.
