# Hearth — Architecture

**Hearth — household money, on plan.**

> Status: **draft for review, section by section.** A section is decided when
> its header says `✓ decided`. Until then it is a proposal.

## 1. What Hearth is for — proposed

Hearth is a self-hosted budget for one household. It keeps the family on a
monthly plan, keeps an honest running estimate of cash, and tells you how low
that cash will get before the next paycheck. It raises a flag when something
is off-plan and lets an AI assistant (Claude, over MCP) do the day-to-day
tending: categorizing the odd transaction, asking the humans a question,
answering "can we buy X?" with a straight yes, not now, or no.

Hearth is **not**: a multi-user SaaS, an investment tracker, a bill-pay or
money-movement tool, a receipt scanner, or a forecasting engine beyond the
next paycheck. It runs on a home server in one Docker container, behind the
family's own reverse proxy, and nothing about the household leaves that box
except what the integrations the family turns on need.

## 2. Core concepts — proposed

Nine nouns. Everything on screen and every MCP tool is made of these.

| Noun | One line | Lives on screen as |
| --- | --- | --- |
| **Account** | A place money sits: checking, savings, cash, card. Has a user-reported balance with an as-of time, and optionally a bank-reported one. | Cash card, More → Accounts |
| **Transaction** | One movement of money: date, description, signed amount (negative = out), source, category, pending flag, note. | Category detail, Activity |
| **Category** | A bucket with a sign (income/spend), a group, a standing note, flags (`everyday`, `due_day`), and a **plan** made of phases. | Budget list rows |
| **Plan phase** | "From month M, plan N per month." The current phase is the plan; the next phase is the "after the change" column. | Category detail → Plan |
| **Rule** | Pattern → category (or ignore / flag canceled). Runs on every new transaction; no match → review. | More → Rules, "make a rule" chips |
| **Planned item** | A one-off approved ahead of time so it isn't flagged when it posts. | Planned tab, low-point tile |
| **Sinking fund** | Non-monthly bills with due month and cycle; monthly set-aside vs funded balance. | More → Sinking fund, a Savings row |
| **Debt** | Balance, APR, minimum, due day; payoff projection and interest per day. | Debts tab |
| **Alert / Question** | Something that needs a human: raised by a job (alert) or by the AI (question). Lifecycle open → notified → ack → done. | Activity inbox, attention pills |

Derived, never stored: running estimate, projected low point, category
rollups, pace, sinking-fund should-have, debt projections.

## 3. How a day works — proposed

1. **Sync** jobs pull new transactions from each configured source (Plaid,
   Privacy.com) on an interval; CSV import and manual entry add the rest.
2. **Process** applies rules, dedups, matches planned items, and parks
   anything unmatched in the review queue.
3. **Evaluate** recomputes the month view and the low point and raises or
   refreshes alerts for anything off-plan. Deterministic: same data in, same
   alerts out.
4. **Notify** sends new alerts and the AI's open questions over ntfy/email,
   respects quiet hours, and nags unacknowledged alerts on a schedule.
5. **People** open the Budget tab for the numbers and the Activity inbox for
   anything that needs them. Acknowledging is a tap or a reply, never a data
   edit.
6. **The AI**, over MCP, reads the same model: it clears the review queue
   where it can, asks a question where it can't, records balances the
   humans report in chat, and answers "can we buy X?" by checking cash, the
   category, and the plan.

Jobs detect; people and the AI decide. A job never categorizes an ambiguous
transaction, never resolves an alert, never changes a plan.

## 4. Integrations — proposed

All optional, all behind environment variables; absent config = disabled.

- **Plaid** (developer tier): Link flow, `/transactions/sync` with cursors,
  balances treated as possibly stale and always labeled by age. Access
  tokens encrypted at rest.
- **Privacy.com**: virtual cards as accounts, transactions and declines
  (declines become alerts, not transactions). Bank-feed duplicates of card
  charges are linked as shadows so the budget counts each charge once.
- **CSV import**: column mapping with saved presets, fingerprint dedup,
  per-import rollback.
- **Notifications**: ntfy and SMTP behind one interface; ack via action
  button or signed link; quiet hours; nag schedule.
- **MCP**: first-class; see §8.

## 5. Stack and deployment — ✓ decided

- **Backend: Rust.** Axum on Tokio; sqlx on PostgreSQL 16 with embedded SQL
  migrations; `rmcp` for the MCP server; `reqwest` clients for Plaid and
  Privacy.com; `lettre` for SMTP. One static binary, small image, strong
  types around money.
