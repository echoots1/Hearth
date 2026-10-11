-- Hearth initial schema. Source of truth: docs/DATA-MODEL.md (mirrors docs/ARCHITECTURE.md §6).
-- Money: bigint cents (negative = out). Dates: date in household TZ. Months: char(7) YYYY-MM. Instants: timestamptz.

create extension if not exists citext;
create extension if not exists pgcrypto;

-- ───────────────────────────── enums ─────────────────────────────
create type user_role as enum ('owner', 'member');
create type api_key_scope as enum ('read', 'read_write');
create type account_type as enum ('checking', 'savings', 'credit_card', 'virtual_card', 'cash', 'loan', 'investment', 'other');
create type account_source as enum ('plaid', 'privacy', 'manual');
create type transaction_source as enum ('plaid', 'privacy', 'csv', 'manual');
create type review_status as enum ('none', 'pending', 'resolved');
create type category_sign as enum ('income', 'spend');
create type rule_match_type as enum ('contains', 'regex');
create type rule_action as enum ('categorize', 'ignore');
create type bill_status as enum ('active', 'paused', 'canceled');
create type planned_status as enum ('planned', 'done', 'dropped');
create type debt_kind as enum ('credit_card', 'loan', 'mortgage', 'other');
create type goal_status as enum ('active', 'reached', 'paused', 'dropped');
create type alert_type as enum (
  'unplanned_large_charge', 'canceled_bill_charged', 'bill_amount_changed', 'bill_not_seen',
  'category_blown', 'category_pace', 'floor_breached', 'debt_redraw', 'card_declined',
  'planned_item_missing', 'balance_stale', 'sync_failed', 'review_queue_backlog', 'milestone'
);
create type alert_severity as enum ('info', 'warn', 'critical');
create type alert_status as enum ('open', 'notified', 'ack', 'done');
create type notification_channel as enum ('webpush', 'ntfy', 'smtp');
create type notification_status as enum ('sent', 'failed');
create type question_kind as enum ('ask', 'proposal');
create type question_status as enum ('open', 'answered', 'approved', 'declined', 'dismissed');
create type autonomy_area as enum ('categorizing_notes', 'plans_bills', 'debts_goals');
create type card_event_result as enum ('declined', 'approved');
create type job_run_status as enum ('running', 'ok', 'failed', 'skipped');
create type actor_kind as enum ('user', 'api_key', 'assistant', 'job', 'system');

-- ───────────────────────────── household & auth ─────────────────────────────
create table households (
  id          uuid primary key default gen_random_uuid(),
  name        text not null,
  timezone    text not null default 'UTC',
  currency    char(3) not null default 'USD',
  created_at  timestamptz not null default now()
);

create table users (
  id            uuid primary key default gen_random_uuid(),
  household_id  uuid not null references households(id) on delete cascade,
  email         citext not null unique,
  display_name  text not null,
  password_hash text not null,
  role          user_role not null default 'member',
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now()
);

create table sessions (
  id          text primary key,
  user_id     uuid not null references users(id) on delete cascade,
  expires_at  timestamptz not null,
  created_at  timestamptz not null default now()
);
create index sessions_user_idx on sessions(user_id);
create index sessions_expires_idx on sessions(expires_at);

create table api_keys (
  id            uuid primary key default gen_random_uuid(),
  household_id  uuid not null references households(id) on delete cascade,
  name          text not null,
  key_prefix    text not null,
  key_hash      text not null unique,
  scope         api_key_scope not null default 'read',
  created_by    uuid references users(id) on delete set null,
  last_used_at  timestamptz,
  revoked_at    timestamptz,
  created_at    timestamptz not null default now()
);

create table settings (
  household_id  uuid not null references households(id) on delete cascade,
  key           text not null,
  value         jsonb not null,
  updated_at    timestamptz not null default now(),
  primary key (household_id, key)
);

-- Encrypted secrets (Anthropic API key, VAPID private key, ...). Values are AES-256-GCM, base64.
create table secrets (
  household_id  uuid not null references households(id) on delete cascade,
  name          text not null,
  value_enc     text not null,
  updated_at    timestamptz not null default now(),
  primary key (household_id, name)
);

