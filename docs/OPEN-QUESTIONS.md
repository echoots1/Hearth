# Hearth — Open Questions

Questions that would change the data model or the money math. Each has a
proposed default so work can continue.

**Status (2026-10-10):** all proposed defaults below were accepted at the
design review. They are now the decisions; the alternatives stay listed so
the reasoning isn't lost. New questions go at the end with the same format.

## Q1. Category rollover

Should unspent plan in a spend category roll into the next month (YNAB
style) or reset (EveryDollar style)?

- **Proposed default:** reset. No `rollover` column; the category view is
  purely per-month. Adding rollover later means a `rollover_mode` on
  `categories` and a per-month carry computed from history, which is
  additive.

## Q2. Credit cards in the running balance

A credit-card purchase doesn't move cash until the statement is paid. The
spreadsheet model tracked spend by category regardless of account.

- **Proposed default:** card accounts have `include_in_running_balance =
  false`; their transactions count toward categories (so the budget is
  right) but not toward the cash estimate; the card *payment* from checking
  is categorized to a transfer/`ignore` rule so it isn't double counted.
  The projected low point therefore does not anticipate the next card
  payment unless it's a planned item or a `due_day` category.
- Alternative: treat the card's running balance as a bill due on its
  `due_day` and subtract it. Needs a per-card "statement balance" concept.

## Q3. Cross-source duplicates (Privacy.com ↔ bank feed)

A Privacy.com charge appears on the virtual card feed within minutes and
again in the funding bank's feed days later (as a "PRIVACY.COM" line). Both
are real rows from their own source, but only one should hit the budget.

- **Proposed default:** `transactions.shadow_of_transaction_id`. The
  `process` job links a bank-feed row whose description matches the
  Privacy funding descriptor and whose amount/date are within tolerance to
  the earlier card row, marks the bank row as a shadow, and shadows are
  excluded from rollups and the running estimate. The card row keeps the
  category. The card accounts are excluded from the running balance (Q2),
  so the *bank* row is the one that should affect cash; this means the
  running estimate must count the shadow bank row and the rollup must count
  the card row. That asymmetry is deliberate and the docs call it out, but
  it's worth confirming.
- Alternative: let the card row be the only row and drop the bank row on
  sync (loses the audit trail).

## Q4. Running estimate "since as-of" boundary

The reported balance has a timestamp; transactions have a date. A
transaction dated the same day as the as-of could be before or after the
reading.

- **Proposed default:** include transactions with `date > as_of_date` plus
  all pending ones; the UI tells the user to report an end-of-day balance.
  Off-by-one risk is one day's worth of posted transactions and is visible
  in the explained components.
- Alternative: include rows with `created_at > reported_balance_as_of`
  (first seen after the reading). More precise for synced sources, wrong for
  CSV backfills.

## Q5. Everyday pace definition

`pace = max(spent / days_elapsed, plan / days_in_month)` is proposed.

- Alternatives: actual pace only (can under-project early in the month);
  plan pace only (ignores a blowout). The max is pessimistic by design;
  confirm that's the intended bias for the low-point number.

## Q6. Pay schedule vs. income transactions

Next paycheck is derived from a `pay_schedule` setting rather than from
income transactions or an income category's `due_day`.

- **Proposed default:** settings-based, one schedule per household. Two
  earners with different schedules → add a second schedule later
  (`pay_schedules` array in the same setting; additive).

## Q7. Sinking fund as one fund or many

- **Proposed default:** the table supports many funds, the UI and seed show
  one. No change needed either way.

## Q8. OIDC later

- **Proposed default:** `users.password_hash` is NOT NULL in v1. When OIDC
  lands: make it nullable and add `oidc_subjects(provider, subject, user_id)`.
  Additive.

## Q9. Alert ack via ntfy reply

ntfy doesn't support free-text replies back to the app; it supports action
buttons (HTTP actions). Email replies require an inbound mail hook.

- **Proposed default:** ack/done via signed link (email) and action buttons
  (ntfy). "Reply to the notification" for *questions* means replying in the
  UI, via MCP, or via a signed one-tap option link; free-text answers by
  email reply are out of scope for v1.

## Q10. Debt payoff: minimum-payment behavior

Credit-card minimums typically scale with balance (e.g. 1% + interest, floor
$25); the model stores a fixed `min_payment_cents`.

- **Proposed default:** fixed minimum, user-updated, `planned_payment_cents`
  used for projections. A `min_payment_rule` jsonb can be added later
  without touching other tables.

## Q11. Month assignment for transactions

A transaction is assigned to the month of its `date`. Some households
assign by statement period or shift late-month purchases forward.

- **Proposed default:** calendar month of `date` in household TZ. Users can
  edit `date` (original is preserved in `raw`) if they need to move one.

## Q12. Money column width

- **Decided:** `bigint` cents, mapping directly to Rust `i64`. (The original
  proposal was `integer`; switching to a Rust backend made `bigint` the
  natural choice, and the cost is nil.)
