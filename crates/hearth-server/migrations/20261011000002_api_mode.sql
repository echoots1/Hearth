-- API mode: chat threads with Claude, and an alert when the monthly cap is hit.

alter type alert_type add value if not exists 'api_spend_cap';

create table chat_threads (
  id            uuid primary key default gen_random_uuid(),
  household_id  uuid not null references households(id) on delete cascade,
  user_id       uuid references users(id) on delete set null,
  title         text not null default '',
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now()
);
create index chat_threads_household_idx on chat_threads(household_id, updated_at desc);

-- One row per Messages API message, in order; `content` is the API content
-- array verbatim (text, tool_use, tool_result, thinking) so a thread replays.
create table chat_messages (
  id          uuid primary key default gen_random_uuid(),
  thread_id   uuid not null references chat_threads(id) on delete cascade,
  role        text not null check (role in ('user', 'assistant')),
  content     jsonb not null,
  text        text not null default '',
  cost_cents  bigint not null default 0,
  created_at  timestamptz not null default now()
);
create index chat_messages_thread_idx on chat_messages(thread_id, created_at);