-- ───────────────────────────── accounts ─────────────────────────────
create table plaid_items (
  id                    uuid primary key default gen_random_uuid(),
  household_id          uuid not null references households(id) on delete cascade,
  item_id               text not null unique,
  access_token_enc      text not null,
  institution_id        text,
  institution_name      text,
  sync_cursor           text,
  status                text not null default 'ok',
  last_error            text,
  last_synced_at        timestamptz,
  consecutive_failures  integer not null default 0,
  created_at            timestamptz not null default now(),
  updated_at            timestamptz not null default now()
);

create table accounts (
  id                      uuid primary key default gen_random_uuid(),
  household_id            uuid not null references households(id) on delete cascade,
  name                    text not null,
  type                    account_type not null,
  source                  account_source not null default 'manual',
  external_id             text,
  plaid_item_id           uuid references plaid_items(id) on delete set null,
  institution_label       text,
  mask                    text,
  currency                char(3) not null default 'USD',
  is_active               boolean not null default true,
  include_in_cash         boolean not null default true,
  reported_balance_cents  bigint,
  reported_balance_as_of  timestamptz,
  bank_balance_cents      bigint,
  bank_balance_as_of      timestamptz,
  created_at              timestamptz not null default now(),
  updated_at              timestamptz not null default now()
);
create unique index accounts_source_external_idx on accounts(source, external_id) where external_id is not null;
create index accounts_household_idx on accounts(household_id);

-- ───────────────────────────── budget ─────────────────────────────
create table category_groups (
  id            uuid primary key default gen_random_uuid(),
  household_id  uuid not null references households(id) on delete cascade,
  name          text not null,
  sort_order    integer not null default 0,
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now(),
  unique (household_id, name)
);

create table categories (
  id            uuid primary key default gen_random_uuid(),
  household_id  uuid not null references households(id) on delete cascade,
  group_id      uuid not null references category_groups(id) on delete restrict,
  name          text not null,
  sign          category_sign not null default 'spend',
  note          text not null default '',
  is_everyday   boolean not null default false,
  sort_order    integer not null default 0,
  is_archived   boolean not null default false,
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now()
);
create unique index categories_household_name_idx on categories(household_id, name) where not is_archived;
create index categories_group_idx on categories(group_id);

create table category_plans (
  id            uuid primary key default gen_random_uuid(),
  category_id   uuid not null references categories(id) on delete cascade,
  starts_month  char(7) not null check (starts_month ~ '^[0-9]{4}-(0[1-9]|1[0-2])$'),
  amount_cents  bigint not null check (amount_cents >= 0),
  label         text,
  note          text,
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now(),
  unique (category_id, starts_month)
);

-- ───────────────────────────── recurring bills ─────────────────────────────
create table recurring_bills (
  id                       uuid primary key default gen_random_uuid(),
  household_id             uuid not null references households(id) on delete cascade,
  name                     text not null,
  amount_cents             bigint not null check (amount_cents >= 0),
  cadence_months           smallint not null default 1 check (cadence_months >= 1),
  next_due                 date not null,
  category_id              uuid references categories(id) on delete set null,
  account_id               uuid references accounts(id) on delete set null,
  match_pattern            text,
  status                   bill_status not null default 'active',
  note                     text not null default '',
  last_matched_transaction uuid,   -- fk added after transactions exists
  last_matched_at          timestamptz,
  created_at               timestamptz not null default now(),
  updated_at               timestamptz not null default now()
);
create index recurring_bills_household_due_idx on recurring_bills(household_id, status, next_due);

-- Only when a person asks for a split; absent = whole amount to the bill's category.
create table bill_splits (
  id            uuid primary key default gen_random_uuid(),
  bill_id       uuid not null references recurring_bills(id) on delete cascade,
  category_id   uuid not null references categories(id) on delete cascade,
  amount_cents  bigint not null check (amount_cents >= 0),
  unique (bill_id, category_id)
);

