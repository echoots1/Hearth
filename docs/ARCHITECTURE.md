# Hearth — Architecture

**Hearth — household money, on plan.**

> Status: **draft for review, section by section.** A section is decided when
> its header says `✓ decided`. Until then it is a proposal.

## 1. What Hearth is for — ✓ decided

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

## 2. Core concepts — ✓ decided

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

## 3. How a day works — ✓ decided

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

## 4. Integrations — ✓ decided

All optional, all behind environment variables; absent config = disabled.

### Data in

- **Plaid** (developer tier): Link flow, `/transactions/sync` with cursors,
  balances treated as possibly stale and always labeled by age. Access
  tokens encrypted at rest.
- **Privacy.com**: virtual cards as accounts, transactions and declines
  (declines become alerts, not transactions). Bank-feed duplicates of card
  charges are linked as shadows so the budget counts each charge once.
- **CSV import**: column mapping with saved presets, fingerprint dedup,
  per-import rollback.
- **Manual entry** in the UI and by chat.

### The app on your phone

Hearth is an **installable web app (PWA)**; there is no separate phone
build. The web UI ships a manifest and a service worker, so "Add to Home
Screen" on Android or iOS gives a full-screen app with its own icon and
splash, an offline shell, and **Web Push** notifications that arrive with
the app closed. Push uses VAPID keys the server generates on first run; no
Firebase project, Google account, or app store is involved. On Android the
browser routes delivery through its own push service; on iOS push works
once the app is on the home screen. HTTPS on `APP_URL` is required, which
the reverse proxy already provides.

A native client (APK or otherwise) is deliberately out of scope: one UI,
one notification path. The JSON API and MCP surface are there if someone
ever wants to build one.

### Notifications out

One `Notifier` interface, three channels, any combination on:

- **Web Push** (default, built in): rich notifications with action buttons
  (Acknowledge, Snooze, Open) handled by the service worker, which calls
  the API with a per-alert signed token so acknowledging never needs a
  login prompt.
- **ntfy**: for households already running it or wanting a desktop path;
  action buttons via ntfy actions.
- **Email (SMTP)**: digest-friendly; acknowledge via signed link.

Quiet hours and the nag schedule apply across channels; a critical alert can
override quiet hours. Every delivery is logged.

### Talking to the assistant

Over MCP from the Claude app (desktop or mobile): no extra channel to run.
A chat bridge (Telegram, Discord, SMS) is out of scope for v1; if wanted
later it is a separate small service that talks to the same MCP surface.

## 5. Stack and deployment — ✓ decided

- **Backend: Rust.** Axum on Tokio; sqlx on PostgreSQL 16 with embedded SQL
  migrations; `rmcp` for the MCP server; `reqwest` clients for Plaid and
  Privacy.com; Web Push (VAPID, RFC 8291) implemented directly on p256 and
  AES-GCM; `lettre` for SMTP. One static binary, small image, strong
  types around money.
- **UI: TypeScript.** Vite + React + Tailwind, mobile-first, built to static
  files and embedded in the binary. Ships as a PWA (manifest, service
  worker, Web Push); no native client.
- **Packaging:** one Docker image plus Postgres in `docker-compose.yml`;
  runs behind any reverse proxy; migrations run on start.
- **Conventions:** money is integer cents (`bigint` / `i64`, never a float;
  clippy denies float arithmetic); calendar dates are ISO strings in the
  household timezone; months are `YYYY-MM`; every write is audited with the
  actor (user, API key, or job).

## 6. Data model — ✓ decided

PostgreSQL. One household. Money `bigint` cents; dates `date` in the
household timezone; months `YYYY-MM`; instants `timestamptz`; uuid keys;
`created_at`/`updated_at` everywhere. Full column-level detail goes in
`docs/DATA-MODEL.md` once this section is decided.

