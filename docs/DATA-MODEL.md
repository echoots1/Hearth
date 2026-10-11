# Hearth — Data Model

PostgreSQL 16, managed by sqlx with SQL migrations in
`crates/hearth-server/migrations/` (embedded in the binary, run on start).
This document is the source of truth for the schema; the Rust row types in
`crates/hearth-server/src/db/models.rs` must match it, and a change here is a
migration. The entity overview and the reasoning live in
`docs/ARCHITECTURE.md` §6.

Conventions:

- Primary keys are `id uuid default gen_random_uuid()`.
- Money columns end in `_cents` and are `bigint` (Rust `Cents(i64)`). Never
  `numeric`, never `float`. Signed amounts: negative = out, positive = in.
- Calendar dates are `date` in the household timezone. Months are `char(7)`
  `YYYY-MM` named `*_month`. Instants are `timestamptz` named `*_at`/`as_of`.
- Every table has `created_at`; mutable tables also `updated_at`, set by the
  service layer.
- Postgres enums (§1) map to Rust enums one to one.
- Soft delete (`deleted_at`) only on transactions.
- `household_id` on top-level tables; exactly one household in v1.

## 1. Enums

| Enum | Values |
| --- | --- |
| `user_role` | owner, member |
| `api_key_scope` | read, read_write |
| `account_type` | checking, savings, credit_card, virtual_card, cash, loan, investment, other |
| `account_source` | plaid, privacy, manual |
| `transaction_source` | plaid, privacy, csv, manual |
| `review_status` | none, pending, resolved |
| `category_sign` | income, spend |
| `rule_match_type` | contains, regex |
| `rule_action` | categorize, ignore |
| `bill_status` | active, paused, canceled |
| `planned_status` | planned, done, dropped |
| `debt_kind` | credit_card, loan, mortgage, other |
| `goal_status` | active, reached, paused, dropped |
| `alert_type` | unplanned_large_charge, canceled_bill_charged, bill_amount_changed, bill_not_seen, category_blown, category_pace, floor_breached, debt_redraw, card_declined, planned_item_missing, balance_stale, sync_failed, review_queue_backlog, milestone |
| `alert_severity` | info, warn, critical |
| `alert_status` | open, notified, ack, done |
| `notification_channel` | webpush, ntfy, smtp |
| `notification_status` | sent, failed |
| `question_kind` | ask, proposal |
| `question_status` | open, answered, approved, declined, dismissed |
| `autonomy_area` | categorizing_notes, plans_bills, debts_goals |
| `card_event_result` | declined, approved |
| `job_run_status` | running, ok, failed, skipped |
| `actor_kind` | user, api_key, assistant, job, system |

## 2. Tables

### Household and auth

**households** — `name`, `timezone` (IANA), `currency` (char 3). One row.

**users** — `household_id`, `email` (citext, unique), `display_name`,
`password_hash` (argon2id), `role`.

**sessions** — `id` (text, the cookie value), `user_id`, `expires_at`
(sliding 30 days).

**api_keys** — `name`, `key_prefix` (shown in UI), `key_hash` (sha256,
unique), `scope`, `created_by`, `last_used_at`, `revoked_at`.

**settings** — `(household_id, key)` → `value` jsonb. Keys and defaults are
the fields of `hearth_core::settings::Settings`: `pay_schedule`,
`cash_floor_cents`, `large_charge_threshold_cents`, `planned_match`,
`bill_match`, `pace_alert`, `stale_balance_days`, `review_backlog`,
`sync_failure_threshold`, `debt_strategy`, `notifications`, `nag`,
`assistant` (mode, autonomy + per-area overrides, models, monthly cap, pass
time); plus server-only `csv_presets`.

**secrets** — `(household_id, name)` → `value_enc` (AES-256-GCM, base64,
key from `APP_ENCRYPTION_KEY`). Names: `anthropic_api_key`,
`vapid_private_key`. Never returned by any API.

### Accounts

**plaid_items** — `item_id` (unique), `access_token_enc`, `institution_*`,
`sync_cursor`, `status`, `last_error`, `last_synced_at`,
`consecutive_failures`.

**accounts** — `name`, `type`, `source`, `external_id` (Plaid account id;
`privacy` is a legacy source: cards are no longer accounts, and migration
0005 or the sync removes the ones earlier versions made), `plaid_item_id`, `institution_label`, `mask`,
`currency`, `is_active`, `include_in_cash` (default true for checking /
savings / cash, false otherwise), `reported_balance_cents` +
`reported_balance_as_of` (user-reported), `bank_balance_cents` +
`bank_balance_as_of` (synced, possibly stale). Unique `(source,
external_id)` where not null.

