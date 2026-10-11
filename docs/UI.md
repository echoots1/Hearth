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
     payday, a pill with distance above/below the cash floor, and three
     tiles explaining the deductions: bills due, everyday pace, planned.
4. **Attention row** of pills: N alerts (orange when any critical), N
   questions from the AI, N to review. Each opens the Activity inbox
   filtered.
5. **Totals strip**: planned in, planned out, left to assign (zero-based
   check).
6. **Groups → categories** list. Each row: name; a context subtitle (due
   day, "paid", upcoming plan phase, pace, or the standing note); spent /
   plan; left (orange when over or pacing over); progress bar. Income rows
   show "still expected" and a blue bar. Tap → Category detail.

### Category detail

- Hero: left this month (large), spent and plan, progress bar with a
  marker for how far through the month we are, three tiles: actual pace,
  plan pace, projected month-end.
- Actions: Set plan · Move money · Ask Claude.
- Plan phases: "Now" and any future phase ("After the move · from Jan").
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

- **Up next**: recurring bills and planned one-offs between now and the next
  paycheck, each with due date, amount, category, and whether it's already
  posted. Mirrors the low-point tiles on the home screen.
- **Recurring bills**: every active bill grouped by cadence (monthly ·
  every N months · yearly), with amount, next due, category, the card or
  account it hits, and a canceled/paused tag. Tap → edit; "+ Add a bill"
  opens a short form. The same form is what the AI fills from a sentence.
- **Sinking fund**: funded balance vs should-have for the non-monthly bills,
  the monthly set-aside, and an "update balance" button.
- **One-offs**: planned items with status chips (planned · done · dropped).

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
