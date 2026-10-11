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
answering "can we buy X?" with a straight yes, not now, or no, and doing the
**money planning**: when you say "we added a streaming service, $11.99 on the
14th" or "the car insurance renews in March at $640", the assistant records
the bill, puts it in the right category, sets the plan, and the app does the
rest. Everything can also be done by hand in the UI; chat is the preferred
front door, the UI is the fallback and the place you look.

Hearth also looks further out than the next paycheck: it forecasts **debt
payoff** under the standard strategies and shows what an extra payment or a
new charge does to the date and the interest; it projects **savings goals**
(when you'll reach a target at the current pace, what pace reaches it by a
date); and, in a later phase, it tracks **net worth and wealth building**
once the debt is gone. The app does the math and lays out the options; the
assistant turns them into advice.

Hearth is **not**: a multi-user SaaS, a bill-pay or money-movement tool, a
receipt scanner, or (in v1) an investment tracker or market data consumer. It runs on a home server in one Docker container, behind the
family's own reverse proxy, and nothing about the household leaves that box
except what the integrations the family turns on need.

## 2. Core concepts — proposed

Eleven nouns. Everything on screen and every MCP tool is made of these.

| Noun | One line | Lives on screen as |
| --- | --- | --- |
| **Account** | A place money sits: checking, savings, cash, card. Has a user-reported balance with an as-of time, and optionally a bank-reported one. | Cash card, More → Accounts |
| **Transaction** | One movement of money: date, description, signed amount (negative = out), source, category, pending flag, note. | Category detail, Activity |
| **Category** | A bucket with a sign (income/spend), a group, a standing note, flags (`everyday`, `due_day`), and a **plan** made of phases. | Budget list rows |
| **Plan phase** | "From month M, plan N per month." The current phase is the plan; the next phase is the "after the change" column. | Category detail → Plan |
| **Rule** | Pattern → category (or ignore). Runs on every new transaction; no match → review. | More → Rules, "make a rule" chips |
| **Recurring bill** | A commitment that repeats: name, amount, cadence (monthly / every N months / yearly), next due date, the category it posts to, the account or card it hits, a match pattern, status (active / canceled / paused). Monthly bills drive the category plan and the bills-due-before-payday number; non-monthly bills drive the sinking-fund set-aside; canceled bills that charge again raise an alert. | Plan tab, category subtitle ("due 14th"), low-point tile |
| **Planned item** | A one-off approved ahead of time so it isn't flagged when it posts. | Plan tab, low-point tile |
| **Sinking fund** | The money set aside for non-monthly bills: a funded balance with an as-of, and the derived should-have-saved from the yearly/quarterly bills. | Plan tab → Sinking fund |
| **Debt** | Balance, APR, minimum, due day, planned payment; payoff projection, interest per day, what-if deltas. | Goals tab → Debts |
| **Goal** | A savings target: name, target amount, optional target date, the account it accrues in, the monthly contribution (a category plan). Projection: date reached at current pace, or pace needed for the date. Debt-free is an implicit goal. | Goals tab → Savings |
| **Alert / Question** | Something that needs a human: raised by a job (alert) or by the AI (question). A question may carry a **proposal**: a structured change ("set Groceries plan to $900", "switch to avalanche") that applying approves in one tap. Lifecycle open → notified → ack → done. | Activity inbox, attention pills |