| Table | Key fields | Notes |
| --- | --- | --- |
| `households` | name, timezone, currency | exactly one row |
| `users`, `sessions`, `api_keys` | email, argon2id hash, role; session id + expiry; key prefix + hash + scope | local auth; keys hashed, shown once |
| `settings` | key, jsonb value | typed in code; pay schedule, cash floor, thresholds, quiet hours, nag, assistant autonomy (global + per area) |
| `accounts` | name, type, source, external id, include_in_cash, reported balance + as-of, bank balance + as-of | cards excluded from cash by default |
| `balance_snapshots` | account or debt, kind (reported/bank), balance, as-of | history; later the net-worth series |
| `category_groups`, `categories` | name, sort; sign, group, note, is_everyday, sort, archived | no `due_day` on categories any more: bills carry due dates |
| `category_plans` | category, starts_month, amount, label | plan phases; effective = latest `starts_month ≤ month` |
| `recurring_bills` | name, amount, cadence (months: 1, 3, 6, 12…), next_due, category, account, match pattern, status (active/paused/canceled), note, last_matched_transaction | the new noun; yearly ones feed the sinking fund |
| `bill_splits` | bill, category, amount or percent | only when a person asks for a split |
| `rules` | priority, match type, pattern, constraints, action (categorize/ignore), category, set_note, hit stats | canceled-bill flagging moved to bills |
| `transactions` | account, date, description, original, amount, category, source, external id, fingerprint, pending, note, planned_item, recurring_bill, rule, review_status, import_batch, shadow_of, raw, deleted_at | unique (source, external id); unique (account, fingerprint); soft delete |
| `planned_items` | date, item, amount, category, status, decided_by, note, matched_transaction | one-offs |
| `sinking_funds` | name, account, funded balance + as-of | should-have derived from yearly/quarterly bills |
| `debts` | name, kind, balance + as-of, apr_bps, minimum, planned payment, due day, account, custom order, closed, note | strategy + extra payment live in settings |
| `goals` | name, target, target_date, account, category (the contribution), status | savings goals |
| `alerts` | type, severity, title, detail, amount, dedupe key, status, refs (transaction/category/bill/debt/account/planned), context, notified/nudged/acked/resolved times, snooze | unique live dedupe key |
| `questions` | asked_by, text, options, kind (ask/proposal), proposal jsonb (tool + args), status, answer, applied_at, refs | proposals approved here in Backseat mode |
| `assistant_actions` | tool, args, before, after, undo (tool + args), undone_at, actor key | the "Claude did…" feed with Undo |
| `push_subscriptions` | user, endpoint, keys, user agent, last success | Web Push targets |
| `notifications` | alert or question, channel (webpush/ntfy/smtp), kind, status, external id, error | delivery log |
| `import_batches`, `card_events` | mapping, counts, rolled_back_at; decline events from Privacy.com | |
| `plaid_items` | item id, encrypted access token, cursor, status, failures | |
| `job_runs`, `audit_log` | job, status, summary, error; actor, action, entity, before/after | every write audited |

Derived on read, never stored: category rollups and the month view; running
estimate and projected low point; each category's bill total and suggested
plan; sinking-fund set-aside and should-have; debt projections, strategy
comparison and what-ifs; goal projections; pace.

## 7. Jobs and automation — ✓ decided

In-process scheduler (no external cron). Each job takes a Postgres advisory
lock so two containers never run it at once, records a `job_runs` row with a
summary, and can be run on demand from Settings or the `jobs.run` MCP tool.
Intervals are env-configurable; the defaults:

| Job | Interval | May do | May not do |
| --- | --- | --- | --- |
| `sync:plaid` | 60 min | fetch transactions since cursor, upsert by external id (pending → posted in place), refresh bank balances, soft-delete removed rows | categorize, touch notes |
| `sync:privacy` | 10 min | fetch card transactions and declines, upsert, write decline events | categorize |
| `process` | after any sync, and every 5 min | apply rules to new rows; match rows to recurring bills (advance `next_due`, note amount drift) and planned items (mark done); shadow-link bank rows that duplicate card rows; queue the rest for review | guess a category, create a bill, change a plan |
| `evaluate` | 15 min | recompute month view and low point; raise or refresh alerts by dedupe key; note milestones (debt paid off, goal reached, payoff date slipped) | resolve, ack, or snooze an alert |
| `notify` | 1 min | send new alerts and open questions/proposals over the enabled channels; nag unacknowledged alerts on the schedule; honor quiet hours (critical may override) | interpret a reply |
| `housekeeping` | daily | prune old job runs, expired sessions, dead push subscriptions, delivered notification rows | touch money data |

**Alert catalogue.** Each has a dedupe key so re-running never duplicates
it, and a default severity:

| Alert | Fires when | Severity |
| --- | --- | --- |
| unplanned large charge | spend ≥ threshold, no planned item or bill match, category not opted out | warn |
| canceled bill charged | a charge matches a bill with status canceled | critical |
| bill amount changed | a bill matches but posts more than its amount by > tolerance | warn |
| bill not seen | a bill is past `next_due` by > N days with no match | info |
| category blown | spent > plan this month | warn |
| category pace | projected month-end > plan × ratio, after day N | info |
| cash floor breached | projected low point < cash floor | critical |
| debt redraw | a linked debt's balance rose by more than its minimum | warn |
| card declined | a Privacy.com decline event | warn |
| planned item not seen | a planned item is past date by > N days with no match | info |
| stale balance | a cash account's reported balance is older than N days | info |
| sync failing | a source has failed N runs in a row | warn |
| review backlog | > N uncategorized rows older than M days | info |
| milestone | a debt hits zero, a goal is reached, or a payoff date moves by > 1 month | info |

