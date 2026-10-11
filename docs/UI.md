# Hearth — UI

Approved concept (2026-10-11). Mobile-first; every screen works one-handed on
a phone and widens gracefully. Demo data only in mockups and seeds.

## Look

- Warm stone neutrals (`#F4F1EC` ground, white cards, `#1C1917` ink), one
  near-black "cash" card so the number that matters is the darkest thing on
  the screen.
- One typeface (Manrope or equivalent humanist sans), tabular numerals
  everywhere money appears.
- Orange (`#C2410C`) means **attention** (over plan, on a bad pace, due
  inside the low-point window, a critical alert). Blue (`#1D4ED8`) means
  **income**. No red/green coding.
- Big rounded cards, 44 px minimum touch targets, a floating bottom nav.

## Installable

The web UI is a PWA: add to home screen on Android or iOS for a full-screen
app with its own icon, splash, and push notifications. No separate native
app. Notifications carry Acknowledge / Snooze / Open actions.

## Navigation

Bottom nav, five items: **Budget** (home), **Activity**, **Plan**,
**Goals**, **More** (accounts, rules, settings, users, API keys,
integrations, jobs).

## Screens

### Budget (home)

1. Header: wordmark + tagline, settings button.
2. Month switcher with "day N of M · K days left".
3. **Cash card** (dark):
   - *Cash now*: household running estimate; subtitle shows the age of the
     reported balance and the bank's own number with its age. "Update"
     button records a fresh reported balance.
   - *Low point before <next payday>*: projected low point, days until
     payday, a pill with distance above/below the cash floor, and four
     tiles explaining the deductions: bills due (a bill past due with no
     matching charge is still counted), everyday pace, other plans (every
     other spend category's plan above its bills and what is spent, spread
     over the rest of the month), one-offs; a link to the timeline on the
     Plan tab.
4. **Attention row** of pills: N alerts (orange when any critical), N
   questions from the AI, N to review. Each opens the Activity inbox
   filtered.
5. **Totals strip**: planned in, planned out, left to assign (zero-based
   check).
6. **Groups → categories** list. Each row: name; a context subtitle (due
   day, "paid", upcoming plan phase, pace, or the standing note); spent /
   plan; left (orange when over or pacing over); progress bar. Income rows
   show "still expected" and a blue bar. A category planned per paycheck
   shows "$X × N paychecks" (N from its own pay schedule or the
   household's, so a three-paycheck month plans three) and, when it has its
   own schedule, "every 2 weeks" or "monthly on the 1st". Tap → Category
   detail.

### Category detail

- Hero: left this month (large), spent and plan, progress bar with a
  marker for how far through the month we are, three tiles: actual pace,
  plan pace, projected month-end.
- Actions: Set plan · Move money · Ask Claude. For a per-paycheck
  category the plan form takes the amount per paycheck and shows what the
  month comes to.
- Plan phases: "Now" and any future phase ("After the move · from Jan"),
  per month or per paycheck.
- Edit: name, group, everyday, plan per paycheck (switching converts the
  amount so the month's plan stays the same, and the hint says what it
  becomes), and for a per-paycheck category whether it is paid on the
  household schedule or its own (weekly, every two weeks, twice a month,
  monthly), archived.
- Standing note: free text read by the AI and (for a few literal tokens)
  by the alert rules.
- This month's transactions with source and how each was categorized
  (rule name, "by Claude", manual).

### Activity (inbox)

Tabs: **Inbox** · All activity · Done.

- **Alerts** as cards: type label, one-line title, plain-language detail,
  amount; actions Acknowledge · Resolve… · Snooze. Critical alerts get an
  orange outline.
- **Questions from the AI** on a warm tint with the AI's option buttons and
  "Type an answer…". A question carrying a **proposal** shows the exact
  change ("Groceries plan $800 → $900") with Approve · Edit · Decline.
- **Claude did …** entries when the assistant is in Driver mode: what
  changed, before → after, and Undo where available. These appear under
  "All activity", not in the Inbox, unless an Undo is still available.
- **To categorize**: uncategorized transactions with suggested-category
  chips, "Other…", "+ make a rule", and a "Let Claude take a pass" button
  for the remainder.

### Plan

A timeline of what's coming, soonest first, with the same card language:

- **Cash ahead**: expected cash from today's estimate over 4, 8 or 13
  weeks: now, the low point (and when), and the balance at the end, with a
  pill for the distance from the cash floor at the low. Below, every row in
  date order with the running balance after it: each paycheck (income
  categories planned per paycheck, on the household's paydays or the
  category's own), each bill occurrence (a per-paycheck bill on every
  payday), the rest of each other spend category's plan as a muted
  "<category> (remaining plan)" row per stretch,
  planned one-offs, and the everyday drain as one row per stretch between
  them. A bill or one-off past its date with no matching charge is still
  expected: it lands today, marked past due, and stays until the charge
  posts. Tap a bill or one-off → edit. The stretch to the first paycheck is
  the low point on the home screen.
- **Recurring bills**: every active bill grouped by cadence (every paycheck
  · monthly · every N months · yearly), with amount, next due, category,
  the card or account it hits, and a canceled/paused tag. A per-paycheck
  bill shows "each payday" and this month's cost (amount × paydays). Tap →
  edit; "+ Add a bill" opens a short form whose "How often" includes
  "Every paycheck" (the due date snaps to a payday). The same form is what
  the AI fills from a sentence.
- **Sinking fund**: funded balance vs should-have for the non-monthly bills,
  the monthly set-aside, and an "update balance" button.
- **One-offs**: planned items with status chips (planned · done · dropped).
  "Mark done" appears once a charge is matched; before that the only way
  to close one is "Paid outside the feed" (cash, another account), which
  records that flag. Otherwise it stays planned and overdue.

### Goals

Getting out of debt, then building savings; same card language.

- **Debt-free date** hero: the projected month under the current strategy,
  total interest remaining, and "N months sooner / $X less" if the extra
  payment slider moves.
- **Debts**: one card each with balance, APR, minimum, due day, interest per
  day, payoff month, and the order the strategy pays them in. Tap → edit,
  set balance, what-if a charge.
- **Strategy**: avalanche · snowball · custom, shown side by side with payoff
  month and total interest; the chosen one is highlighted. Switching is a
  deliberate tap (or the assistant's recorded suggestion), never automatic.
- **Savings goals**: target, saved so far, monthly contribution, reach date
  at current pace, and the pace needed for a target date.
- *Later phase:* net worth over time from balance snapshots.

### More

Accounts, rules, settings, users, API keys, integrations, jobs.

**Settings → Assistant** has the mode toggle (Pull: Claude app + recurring
task, the default; API: paste your own key, pick models per task, set a
monthly cap, see usage) and the Backseat / Driver switch: one global
position and per-area overrides (categorizing & notes · plans & bills ·
debt strategy & goals), each with a one-line description of what the
assistant may do in that position.