**balance_snapshots** — history for an `account_id`, `debt_id` or `goal_id`
(exactly one): `kind` (reported | bank), `balance_cents`, `as_of`,
`actor_kind`, `actor_id`. Later the net-worth series.

### Budget

**category_groups** — `name` (unique per household), `sort_order`.

**categories** — `group_id`, `name` (unique among non-archived), `sign`,
`note` (free text for the assistant; jobs honor only `[no-alerts]`,
`[no-large-charge]`, `[no-pace]`), `is_everyday`, `plan_per_paycheck`,
`pay_schedule` (jsonb, same shape as the `pay_schedule` setting; null = the
household's; an income paid on its own rhythm), `sort_order`,
`is_archived`. No due day: bills carry due dates.

**category_plans** — `category_id`, `starts_month`, `amount_cents` (≥ 0,
unsigned), `label`, `note`. Unique `(category_id, starts_month)`. Effective
plan for month M = row with greatest `starts_month ≤ M`, else 0. When the
category has `plan_per_paycheck`, the amount is per paycheck and the month's
plan is `amount × paydays in M` under the category's own pay schedule, or
the household's when it has none (a biweekly schedule gives some months
three); the paychecks also land on the cash timeline on those paydays.
Switching `plan_per_paycheck` rescales every phase by the schedule's typical
paychecks a month (weekly 4, biweekly and semimonthly 2, monthly 1) so the
month's plan is unchanged; the audit log records it as
`budget.rescale_plans`.

### Recurring bills

**recurring_bills** — `name`, `amount_cents`, `cadence_months` (1 monthly,
3, 6, 12…), `per_paycheck` (due on every payday of the household pay
schedule, so three times in a three-paycheck month; `cadence_months` is
then ignored and `next_due` always sits on a payday; the API also spells it
`cadence_months: 0`), `next_due` (date), `category_id`, `account_id` (the
card or account it hits), `match_pattern` (substring on the normalized
description; null = name), `status`, `note`, `last_matched_transaction`,
`last_matched_at`. Monthly and per-paycheck bills feed category bill
totals (a per-paycheck bill at `amount × paydays in the month`) and the
bills-due-before-payday number; non-monthly bills feed the sinking fund. A
match advances `next_due` one cadence, or to the next payday.

**bill_splits** — `bill_id`, `category_id`, `amount_cents`. Present only
when a person asked for a split; otherwise the whole amount goes to the
bill's category.

### Rules, imports, planned items

**rules** — `name`, `priority` (lower first), `match_type`, `pattern`,
optional `account_id` / `amount_min_cents` / `amount_max_cents` (on
`abs(amount)`), `action` (categorize requires `category_id`; ignore marks
reviewed with no category), `set_note`, `is_active`, `hit_count`,
`last_hit_at`.

**import_batches** — `account_id`, `filename`, `mapping` jsonb, counts,
`created_by`, `rolled_back_at` (soft-deletes every row in the batch).

**planned_items** — `date`, `item`, `amount_cents` (unsigned),
`category_id`, `status`, `decided_by` (free text so the assistant can be
named), `note`, `matched_transaction_id`, `paid_outside_feed`. `done`
needs a matched transaction or `paid_outside_feed` (cash, another
account); otherwise the item stays planned, overdue, and on the cash
projection until the charge posts.

### Transactions

**transactions** — `account_id`, `date`, `description` (editable),
`original_description` (never edited), `amount_cents` (signed),
`category_id`, `source`, `external_id`, `fingerprint` (sha256 of account |
date | amount | normalized description, for csv/manual dedup), `pending`,
`note`, `planned_item_id`, `recurring_bill_id` + `bill_drift_cents`
(matched bill and `abs(amount) − bill.amount`), `rule_id`, `review_status`,
`import_batch_id`, `shadow_of_transaction_id` (the bank's `PwP` row
duplicating a Privacy.com card row on the same account; shadows are excluded
from rollups and alert evaluation, and from the cash estimate when the row
they shadow is on the same account), `raw` (for `privacy` rows: the card
token and label), `deleted_at`. Privacy rows live on the funding account.

Unique `(source, external_id)` where not null; unique `(account_id,
fingerprint)` where not null; check: plaid/privacy rows need an external id.
Pending → posted replacements update the existing row in place so category,
notes and links survive.

### Sinking fund, debts, goals

**sinking_funds** — `name`, `account_id`, `category_id` (the set-aside
row), `funded_balance_cents`, `balance_as_of`. Should-have-saved and
set-aside are derived from the non-monthly bills.