-- ───────────────────────────── rules, imports, planned ─────────────────────────────
create table rules (
  id                uuid primary key default gen_random_uuid(),
  household_id      uuid not null references households(id) on delete cascade,
  name              text not null,
  priority          integer not null default 100,
  match_type        rule_match_type not null default 'contains',
  pattern           text not null,
  account_id        uuid references accounts(id) on delete cascade,
  amount_min_cents  bigint,
  amount_max_cents  bigint,
  action            rule_action not null default 'categorize',
  category_id       uuid references categories(id) on delete set null,
  set_note          text,
  is_active         boolean not null default true,
  hit_count         integer not null default 0,
  last_hit_at       timestamptz,
  created_at        timestamptz not null default now(),
  updated_at        timestamptz not null default now(),
  check (action <> 'categorize' or category_id is not null)
);
create index rules_household_priority_idx on rules(household_id, priority);

create table import_batches (
  id               uuid primary key default gen_random_uuid(),
  household_id     uuid not null references households(id) on delete cascade,
  account_id       uuid not null references accounts(id) on delete cascade,
  filename         text not null,
  mapping          jsonb not null,
  row_count        integer not null default 0,
  inserted_count   integer not null default 0,
  duplicate_count  integer not null default 0,
  created_by       uuid references users(id) on delete set null,
  rolled_back_at   timestamptz,
  created_at       timestamptz not null default now()
);

create table planned_items (
  id                      uuid primary key default gen_random_uuid(),
  household_id            uuid not null references households(id) on delete cascade,
  date                    date not null,
  item                    text not null,
  amount_cents            bigint not null check (amount_cents >= 0),
  category_id             uuid references categories(id) on delete set null,
  status                  planned_status not null default 'planned',
  decided_by              text not null default '',
  note                    text not null default '',
  matched_transaction_id  uuid,   -- fk added after transactions exists
  created_at              timestamptz not null default now(),
  updated_at              timestamptz not null default now()
);
create index planned_items_status_date_idx on planned_items(household_id, status, date);

-- ───────────────────────────── transactions ─────────────────────────────
create table transactions (
  id                        uuid primary key default gen_random_uuid(),
  household_id              uuid not null references households(id) on delete cascade,
  account_id                uuid not null references accounts(id) on delete cascade,
  date                      date not null,
  description               text not null,
  original_description      text not null,
  amount_cents              bigint not null,
  category_id               uuid references categories(id) on delete set null,
  source                    transaction_source not null,
  external_id               text,
  fingerprint               text,
  pending                   boolean not null default false,
  note                      text not null default '',
  planned_item_id           uuid references planned_items(id) on delete set null,
  recurring_bill_id         uuid references recurring_bills(id) on delete set null,
  bill_drift_cents          bigint,
  rule_id                   uuid references rules(id) on delete set null,
  review_status             review_status not null default 'none',
  import_batch_id           uuid references import_batches(id) on delete set null,
  shadow_of_transaction_id  uuid references transactions(id) on delete set null,
  raw                       jsonb,
  deleted_at                timestamptz,
  created_at                timestamptz not null default now(),
  updated_at                timestamptz not null default now(),
  check (source not in ('plaid', 'privacy') or external_id is not null)
);
create unique index transactions_source_external_idx on transactions(source, external_id) where external_id is not null;
create unique index transactions_account_fingerprint_idx on transactions(account_id, fingerprint) where fingerprint is not null;
create index transactions_household_date_idx on transactions(household_id, date desc);
create index transactions_category_date_idx on transactions(category_id, date);
create index transactions_review_pending_idx on transactions(household_id) where review_status = 'pending';
create index transactions_bill_idx on transactions(recurring_bill_id, date desc);
create index transactions_account_date_idx on transactions(account_id, date);

alter table planned_items
  add constraint planned_items_matched_transaction_fk
  foreign key (matched_transaction_id) references transactions(id) on delete set null;
alter table recurring_bills
  add constraint recurring_bills_last_matched_fk
  foreign key (last_matched_transaction) references transactions(id) on delete set null;

-- ───────────────────────────── sinking fund, debts, goals ─────────────────────────────
create table sinking_funds (
  id                    uuid primary key default gen_random_uuid(),
  household_id          uuid not null references households(id) on delete cascade,
  name                  text not null,
  account_id            uuid references accounts(id) on delete set null,
  category_id           uuid references categories(id) on delete set null,
  funded_balance_cents  bigint not null default 0,
  balance_as_of         timestamptz,
  created_at            timestamptz not null default now(),
  updated_at            timestamptz not null default now()
);