Derived, never stored: running estimate, projected low point, category
rollups, pace, suggested category plans (sum of a category's bills), the
sinking-fund should-have, debt projections and strategy comparisons, goal
projections, each bill's next due date after a match. Later phase: net worth
series from account and debt balance snapshots.

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
7. **Getting out of debt and saving.** The app keeps the debt and savings
   math current: payoff date and total interest per debt under avalanche,
   snowball, and your custom order side by side; what one extra $N a month
   does; what putting a purchase on a card costs in days and dollars; when
   each savings goal lands at today's pace. The assistant uses those numbers
   to make the actual suggestions ("move the $120 freed by the paid-off card
   onto the 24% one; debt-free four months sooner, $310 less interest") and
   records the decision as a plan change the humans can see.
8. **Planning happens in chat first.** "Add the gym, $45 on the 3rd" →
   the AI creates the recurring bill, picks or creates the category, raises
   the plan if the bills in it now exceed it, and adds a match pattern so
   the charge categorizes itself. "Cancel the streaming service" → the bill
   is marked canceled and any later charge is an alert. "Insurance renews
   in March, $640" → a yearly bill, and the sinking fund's set-aside goes
   up by $640/12. The UI shows the result and offers the same edits by
   hand.

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
| process | after sync, 5 min | apply rules, match recurring bills and planned items, advance due dates, queue review | guess a category |
| evaluate | 15 min | raise/refresh alerts by dedupe key; note debt/goal milestones (a debt paid off, a goal reached, payoff date slipped) | resolve or ack |
| notify | 1 min | send, nag, respect quiet hours | interpret replies |
| housekeeping | daily | prune runs, sessions, old deliveries | touch money data |

Alert catalogue (each with a dedupe key and default severity): unplanned
large charge, canceled bill charged, bill amount changed (a recurring bill
posts for noticeably more than its amount), bill not seen (past due, no
matching charge), category blown, category pace, cash floor breached, debt
redraw, card declined, planned item not seen, stale balance, sync failing,
review backlog. Category notes may carry three
literal tokens jobs honor: `[no-alerts]`, `[no-large-charge]`, `[no-pace]`.

## 8. AI / MCP surface — proposed

Endpoint `/mcp` (Streamable HTTP), bearer API key created in Settings with
`read` or `read_write` scope. Tools are thin wrappers over the same services
the UI uses, named `<noun>.<verb>`, money as `*_cents`, dates ISO.

Reads: budget month view, category, balance overview (estimate + low point
with components), transactions and review queue, recurring bills (upcoming,
by category, canceled), planned, sinking, debts (+ projection under each strategy, strategy
comparison, what-if for an extra payment or a new charge), goals (+
projection), alerts, questions, settings, "today".
Writes: categorize, set note, create manual transaction, apply rules,
create rule, set plan phase, **add / update / cancel / pause a recurring
bill**, create/update planned item, set reported balance, set sinking-fund
balance, upsert debt, set debt strategy and extra payment, create/update a
goal, ack/resolve/snooze alert, ask/answer/dismiss question,
allow-listed settings.

Planning tools are designed for one-sentence requests: `bills.add` takes
name, amount, cadence, next due, and optional category/account/pattern, and
returns what it created plus any plan it raised, so the assistant can
confirm in one line. When a category is ambiguous the assistant asks (a
question), it does not guess.

**Assistant autonomy is a household setting with two positions.**

- **Backseat** (suggest): the assistant's write tools don't change data
  directly; each call becomes a *proposal* in the Activity inbox with the
  exact change spelled out and an Approve button. Reads are unrestricted.
- **Driver** (act): write tools apply immediately. Every change shows in
  Activity as "Claude did …" with the before/after and, where it makes
  sense, an Undo. Nothing is hidden; the audit log is the record.

The switch is global with per-area overrides, so you can let the assistant
drive categorizing and notes while keeping plan amounts, bills, and debt
strategy on backseat (the proposed default). Changing the setting is itself
a UI-only action; the assistant cannot promote itself. Either way, "can we
buy X?" answers are reads and always work.

Forecasting tools return the numbers for every option, not a pick:
`debts.compare` gives payoff month and total interest for avalanche,
snowball, and custom; `debts.what_if` takes an extra monthly amount or a
one-time charge and returns the delta in months and dollars; `goals.project`
returns the reach date at current pace and the pace for a target date. The
assistant recommends; in Driver mode it may apply the strategy change, and
the change is visible and undoable in Activity.

"Can we buy X?" = `balance.get` (cash check) → `budget.get_month` (category
check) → `planned.list` / `debts.delay_cost` (plan check) → YES / NOT NOW /
NO, optionally `planned.create` on YES.

## 9. UI — ✓ decided (concept)

See `docs/UI.md`. Home is the Budget: cash card, attention pills, totals,
grouped category rows. Activity is the single inbox for alerts, AI
questions, and uncategorized transactions. **Plan** is a timeline of what's
coming: recurring bills by due date, planned one-offs, and the sinking fund
for the yearly ones, with add/edit by hand. **Goals** holds debts and
savings: each debt with balance, APR, interest per day and payoff date; a
strategy comparison; a what-if control for extra payment; savings goals with
reach dates. Accounts, rules, settings, users, API keys, integrations and
jobs live under More. Later phase: a net-worth view on the Goals tab. Warm stone neutrals, one typeface, orange =
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
| 6 | Is a category's plan set by hand, or derived from its bills? | **Decided:** the plan is a number you (or the AI) set and can edit at any time; the app shows "bills in this category total $X" and the AI may raise it when a new bill exceeds it. Never silently lowers it. |
| 7 | One bill → one category, or can a bill split? | **Decided:** one category by default; a bill can be split across categories only when a person asks for it (by chat or in the form), never on the assistant's own initiative. |
| 8 | Do yearly bills live in a category too? | Yes: they post to their category in the month they hit; the set-aside shows as the Sinking fund row. |
| 9 | Who suggests, who acts? | **Decided:** the app computes every option's numbers; the assistant recommends. Whether it may also *apply* a change is the Backseat / Driver setting in §8, per area, default Driver for categorizing and notes, Backseat for plans, bills, and strategy. |
| 11 | Undo | Proposed: changes made by the assistant in Driver mode get an Undo in Activity for categorize, note, plan amount, and strategy; adding a bill is reversed by canceling it. Undo is a new audited write, not a rollback. |
| 10 | Wealth building scope | Phase 2: net worth from balance snapshots, investment accounts with manually reported balances, contribution tracking. No brokerage sync or market data in scope yet. |