Category notes may carry three literal tokens the jobs honor: `[no-alerts]`,
`[no-large-charge]`, `[no-pace]`. Everything else in a note is for the
assistant.

**The line.** Jobs detect and compute; they never decide. No job
categorizes an ambiguous transaction, creates or edits a bill, changes a
plan, switches a debt strategy, or resolves an alert. Those are people, or
the assistant within its autonomy setting.

## 8. AI / MCP surface — ✓ decided

**Transport and auth.** Streamable HTTP at `/mcp`, bearer API key created in
Settings with `read` or `read_write` scope; keys are hashed, shown once,
revocable. Tools are thin wrappers over the same services the UI uses, so
the assistant and the screen never disagree about a number. Names are
`<noun>.<verb>`; money is `*_cents` integers; dates are ISO strings; list
tools paginate.

**Reads** (always allowed with any key):

| Group | Tools |
| --- | --- |
| household | `household.today` (date, month, days left, next payday, autonomy setting, assistant mode), `household.settings` |
| assistant | `assistant.work` (everything waiting for the assistant, in one call, with suggestions), `assistant.summary_write` (leave a short report of a pass that shows in Activity) |
| budget | `budget.month` (groups → categories: plan, spent, left, pace, next phase; totals), `budget.category` (detail + this month's transactions + bills in it) |
| balance | `balance.overview` (per-account estimate with ages, household cash, low point with its components, headroom over floor) |
| transactions | `transactions.list` (filters), `transactions.get`, `transactions.review_queue` |
| bills | `bills.list` (active / paused / canceled, upcoming before payday, by category), `bills.get` |
| planned | `planned.list` |
| sinking | `sinking.overview` (funded, should-have, set-aside, per bill) |
| debts | `debts.list` (+ interest/day), `debts.compare` (avalanche / snowball / custom: payoff month, total interest), `debts.what_if` (extra monthly amount, or a one-time charge → delta months and dollars) |
| goals | `goals.list`, `goals.project` (reach date at current pace; pace for a target date) |
| alerts | `alerts.list`, `alerts.get` |
| questions | `questions.list` (open questions and proposals, with answers) |
| rules, accounts | `rules.list`, `accounts.list` |
| jobs | `jobs.status` |

**Writes** (need `read_write`, and pass through the autonomy switch):

| Area | Tools |
| --- | --- |
| categorizing & notes | `transactions.categorize`, `transactions.set_note`, `transactions.link_planned`, `transactions.create_manual`, `transactions.apply_rules`, `rules.create`, `rules.update`, `rules.delete`, `budget.set_category_note`, `budget.create_category`, `budget.update_category` |
| plans & bills | `budget.set_plan` (phase upsert), `bills.add`, `bills.update`, `bills.cancel`, `bills.pause`, `bills.split` (only on a person's request), `planned.create`, `planned.update`, `planned.set_status`, `sinking.set_balance` |
| debts & goals | `debts.upsert`, `debts.set_balance`, `debts.set_strategy` (kind, order, extra payment), `goals.create`, `goals.update` |
| cash | `balance.set_reported` (per account; what the humans report in chat) |
| alerts & questions | `alerts.ack`, `alerts.resolve`, `alerts.snooze`, `questions.ask`, `questions.answer`, `questions.dismiss` |
| settings | `household.update_settings` (allow-listed keys; never the autonomy setting) |
| jobs | `jobs.run` |

**Assistant autonomy: Backseat / Driver.** A household setting with one
global position and overrides per area (categorizing & notes · plans & bills
· debts & goals). Ack/resolve/snooze and questions always apply directly;
they are the assistant acting as the humans' hands on the inbox.

- **Backseat (suggest):** a write tool does not change data. It records a
  *proposal* (a question of kind `proposal` carrying the tool and
  arguments) and returns it. The proposal appears in the Activity inbox
  and in notifications with the exact change spelled out; Approve runs
  it, Edit lets a person adjust the arguments first, Decline closes it.
- **Driver (act):** the write applies immediately and is recorded in
  `assistant_actions` with before, after, and the inverse call. Activity
  shows "Claude did …" with Undo where an inverse exists (categorize, note,
  plan amount, strategy, bill pause/cancel; adding a bill is undone by
  canceling it).
- The setting is UI-only; no tool can change it. `household.today` tells
  the assistant which position it is in so it can say "I've proposed…"
  versus "I've set…".
- Proposed default: Driver for categorizing & notes, Backseat for plans &
  bills and for debts & goals.

**Planning by sentence.** `bills.add` takes name, amount, cadence, next due,
and optional category, account, pattern; it returns what it created, the
category's new bill total, and whether the plan now falls short, so the
assistant can confirm in one line or follow with `budget.set_plan`. When the
category is ambiguous the assistant asks (a question), it does not guess.

**Forecasting.** The app computes every option; the assistant recommends.
`debts.compare` and `debts.what_if` return numbers, not a pick. A strategy
change goes through `debts.set_strategy`, which obeys the autonomy switch
like any other write.

**"Can we buy X?"** is three reads and a verdict:
1. `balance.overview` → is `low_point − X ≥ cash_floor`? (cash check)
2. `budget.month` → does the category have `left ≥ X`, or does the next
   phase cover it? (category check)
3. `planned.list` for the item, `debts.what_if` if it would go on a card,
   `bills.list` if it is really a recurring thing (plan check)
4. YES / NOT NOW / NO, optionally `planned.create` on YES so the charge is
   pre-approved when it posts. The verdict is recorded as an answered
   question so it is auditable.

Full tool reference with schemas will live in `docs/MCP.md` once the tools
exist.

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

## 10. Security and privacy — ✓ decided

- **Local auth.** Email + password (argon2id), DB-backed sessions in an
  httpOnly `SameSite=Lax` cookie, owner and member roles; the first visit
  creates the owner and must present the **setup token** the server printed
  to its log, so an instance reachable before setup cannot be claimed by a
  stranger. Writes from a cookie session are also checked against
  `Origin`/`Sec-Fetch-Site`. OIDC is a later option, not v1.
- **Roles.** Members read and tend the budget; only the owner manages
  people, keys, integrations, server and assistant settings, runs jobs by
  hand, changes household settings, and deletes bills, rules, plans or
  transactions.
- **API keys** for MCP: hashed at rest, shown once, `read` or `read_write`
  scope, revocable, last-used tracked.
- **Secrets at rest.** Plaid access tokens, the optional Anthropic API key,
  integration credentials and VAPID private keys are encrypted (AES-256-GCM)
  with a key derived by argon2id from `APP_ENCRYPTION_KEY` and a per-install
  salt; rotating it means re-entering the secrets. Nothing secret is ever
  returned by the API or MCP.
- **Spend control.** API mode has a monthly cap and per-call usage logging;
  the pass job stops when the cap is reached and raises an info alert.
- **Notification actions** use per-alert signed, expiring tokens, so a tap
  on a push button acknowledges without a login prompt and can't be replayed
  against another alert.
- **Rate limits** on login, setup and `/mcp`. Reverse-proxy headers trusted
  only when configured, and then only the last `X-Forwarded-For` hop.
- **Browser hardening.** Every response carries a strict Content Security
  Policy (self, plus Plaid Link's CDN), `frame-ancestors 'none'`, `nosniff`,
  `Referrer-Policy: no-referrer` and, over https, HSTS.
- **Untrusted text.** Descriptions, memos, notes and imported cells are data
  the assistant must not take instructions from; the MCP instructions say
  so, and delete tools always become proposals whatever the autonomy
  position. Push endpoints must be public push services, and Plaid webhooks
  must carry Plaid's signature.
- **Audit everything.** Every write records the actor (user, API key, job,
  assistant), before and after. The assistant's actions are additionally
  listed in Activity with Undo.
- **Nothing leaves the box** except what enabled integrations need: Plaid
  and Privacy.com calls, push deliveries, and (API mode only) the context
  sent to Claude, which is the household's budget data. The setup guide
  says this plainly next to the toggle.
- **Public repo hygiene.** No real names, balances, institutions or people
  anywhere in the repository; seed and test data are obviously fake;
  `.env.example` has every setting and no values.

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
| 8 | Do yearly bills live in a category too? | **Decided:** yes. A yearly/quarterly bill posts to its category in the month it hits; the saving for all of them shows as one "Sinking fund" row on the Budget, with the per-bill breakdown on the Plan tab. |
| 9 | Who suggests, who acts? | **Decided:** the app computes every option's numbers; the assistant recommends. Whether it may also *apply* a change is the Backseat / Driver setting in §8, per area, default Driver for categorizing and notes, Backseat for plans, bills, and strategy. |
| 12 | Phone app path | **Decided:** PWA + Web Push only; no APK or native client. Requires HTTPS on `APP_URL` (already assumed behind the reverse proxy). |
| 13 | Assistant mode | **Decided:** Pull mode (MCP + a recurring task in the Claude app) is the default and free; API mode is an opt-in toggle with the instance owner's own key and a spend cap. Setup guide documents both. |
| 11 | Undo | Proposed: changes made by the assistant in Driver mode get an Undo in Activity for categorize, note, plan amount, and strategy; adding a bill is reversed by canceling it. Undo is a new audited write, not a rollback. |
| 10 | Wealth building scope | Phase 2: net worth from balance snapshots, investment accounts with manually reported balances, contribution tracking. No brokerage sync or market data in scope yet. |