- **UI: TypeScript.** Vite + React + Tailwind, mobile-first, built to static
  files and embedded in the binary.
- **Packaging:** one Docker image plus Postgres in `docker-compose.yml`;
  runs behind any reverse proxy; migrations run on start.
- **Conventions:** money is integer cents (`bigint` / `i64`, never a float;
  clippy denies float arithmetic); calendar dates are ISO strings in the
  household timezone; months are `YYYY-MM`; every write is audited with the
  actor (user, API key, or job).

## 6. Data model — proposed outline

Detailed in `docs/DATA-MODEL.md` once §2 is decided. Tables, one per noun in
§2 plus: `households`, `users`, `sessions`, `api_keys`, `settings` (k/v,
typed in code), `category_groups`, `balance_snapshots`, `import_batches`,
`card_events`, `notifications`, `job_runs`, `audit_log`. Dedup: unique
`(source, external_id)` for synced rows; unique `(account, fingerprint)` for
CSV and manual rows. Soft delete only on transactions.

## 7. Jobs and automation — proposed

In-process scheduler (no external cron), Postgres advisory lock per job,
`job_runs` history, run-now from Settings or MCP.

| Job | Interval | May do | May not do |
| --- | --- | --- | --- |
| sync:plaid / sync:privacy | 60 / 10 min | upsert transactions, balances, declines | categorize beyond rules |
| process | after sync, 5 min | apply rules, match planned items, queue review | guess a category |
| evaluate | 15 min | raise/refresh alerts by dedupe key | resolve or ack |
| notify | 1 min | send, nag, respect quiet hours | interpret replies |
| housekeeping | daily | prune runs, sessions, old deliveries | touch money data |

Alert catalogue (each with a dedupe key and default severity): unplanned
large charge, canceled service charged, category blown, category pace, cash
floor breached, debt redraw, card declined, planned item not seen, stale
balance, sync failing, review backlog. Category notes may carry three
literal tokens jobs honor: `[no-alerts]`, `[no-large-charge]`, `[no-pace]`.

## 8. AI / MCP surface — proposed

Endpoint `/mcp` (Streamable HTTP), bearer API key created in Settings with
`read` or `read_write` scope. Tools are thin wrappers over the same services
the UI uses, named `<noun>.<verb>`, money as `*_cents`, dates ISO.

Reads: budget month view, category, balance overview (estimate + low point
with components), transactions and review queue, planned, sinking, debts
(+ projection, delay cost), alerts, questions, settings, "today".
Writes: categorize, set note, create manual transaction, apply rules,
create rule, set plan phase, create/update planned item, set reported
balance, upsert debt, ack/resolve/snooze alert, ask/answer/dismiss question,
allow-listed settings.

"Can we buy X?" = `balance.get` (cash check) → `budget.get_month` (category
check) → `planned.list` / `debts.delay_cost` (plan check) → YES / NOT NOW /
NO, optionally `planned.create` on YES.

## 9. UI — ✓ decided (concept)

See `docs/UI.md`. Home is the Budget: cash card, attention pills, totals,
grouped category rows. Activity is the single inbox for alerts, AI
questions, and uncategorized transactions. Planned and Debts are their own
tabs; sinking fund, accounts, rules, settings, users, API keys, integrations
and jobs live under More. Warm stone neutrals, one typeface, orange =
attention, blue = income.

## 10. Security and privacy — proposed

Local auth (argon2id, DB-backed sessions, httpOnly cookie, owner/member
roles); first run creates the owner. API keys hashed, shown once. Plaid
tokens encrypted with an app key. Signed, expiring ack links. Rate limits on
login and MCP. OIDC later. The repo never contains real names, balances,
institutions or people; seed data is obviously fake.

## 11. Open questions

| # | Question | Proposed default |
| --- | --- | --- |
| 1 | Do card accounts count toward "cash now"? | No; their charges count toward categories, their payment is a transfer. |
| 2 | Privacy.com charge vs. the bank's later line for it | Shadow-link the bank row; budget counts the card row, cash counts the bank row. |
| 3 | Everyday pace for the low point | `max(actual pace, plan pace)`, pessimistic by design. |
| 4 | Unspent plan at month end | Resets (no rollover) in v1. |
| 5 | Where the AI's "can we buy" answer is logged | As a question with the answer recorded, so it's auditable. |