**debts** — `name`, `kind`, `balance_cents` + `balance_as_of`, `apr_bps`,
`min_payment_cents`, `planned_payment_cents`, `due_day`, `account_id`,
`custom_order`, `is_closed`, `note`, `last_payoff_month` (for slip
detection). Strategy and extra payment live in `settings.debt_strategy`.

**goals** — `name`, `target_cents`, `target_date`, `account_id`,
`category_id` (the monthly contribution), `saved_cents` + `saved_as_of`,
`status`, `note`, `reached_at`.

### Alerts, questions, assistant

**alerts** — `type`, `severity`, `title`, `detail`, `amount_cents`,
`source` (job), `dedupe_key`, `status`, `resolution`, refs
(`transaction_id`, `category_id`, `bill_id`, `debt_id`, `goal_id`,
`account_id`, `planned_item_id`), `context` jsonb, `first_seen_at`,
`last_seen_at`, `notified_at`, `last_nudged_at`, `nudge_count`,
`snoozed_until`, `acked_by`, `acked_at`, `resolved_at`. Unique live
`(household_id, dedupe_key)` where status ≠ done. `last_seen_at` keeps
advancing on a done alert while `evaluate` still sees its condition; a
candidate whose done alert the previous run saw is left done, one it did
not see is a new row.

**questions** — `kind` (ask | proposal), `asked_by`, `question`, `options`
jsonb, `proposal` jsonb (`{ tool, args, area, summary }`; required for
proposals), `area`, refs (`transaction_id`, `alert_id`, `planned_item_id`,
`bill_id`), `context`, `status`, `answer`, `answered_by`, `answered_at`,
`applied_at` (when an approved proposal ran), `notify`.

**assistant_actions** — Driver-mode changes: `api_key_id`, `area`, `tool`,
`args`, `summary`, `entity`, `entity_id`, `before`, `after`, `undo`
(`{ tool, args }` or null), `undone_at`, `undone_by`.

**assistant_usage** — API mode: `at`, `purpose` (chat | pass | ask),
`model`, `input_tokens`, `output_tokens`, `cost_cents`. Summed per month
against the cap.

### Notifications

**push_subscriptions** — `user_id`, `endpoint` (unique), `p256dh`, `auth`,
`user_agent`, `last_success`, `failures`.

**notifications** — delivery log: `alert_id` or `question_id` (or neither
for a summary), `channel`, `kind` (initial | nudge | question | proposal |
summary), `status`, `target` (never a secret), `external_id`, `error`,
`sent_at`.

**card_events** — Privacy.com declines (and optionally approvals):
`account_id` (the funding account), `external_id` (unique), `occurred_at`,
`amount_cents`, `descriptor`, `result`, `reason`, `card_token`, `card_label`
(which card: memo and last four), `raw`.

### Jobs and audit

**job_runs** — `job`, `status`, `started_at`, `finished_at`, `summary`
jsonb, `error`, `triggered_by`.

**audit_log** — `actor_kind`, `actor_id`, `action`, `entity`, `entity_id`,
`before`, `after`, `at`. Every write.

## 3. Derived, never stored

Category rollups and the month view; running estimate and projected low
point; each category's bill total and `bills_over_plan`; sinking-fund
set-aside, should-have and shortfall; debt projections, strategy comparison
and what-ifs; goal projections; pace; the assistant's work queue.

## 4. Seed data

Obviously fake: "Demo Household" in `America/Chicago`; `owner@example.com`
and `member@example.com` with password `demo-password`; "Demo Checking",
"Demo Savings", "Demo Credit Card", one virtual card; groups Income /
Housing / Everyday / Bills / Debt & savings with ~15 categories and one plan
phase change; ~8 recurring bills (one canceled, one yearly); ~90 days of
transactions against "Sample Grocer", "Example Fuel", "Demo Video";
three planned items; one sinking fund; two debts; one goal; two alerts, one
question and one proposal.

Load it with `hearth seed` (in Docker: `docker compose exec app hearth seed`).
It refuses to run on a database that already has users; `hearth seed --force`
wipes every table first and is for demo databases only. Everything goes
through the services layer, so the audit log, the assistant's Driver action
with its undo, and the Backseat proposal look exactly as real use produces
them. Dates are relative to the day it runs.

## 5. Migration policy

Schema changes land as a new `migrations/<timestamp>_<name>.sql` plus the
matching row-type change and this doc, in the same PR. Additive changes
preferred; destructive ones carry a data migration. The binary runs pending
migrations before serving traffic.