create table debts (
  id                     uuid primary key default gen_random_uuid(),
  household_id           uuid not null references households(id) on delete cascade,
  name                   text not null,
  kind                   debt_kind not null default 'other',
  balance_cents          bigint not null check (balance_cents >= 0),
  balance_as_of          timestamptz not null default now(),
  apr_bps                integer not null check (apr_bps >= 0),
  min_payment_cents      bigint not null check (min_payment_cents >= 0),
  planned_payment_cents  bigint check (planned_payment_cents is null or planned_payment_cents >= 0),
  due_day                smallint not null check (due_day between 1 and 31),
  account_id             uuid references accounts(id) on delete set null,
  custom_order           integer,
  is_closed              boolean not null default false,
  note                   text not null default '',
  last_payoff_month      char(7),
  created_at             timestamptz not null default now(),
  updated_at             timestamptz not null default now()
);

create table goals (
  id                   uuid primary key default gen_random_uuid(),
  household_id         uuid not null references households(id) on delete cascade,
  name                 text not null,
  target_cents         bigint not null check (target_cents > 0),
  target_date          date,
  account_id           uuid references accounts(id) on delete set null,
  category_id          uuid references categories(id) on delete set null,
  saved_cents          bigint not null default 0,
  saved_as_of          timestamptz,
  status               goal_status not null default 'active',
  note                 text not null default '',
  reached_at           timestamptz,
  created_at           timestamptz not null default now(),
  updated_at           timestamptz not null default now()
);

create table balance_snapshots (
  id             uuid primary key default gen_random_uuid(),
  account_id     uuid references accounts(id) on delete cascade,
  debt_id        uuid references debts(id) on delete cascade,
  goal_id        uuid references goals(id) on delete cascade,
  kind           text not null check (kind in ('reported', 'bank')),
  balance_cents  bigint not null,
  as_of          timestamptz not null,
  actor_kind     actor_kind not null default 'system',
  actor_id       text,
  created_at     timestamptz not null default now(),
  check ((account_id is not null)::int + (debt_id is not null)::int + (goal_id is not null)::int = 1)
);
create index balance_snapshots_account_idx on balance_snapshots(account_id, kind, as_of desc);
create index balance_snapshots_debt_idx on balance_snapshots(debt_id, as_of desc);
create index balance_snapshots_goal_idx on balance_snapshots(goal_id, as_of desc);

-- ───────────────────────────── alerts, questions, assistant ─────────────────────────────
create table alerts (
  id               uuid primary key default gen_random_uuid(),
  household_id     uuid not null references households(id) on delete cascade,
  type             alert_type not null,
  severity         alert_severity not null,
  title            text not null,
  detail           text not null,
  amount_cents     bigint,
  source           text not null,
  dedupe_key       text not null,
  status           alert_status not null default 'open',
  resolution       text,
  transaction_id   uuid references transactions(id) on delete set null,
  category_id      uuid references categories(id) on delete set null,
  bill_id          uuid references recurring_bills(id) on delete set null,
  debt_id          uuid references debts(id) on delete set null,
  goal_id          uuid references goals(id) on delete set null,
  account_id       uuid references accounts(id) on delete set null,
  planned_item_id  uuid references planned_items(id) on delete set null,
  context          jsonb,
  first_seen_at    timestamptz not null default now(),
  last_seen_at     timestamptz not null default now(),
  notified_at      timestamptz,
  last_nudged_at   timestamptz,
  nudge_count      integer not null default 0,
  snoozed_until    timestamptz,
  acked_by         text,
  acked_at         timestamptz,
  resolved_at      timestamptz,
  created_at       timestamptz not null default now(),
  updated_at       timestamptz not null default now()
);
create unique index alerts_live_dedupe_idx on alerts(household_id, dedupe_key) where status <> 'done';
create index alerts_status_idx on alerts(household_id, status, severity);

create table questions (
  id               uuid primary key default gen_random_uuid(),
  household_id     uuid not null references households(id) on delete cascade,
  kind             question_kind not null default 'ask',
  asked_by         text not null,
  question         text not null,
  options          jsonb,
  -- proposal: { "tool": "budget.set_plan", "args": {...}, "area": "plans_bills", "summary": "Groceries plan $800 → $900" }
  proposal         jsonb,
  area             autonomy_area,
  transaction_id   uuid references transactions(id) on delete set null,
  alert_id         uuid references alerts(id) on delete set null,
  planned_item_id  uuid references planned_items(id) on delete set null,
  bill_id          uuid references recurring_bills(id) on delete set null,
  context          jsonb,
  status           question_status not null default 'open',
  answer           text,
  answered_by      text,
  answered_at      timestamptz,
  applied_at       timestamptz,
  notify           boolean not null default true,
  created_at       timestamptz not null default now(),
  updated_at       timestamptz not null default now(),
  check (kind <> 'proposal' or (proposal is not null and area is not null))
);
create index questions_status_idx on questions(household_id, status, created_at);

-- Every change the assistant made directly (Driver mode), with its inverse.
create table assistant_actions (
  id            uuid primary key default gen_random_uuid(),
  household_id  uuid not null references households(id) on delete cascade,
  api_key_id    uuid references api_keys(id) on delete set null,
  area          autonomy_area not null,
  tool          text not null,
  args          jsonb not null,
  summary       text not null,
  entity        text,
  entity_id     text,
  before        jsonb,
  after         jsonb,
  undo          jsonb,         -- { "tool": ..., "args": ... } or null when not undoable
  undone_at     timestamptz,
  undone_by     text,
  created_at    timestamptz not null default now()
);
create index assistant_actions_idx on assistant_actions(household_id, created_at desc);

-- API-mode usage for the spend cap.
create table assistant_usage (
  id             uuid primary key default gen_random_uuid(),
  household_id   uuid not null references households(id) on delete cascade,
  at             timestamptz not null default now(),
  purpose        text not null,      -- chat | pass | ask
  model          text not null,
  input_tokens   integer not null default 0,
  output_tokens  integer not null default 0,
  cost_cents     bigint not null default 0
);
create index assistant_usage_month_idx on assistant_usage(household_id, at desc);

-- ───────────────────────────── notifications ─────────────────────────────
create table push_subscriptions (
  id            uuid primary key default gen_random_uuid(),
  user_id       uuid not null references users(id) on delete cascade,
  endpoint      text not null unique,
  p256dh        text not null,
  auth          text not null,
  user_agent    text,
  last_success  timestamptz,
  failures      integer not null default 0,
  created_at    timestamptz not null default now()
);

create table notifications (
  id           uuid primary key default gen_random_uuid(),
  alert_id     uuid references alerts(id) on delete cascade,
  question_id  uuid references questions(id) on delete cascade,
  channel      notification_channel not null,
  kind         text not null,              -- initial | nudge | question | proposal | summary
  status       notification_status not null,
  target       text,                       -- subscription id / topic / address (never secret)
  external_id  text,
  error        text,
  sent_at      timestamptz not null default now(),
  check ((alert_id is not null)::int + (question_id is not null)::int <= 1)
);
create index notifications_alert_idx on notifications(alert_id, sent_at desc);

create table card_events (
  id            uuid primary key default gen_random_uuid(),
  account_id    uuid not null references accounts(id) on delete cascade,
  external_id   text not null unique,
  occurred_at   timestamptz not null,
  amount_cents  bigint not null,
  descriptor    text not null,
  result        card_event_result not null,
  reason        text,
  raw           jsonb,
  created_at    timestamptz not null default now()
);
create index card_events_account_idx on card_events(account_id, occurred_at desc);

-- ───────────────────────────── jobs, audit ─────────────────────────────
create table job_runs (
  id            uuid primary key default gen_random_uuid(),
  job           text not null,
  status        job_run_status not null,
  started_at    timestamptz not null default now(),
  finished_at   timestamptz,
  summary       jsonb,
  error         text,
  triggered_by  text not null default 'schedule'
);
create index job_runs_job_started_idx on job_runs(job, started_at desc);

create table audit_log (
  id            bigserial primary key,
  household_id  uuid not null references households(id) on delete cascade,
  actor_kind    actor_kind not null,
  actor_id      text,
  action        text not null,
  entity        text not null,
  entity_id     text not null,
  before        jsonb,
  after         jsonb,
  at            timestamptz not null default now()
);
create index audit_log_entity_idx on audit_log(entity, entity_id, at desc);
create index audit_log_at_idx on audit_log(at desc);
