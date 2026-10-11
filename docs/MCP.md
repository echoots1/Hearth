# Hearth — MCP server

**Hearth — household money, on plan.**

Hearth exposes every operation the UI has as an MCP tool, over
[Streamable HTTP](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)
at `/mcp`. The assistant (Claude, in the Claude app or over the API) and the
web UI call the same registry, so they never disagree about a number.

## Connecting

| | |
| --- | --- |
| Endpoint | `https://<your-hearth-host>/mcp` |
| Transport | Streamable HTTP (POST for messages, optional GET for the server stream) |
| Auth | An API key from **More → Settings → API keys** |
| Key scopes | `read` (reads only) · `read_write` (reads and writes, through the autonomy switch) |

Send the key one of two ways:

1. **Header** (preferred): `Authorization: Bearer hearth_…`
2. **In the URL**, for clients that cannot set headers (the Claude app's
   custom connector dialog): `https://<host>/mcp/hearth_…`

Keys are hashed at rest and shown once when created. Revoke a key in
Settings; the next request with it gets `401`. A key in the URL is never
written to Hearth's logs (the path is logged as `/mcp/<key>`), but it does sit
in your proxy's access log and the client's config: use a key you can revoke
and keep the host on HTTPS.

The server answers only to the `Host` values it expects (the host of
`APP_URL`, with and without port, plus loopback). If you reach Hearth under
another name, set `MCP_ALLOWED_HOSTS`. See `.env.example`.

### Quick check with curl

```sh
H='https://money.example.net/mcp'; K='hearth_…'
curl -s "$H" -H "Authorization: Bearer $K" -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' -D - \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"curl","version":"0"}}}'
# copy the mcp-session-id header into the next calls
curl -s "$H" -H "Authorization: Bearer $K" -H "mcp-session-id: $SID" -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"household.today","arguments":{}}}'
```

## Conventions

- Names are `<noun>.<verb>`. Reads are safe to call any time; writes are
  annotated `readOnlyHint: false`, and `*.delete` / `*.cancel` carry
  `destructiveHint: true`.
- Money is an integer number of **cents** in fields named `*_cents`. Dates
  are ISO `YYYY-MM-DD` in the household's timezone; months are `YYYY-MM`.
- List tools take `limit` (clamped to 1..=500) and return a `next_cursor`
  when there is more.
- Every result is returned twice: as pretty-printed JSON text and as
  `structuredContent`.
- Domain problems (not found, invalid input, read-only key, a conflict) come
  back as a tool result with `isError: true` and a one-line reason the model
  can act on. Protocol errors (unknown tool, arguments that do not match the
  schema, a database failure) are JSON-RPC errors.

## The autonomy switch

Writes with a `read_write` key pass through the household's **Backseat /
Driver** setting, globally or per area (`categorizing_notes`, `plans_bills`,
`debts_goals`). `household.today` reports the current position.

- **Backseat:** the tool records a *proposal* and returns it with
  `"outcome": "proposed"`. Nothing changes until a person approves it in the
  UI (Approve, Edit, Decline). Say "I've proposed…".
- **Driver:** the tool applies the change and returns `"outcome": "applied"`
  with the recorded action, undoable from Activity where an inverse exists.
  Say "I've set…".
- Acknowledging, resolving and snoozing alerts, asking and answering
  questions, and the pass summary always apply: they are the assistant
  acting as the humans' hands on the inbox.
- The setting itself is UI-only. `household.update_settings` refuses the
  `assistant` key.

## A pass

The recurring task in the Claude app (or the API-mode pass) does this:

1. `household.today`: date, days left, next payday, autonomy, mode.
2. `assistant.work`: the queue in one call: transactions to categorize with
   suggestions, answers since the last pass, pending proposals, open alerts,
   overdue bills, categories whose bills exceed their plan, stale balances.
3. Work the queue with the write tools; ask with `questions.ask` when a
   category is genuinely ambiguous rather than guessing.
4. `assistant.summary_write` with two or three lines; it shows in Activity.

## Tool reference

<!-- BEGIN GENERATED: hearth tools -->

_Generated from the registry by `hearth tools`; do not edit by hand._

### Reads

| Tool | Description |
| --- | --- |
| `accounts.list` | Accounts with reported and bank balances. |
| `alerts.get` | One alert. |
| `alerts.list` | Alerts, most severe first. |
| `assistant.actions` | What the assistant changed in Driver mode, newest first, with undo availability. |
| `assistant.work` | Everything waiting for the assistant in one call: transactions to categorize (with suggestions), answered questions since the last pass, pending proposals, open alerts, overdue bills, categories whose bills exceed their plan, stale balances, and the autonomy settings. |
| `audit.list` | The audit log (every write with actor, before and after). |
| `balance.overview` | Cash now: per-account running estimates with ages, the household estimate, and the projected low point before the next payday with its components (bills due, everyday pace, planned items) and headroom over the cash floor. |
| `bills.get` | One recurring bill with its splits. |
| `bills.list` | Recurring bills (active by default) with monthly equivalents and category names. `upcoming` limits to bills due before the next payday. |
| `budget.category` | One category in detail: its view for the month, its plan phases, the bills in it, and this month's transactions. |
| `budget.groups` | Category groups and every category (including archived when asked). |
| `budget.month` | The budget for a month: groups → categories with plan, spent (activity), left, pace, bill totals and the next plan phase; plus household totals and uncategorized activity. |
| `debts.compare` | Payoff month and total interest under avalanche, snowball and custom order, side by side. |
| `debts.list` | Debts with interest per day and per month. |
| `debts.projection` | Payoff projection under the current strategy: payoff month and total interest per debt, debt-free month. |
| `debts.what_if` | What a one-time charge on a debt, or an extra monthly payment, does to payoff dates and interest. |
| `goals.list` | Savings goals with projections (reach month at current pace, pace needed for the target date). |
| `household.settings` | All household settings (pay schedule, cash floor, thresholds, quiet hours, nag, assistant mode and autonomy). |
| `household.today` | Today's date and month in the household timezone, days left in the month, the next payday, the assistant mode and autonomy settings, and the household name. |
| `jobs.status` | Every job with its interval, last run (status, summary, error) and whether it is due. |
| `planned.list` | Planned one-off items. |
| `questions.list` | Questions and proposals, with answers. |
| `rules.list` | Categorization rules in priority order. |
| `rules.preview` | Which recent transactions a pattern would match. |
| `sinking.overview` | The sinking fund: funded balance, monthly set-aside, should-have-saved and shortfall across every non-monthly bill. |
| `transactions.get` | One transaction. |
| `transactions.list` | Transactions, newest first, with filters and cursor paging. |
| `transactions.review_queue` | Uncategorized transactions waiting for a decision. |

### Writes

| Tool | Area | Description |
| --- | --- | --- |
| `accounts.create` | always applies | Add a manual account (checking, savings, credit card, cash…). Synced accounts come from the integrations. |
| `accounts.update` | always applies | Rename an account, change its type (checking, savings, credit card, loan…), change whether it counts as cash, or deactivate it. Works on synced accounts too. |
| `alerts.ack` | always applies | Acknowledge an alert (a human has seen it; nagging stops). |
| `alerts.reopen` | always applies | Reopen a done alert. |
| `alerts.resolve` | always applies | Mark an alert done with a short resolution. |
| `alerts.snooze` | always applies | Pause nagging on an alert until a time. |
| `assistant.summary_write` | always applies | Leave a short report of a pass; shows in Activity and marks the pass time. |
| `balance.set_reported` | categorizing_notes | Record the balance a person just read off their bank, with when they read it. |
| `bills.add` | plans_bills | Add a recurring bill from a sentence: name, amount, cadence, next due; optional category (by id or name), account, match pattern. Returns the bill, the category's bill total and plan, and whether the plan now falls short. |
| `bills.cancel` | plans_bills | Mark a bill canceled; any later charge matching it raises a critical alert. |
| `bills.delete` | plans_bills | Delete a bill outright (prefer cancel, which keeps history). |
| `bills.pause` | plans_bills | Pause a bill (no due-date alerts, not counted in totals) without canceling it. |
| `bills.resume` | plans_bills | Reactivate a paused or canceled bill. |
| `bills.split` | plans_bills | Split a bill across categories. Only when a person asks for it; the assistant must not split on its own initiative. |
| `bills.update` | plans_bills | Change a bill's amount, due date, cadence, category, pattern, note or status. |
| `budget.create_category` | plans_bills | Create a category (and its group if needed), optionally with a plan. |
| `budget.delete_plan` | plans_bills | Remove a plan phase. |
| `budget.set_category_note` | categorizing_notes | Set a category's standing note (read by the assistant; jobs honor [no-alerts], [no-large-charge], [no-pace]). |
| `budget.set_plan` | plans_bills | Set a category's plan from a month onward (a plan phase). Omit starts_month for the current month. |
| `budget.update_category` | plans_bills | Rename, move, flag or archive a category. |
| `debts.set_balance` | debts_goals | Record a debt's current balance. |
| `debts.set_strategy` | debts_goals | Set the payoff strategy (avalanche, snowball, custom order) and the extra monthly payment. |
| `debts.upsert` | debts_goals | Add a debt or update one (name, balance, APR in bps, minimum, planned payment, due day). |
| `goals.create` | debts_goals | Create a savings goal. |
| `goals.set_saved` | debts_goals | Record how much is saved toward a goal. |
| `goals.update` | debts_goals | Change a goal or its status. |
| `household.update_settings` | plans_bills | Change a household setting (pay schedule, cash floor, thresholds, quiet hours, nag). The `assistant` setting is UI-only. |
| `jobs.run` | always applies | Run a job now (under its lock); returns the finished run with its summary. |
| `planned.create` | plans_bills | Approve a one-off expense ahead of time so it isn't flagged when it posts. |
| `planned.update` | plans_bills | Change a planned item or its status (planned, done, dropped). |
| `questions.answer` | always applies | Answer an open question (people, or the assistant relaying an answer given in chat). |
| `questions.ask` | always applies | Ask the humans something (shows in the inbox and is pushed); optional one-tap options. |
| `questions.dismiss` | always applies | Dismiss a question without answering. |
| `rules.create` | categorizing_notes | Create a categorization rule (contains or regex) and apply it to existing uncategorized rows. |
| `rules.delete` | categorizing_notes | Delete a rule. |
| `rules.update` | categorizing_notes | Change a rule. |
| `sinking.set_balance` | plans_bills | Record what's actually saved in the sinking fund. |
| `transactions.apply_rules` | categorizing_notes | Run the categorization rules over uncategorized transactions (or everything with force). |
| `transactions.categorize` | categorizing_notes | Set (or clear) a transaction's category and optionally its note; resolves the review item. |
| `transactions.create_manual` | categorizing_notes | Record a transaction by hand (cash, a check, something the feed missed). |
| `transactions.delete` | categorizing_notes | Soft-delete a transaction (manual or CSV rows; synced rows come back on the next sync). |
| `transactions.link_planned` | categorizing_notes | Link a transaction to a planned item (marks it done) or unlink it. |
| `transactions.set_note` | categorizing_notes | Set a transaction's note. |

### Input schemas

<details><summary><code>accounts.create</code></summary>

```json
{
  "$defs": {
    "AccountType": {
      "enum": [
        "checking",
        "savings",
        "credit_card",
        "virtual_card",
        "cash",
        "loan",
        "investment",
        "other"
      ],
      "type": "string"
    }
  },
  "properties": {
    "include_in_cash": {
      "description": "Counts toward Cash now and the low point. Default: checking, savings and cash do.",
      "type": [
        "boolean",
        "null"
      ]
    },
    "institution_label": {
      "type": [
        "string",
        "null"
      ]
    },
    "mask": {
      "type": [
        "string",
        "null"
      ]
    },
    "name": {
      "type": "string"
    },
    "type": {
      "$ref": "#/$defs/AccountType"
    }
  },
  "required": [
    "name",
    "type"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>accounts.list</code></summary>

```json
{
  "properties": {
    "include_inactive": {
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>accounts.update</code></summary>

```json
{
  "$defs": {
    "AccountType": {
      "enum": [
        "checking",
        "savings",
        "credit_card",
        "virtual_card",
        "cash",
        "loan",
        "investment",
        "other"
      ],
      "type": "string"
    }
  },
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": "string"
    },
    "include_in_cash": {
      "description": "Counts toward Cash now and the low point. When the type changes and this is omitted, the new type's default applies (checking, savings and cash count).",
      "type": [
        "boolean",
        "null"
      ]
    },
    "institution_label": {
      "type": [
        "string",
        "null"
      ]
    },
    "is_active": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "name": {
      "type": [
        "string",
        "null"
      ]
    },
    "type": {
      "anyOf": [
        {
          "$ref": "#/$defs/AccountType"
        },
        {
          "type": "null"
        }
      ],
      "description": "Reclassify the account: checking, savings, credit_card, virtual_card, cash, loan, investment or other. A bank-synced account's type is only Plaid's guess; fix it here when it is wrong."
    }
  },
  "required": [
    "account_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>alerts.ack</code></summary>

```json
{
  "properties": {
    "alert_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "alert_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>alerts.get</code></summary>

```json
{
  "properties": {
    "id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>alerts.list</code></summary>

```json
{
  "$defs": {
    "AlertSeverity": {
      "enum": [
        "info",
        "warn",
        "critical"
      ],
      "type": "string"
    },
    "AlertStatus": {
      "enum": [
        "open",
        "notified",
        "ack",
        "done"
      ],
      "type": "string"
    },
    "AlertType": {
      "enum": [
        "unplanned_large_charge",
        "canceled_bill_charged",
        "bill_amount_changed",
        "bill_not_seen",
        "category_blown",
        "category_pace",
        "floor_breached",
        "debt_redraw",
        "card_declined",
        "planned_item_missing",
        "balance_stale",
        "sync_failed",
        "review_queue_backlog",
        "milestone",
        "api_spend_cap"
      ],
      "type": "string"
    }
  },
  "properties": {
    "limit": {
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "live": {
      "description": "Everything not done.",
      "type": [
        "boolean",
        "null"
      ]
    },
    "severity": {
      "anyOf": [
        {
          "$ref": "#/$defs/AlertSeverity"
        },
        {
          "type": "null"
        }
      ]
    },
    "status": {
      "anyOf": [
        {
          "$ref": "#/$defs/AlertStatus"
        },
        {
          "type": "null"
        }
      ]
    },
    "type": {
      "anyOf": [
        {
          "$ref": "#/$defs/AlertType"
        },
        {
          "type": "null"
        }
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>alerts.reopen</code></summary>

```json
{
  "properties": {
    "alert_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "alert_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>alerts.resolve</code></summary>

```json
{
  "properties": {
    "alert_id": {
      "format": "uuid",
      "type": "string"
    },
    "resolution": {
      "type": "string"
    }
  },
  "required": [
    "alert_id",
    "resolution"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>alerts.snooze</code></summary>

```json
{
  "properties": {
    "alert_id": {
      "format": "uuid",
      "type": "string"
    },
    "until": {
      "format": "date-time",
      "type": "string"
    }
  },
  "required": [
    "alert_id",
    "until"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>assistant.actions</code></summary>

```json
{
  "properties": {
    "limit": {
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "undoable_only": {
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>assistant.summary_write</code></summary>

```json
{
  "properties": {
    "text": {
      "description": "A short report of what the pass did and found.",
      "type": "string"
    }
  },
  "required": [
    "text"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>assistant.work</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>audit.list</code></summary>

```json
{
  "properties": {
    "entity": {
      "type": [
        "string",
        "null"
      ]
    },
    "entity_id": {
      "type": [
        "string",
        "null"
      ]
    },
    "limit": {
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>balance.overview</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>balance.set_reported</code></summary>

```json
{
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": "string"
    },
    "as_of": {
      "description": "Defaults to now.",
      "format": "date-time",
      "type": [
        "string",
        "null"
      ]
    },
    "balance_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "account_id",
    "balance_cents"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>bills.add</code></summary>

```json
{
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "amount_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    },
    "cadence_months": {
      "description": "1 monthly (default), 3 quarterly, 6, 12 yearly.",
      "format": "int16",
      "maximum": 32767,
      "minimum": -32768,
      "type": [
        "integer",
        "null"
      ]
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "category_name": {
      "type": [
        "string",
        "null"
      ]
    },
    "match_pattern": {
      "description": "Substring of the bank description that identifies the charge; defaults to the name.",
      "type": [
        "string",
        "null"
      ]
    },
    "name": {
      "type": "string"
    },
    "next_due": {
      "format": "date",
      "type": "string"
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "raise_plan": {
      "description": "Raise the category plan if its bills now exceed it (default true).",
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "required": [
    "name",
    "amount_cents",
    "next_due"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>bills.cancel</code></summary>

```json
{
  "properties": {
    "bill_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "bill_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>bills.delete</code></summary>

```json
{
  "properties": {
    "bill_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "bill_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>bills.get</code></summary>

```json
{
  "properties": {
    "id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>bills.list</code></summary>

```json
{
  "$defs": {
    "BillStatus": {
      "enum": [
        "active",
        "paused",
        "canceled"
      ],
      "type": "string"
    }
  },
  "properties": {
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "due_before": {
      "description": "Only bills due before this date.",
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    },
    "include_canceled": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "status": {
      "anyOf": [
        {
          "$ref": "#/$defs/BillStatus"
        },
        {
          "type": "null"
        }
      ]
    },
    "upcoming": {
      "description": "Shortcut: only bills due before the next payday.",
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>bills.pause</code></summary>

```json
{
  "properties": {
    "bill_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "bill_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>bills.resume</code></summary>

```json
{
  "properties": {
    "bill_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "bill_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>bills.split</code></summary>

```json
{
  "$defs": {
    "SplitPart": {
      "properties": {
        "amount_cents": {
          "description": "Money in integer cents; negative = out.",
          "format": "int64",
          "type": "integer"
        },
        "category_id": {
          "format": "uuid",
          "type": "string"
        }
      },
      "required": [
        "category_id",
        "amount_cents"
      ],
      "type": "object"
    }
  },
  "properties": {
    "bill_id": {
      "format": "uuid",
      "type": "string"
    },
    "splits": {
      "description": "Must total the bill amount; empty clears the split.",
      "items": {
        "$ref": "#/$defs/SplitPart"
      },
      "type": "array"
    }
  },
  "required": [
    "bill_id",
    "splits"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>bills.update</code></summary>

```json
{
  "$defs": {
    "BillStatus": {
      "enum": [
        "active",
        "paused",
        "canceled"
      ],
      "type": "string"
    }
  },
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "amount_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "bill_id": {
      "format": "uuid",
      "type": "string"
    },
    "cadence_months": {
      "format": "int16",
      "maximum": 32767,
      "minimum": -32768,
      "type": [
        "integer",
        "null"
      ]
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "match_pattern": {
      "type": [
        "string",
        "null"
      ]
    },
    "name": {
      "type": [
        "string",
        "null"
      ]
    },
    "next_due": {
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "status": {
      "anyOf": [
        {
          "$ref": "#/$defs/BillStatus"
        },
        {
          "type": "null"
        }
      ]
    }
  },
  "required": [
    "bill_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>budget.category</code></summary>

```json
{
  "properties": {
    "id": {
      "description": "Category id, or omit and give `name`.",
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "month": {
      "type": [
        "string",
        "null"
      ]
    },
    "name": {
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>budget.create_category</code></summary>

```json
{
  "properties": {
    "group_id": {
      "description": "Group id, or a group name (created if missing).",
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "group_name": {
      "type": [
        "string",
        "null"
      ]
    },
    "income": {
      "description": "`income` or `spend` (default).",
      "type": [
        "boolean",
        "null"
      ]
    },
    "is_everyday": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "name": {
      "type": "string"
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "plan_cents": {
      "description": "Plan for this month onward, in cents.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "name"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>budget.delete_plan</code></summary>

```json
{
  "properties": {
    "plan_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "plan_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>budget.groups</code></summary>

```json
{
  "properties": {
    "include_inactive": {
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>budget.month</code></summary>

```json
{
  "properties": {
    "month": {
      "description": "`YYYY-MM`; defaults to the current month.",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>budget.set_category_note</code></summary>

```json
{
  "properties": {
    "category_id": {
      "format": "uuid",
      "type": "string"
    },
    "note": {
      "type": "string"
    }
  },
  "required": [
    "category_id",
    "note"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>budget.set_plan</code></summary>

```json
{
  "properties": {
    "amount_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "category_name": {
      "type": [
        "string",
        "null"
      ]
    },
    "label": {
      "description": "e.g. \"after the move\"",
      "type": [
        "string",
        "null"
      ]
    },
    "starts_month": {
      "description": "`YYYY-MM` the new amount starts; defaults to the current month.",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "amount_cents"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>budget.update_category</code></summary>

```json
{
  "properties": {
    "category_id": {
      "format": "uuid",
      "type": "string"
    },
    "group_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "is_archived": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "is_everyday": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "name": {
      "type": [
        "string",
        "null"
      ]
    },
    "sort_order": {
      "format": "int32",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "category_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>debts.compare</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>debts.list</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>debts.projection</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>debts.set_balance</code></summary>

```json
{
  "properties": {
    "as_of": {
      "format": "date-time",
      "type": [
        "string",
        "null"
      ]
    },
    "balance_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    },
    "debt_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "debt_id",
    "balance_cents"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>debts.set_strategy</code></summary>

```json
{
  "$defs": {
    "DebtStrategyKind": {
      "oneOf": [
        {
          "const": "avalanche",
          "description": "Highest APR first.",
          "type": "string"
        },
        {
          "const": "snowball",
          "description": "Smallest balance first.",
          "type": "string"
        },
        {
          "const": "custom",
          "description": "`debts.custom_order` / `custom_order` list.",
          "type": "string"
        }
      ]
    }
  },
  "properties": {
    "custom_order": {
      "items": {
        "format": "uuid",
        "type": "string"
      },
      "type": [
        "array",
        "null"
      ]
    },
    "extra_payment_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "kind": {
      "anyOf": [
        {
          "$ref": "#/$defs/DebtStrategyKind"
        },
        {
          "type": "null"
        }
      ],
      "description": "avalanche | snowball | custom"
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>debts.upsert</code></summary>

```json
{
  "$defs": {
    "DebtKind": {
      "enum": [
        "credit_card",
        "loan",
        "mortgage",
        "other"
      ],
      "type": "string"
    }
  },
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "apr_bps": {
      "description": "APR in basis points: 24.99% = 2499.",
      "format": "int32",
      "type": "integer"
    },
    "balance_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    },
    "debt_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "due_day": {
      "format": "int16",
      "maximum": 32767,
      "minimum": -32768,
      "type": "integer"
    },
    "kind": {
      "anyOf": [
        {
          "$ref": "#/$defs/DebtKind"
        },
        {
          "type": "null"
        }
      ]
    },
    "min_payment_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    },
    "name": {
      "type": "string"
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "planned_payment_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "name",
    "balance_cents",
    "apr_bps",
    "min_payment_cents",
    "due_day"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>debts.what_if</code></summary>

```json
{
  "properties": {
    "charge_cents": {
      "description": "A one-time charge to put on this debt, in cents.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "debt_id": {
      "format": "uuid",
      "type": "string"
    },
    "extra_monthly_cents": {
      "description": "An extra amount paid every month, in cents.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "debt_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>goals.create</code></summary>

```json
{
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "name": {
      "type": "string"
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "saved_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "target_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    },
    "target_date": {
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "name",
    "target_cents"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>goals.list</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>goals.set_saved</code></summary>

```json
{
  "properties": {
    "as_of": {
      "format": "date-time",
      "type": [
        "string",
        "null"
      ]
    },
    "goal_id": {
      "format": "uuid",
      "type": "string"
    },
    "saved_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "goal_id",
    "saved_cents"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>goals.update</code></summary>

```json
{
  "$defs": {
    "GoalStatus": {
      "enum": [
        "active",
        "reached",
        "paused",
        "dropped"
      ],
      "type": "string"
    }
  },
  "properties": {
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "goal_id": {
      "format": "uuid",
      "type": "string"
    },
    "name": {
      "type": [
        "string",
        "null"
      ]
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "status": {
      "anyOf": [
        {
          "$ref": "#/$defs/GoalStatus"
        },
        {
          "type": "null"
        }
      ]
    },
    "target_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "target_date": {
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "goal_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>household.settings</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>household.today</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>household.update_settings</code></summary>

```json
{
  "properties": {
    "key": {
      "description": "Setting key (anything except `assistant`, which is UI-only).",
      "type": "string"
    },
    "value": true
  },
  "required": [
    "key",
    "value"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>jobs.run</code></summary>

```json
{
  "properties": {
    "name": {
      "description": "`process`, `evaluate`, `notify`, `housekeeping`, or a configured `sync:*`.",
      "type": "string"
    }
  },
  "required": [
    "name"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>jobs.status</code></summary>

```json
{
  "type": "object"
}
```

</details>

<details><summary><code>planned.create</code></summary>

```json
{
  "properties": {
    "amount_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "category_name": {
      "type": [
        "string",
        "null"
      ]
    },
    "date": {
      "format": "date",
      "type": "string"
    },
    "decided_by": {
      "type": [
        "string",
        "null"
      ]
    },
    "item": {
      "type": "string"
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "date",
    "item",
    "amount_cents"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>planned.list</code></summary>

```json
{
  "$defs": {
    "PlannedStatus": {
      "enum": [
        "planned",
        "done",
        "dropped"
      ],
      "type": "string"
    }
  },
  "properties": {
    "from": {
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    },
    "limit": {
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "status": {
      "anyOf": [
        {
          "$ref": "#/$defs/PlannedStatus"
        },
        {
          "type": "null"
        }
      ]
    },
    "to": {
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>planned.update</code></summary>

```json
{
  "$defs": {
    "PlannedStatus": {
      "enum": [
        "planned",
        "done",
        "dropped"
      ],
      "type": "string"
    }
  },
  "properties": {
    "amount_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "date": {
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    },
    "item": {
      "type": [
        "string",
        "null"
      ]
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "planned_item_id": {
      "format": "uuid",
      "type": "string"
    },
    "status": {
      "anyOf": [
        {
          "$ref": "#/$defs/PlannedStatus"
        },
        {
          "type": "null"
        }
      ]
    }
  },
  "required": [
    "planned_item_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>questions.answer</code></summary>

```json
{
  "properties": {
    "answer": {
      "type": "string"
    },
    "question_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "question_id",
    "answer"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>questions.ask</code></summary>

```json
{
  "properties": {
    "alert_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "bill_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "context": true,
    "options": {
      "description": "`[{\"key\": \"groceries\", \"label\": \"Groceries\"}]`"
    },
    "question": {
      "type": "string"
    },
    "transaction_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "question"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>questions.dismiss</code></summary>

```json
{
  "properties": {
    "question_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "question_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>questions.list</code></summary>

```json
{
  "$defs": {
    "QuestionKind": {
      "enum": [
        "ask",
        "proposal"
      ],
      "type": "string"
    },
    "QuestionStatus": {
      "enum": [
        "open",
        "answered",
        "approved",
        "declined",
        "dismissed"
      ],
      "type": "string"
    }
  },
  "properties": {
    "kind": {
      "anyOf": [
        {
          "$ref": "#/$defs/QuestionKind"
        },
        {
          "type": "null"
        }
      ]
    },
    "limit": {
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "status": {
      "anyOf": [
        {
          "$ref": "#/$defs/QuestionStatus"
        },
        {
          "type": "null"
        }
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>rules.create</code></summary>

```json
{
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "amount_max_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "amount_min_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "apply_now": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "category_name": {
      "type": [
        "string",
        "null"
      ]
    },
    "ignore": {
      "description": "`true` marks matches as reviewed with no category (transfers, refunds).",
      "type": [
        "boolean",
        "null"
      ]
    },
    "name": {
      "type": [
        "string",
        "null"
      ]
    },
    "pattern": {
      "type": "string"
    },
    "priority": {
      "format": "int32",
      "type": [
        "integer",
        "null"
      ]
    },
    "regex": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "set_note": {
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "pattern"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>rules.delete</code></summary>

```json
{
  "properties": {
    "rule_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "rule_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>rules.list</code></summary>

```json
{
  "properties": {
    "include_inactive": {
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>rules.preview</code></summary>

```json
{
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "pattern": {
      "type": "string"
    },
    "regex": {
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "required": [
    "pattern"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>rules.update</code></summary>

```json
{
  "properties": {
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "is_active": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "name": {
      "type": [
        "string",
        "null"
      ]
    },
    "pattern": {
      "type": [
        "string",
        "null"
      ]
    },
    "priority": {
      "format": "int32",
      "type": [
        "integer",
        "null"
      ]
    },
    "regex": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "rule_id": {
      "format": "uuid",
      "type": "string"
    },
    "set_note": {
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "rule_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>sinking.overview</code></summary>

```json
{
  "properties": {
    "month": {
      "description": "`YYYY-MM`; defaults to the current month.",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>sinking.set_balance</code></summary>

```json
{
  "properties": {
    "as_of": {
      "format": "date-time",
      "type": [
        "string",
        "null"
      ]
    },
    "balance_cents": {
      "description": "Money in integer cents; negative = out.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "balance_cents"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>transactions.apply_rules</code></summary>

```json
{
  "properties": {
    "force": {
      "description": "Re-run on every transaction, overriding earlier decisions. Default false.",
      "type": [
        "boolean",
        "null"
      ]
    },
    "transaction_ids": {
      "items": {
        "format": "uuid",
        "type": "string"
      },
      "type": [
        "array",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>transactions.categorize</code></summary>

```json
{
  "properties": {
    "category_id": {
      "description": "Category id, or null to clear.",
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "category_name": {
      "description": "Or name the category instead of its id.",
      "type": [
        "string",
        "null"
      ]
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "transaction_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "transaction_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>transactions.create_manual</code></summary>

```json
{
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": "string"
    },
    "amount_cents": {
      "description": "Signed cents: negative = money out.",
      "format": "int64",
      "type": "integer"
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "date": {
      "format": "date",
      "type": "string"
    },
    "description": {
      "type": "string"
    },
    "note": {
      "type": [
        "string",
        "null"
      ]
    },
    "pending": {
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "required": [
    "account_id",
    "date",
    "description",
    "amount_cents"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>transactions.delete</code></summary>

```json
{
  "properties": {
    "transaction_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "transaction_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>transactions.get</code></summary>

```json
{
  "properties": {
    "id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>transactions.link_planned</code></summary>

```json
{
  "properties": {
    "planned_item_id": {
      "description": "Planned item id, or null to unlink.",
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "transaction_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "transaction_id"
  ],
  "type": "object"
}
```

</details>

<details><summary><code>transactions.list</code></summary>

```json
{
  "properties": {
    "account_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "category_id": {
      "format": "uuid",
      "type": [
        "string",
        "null"
      ]
    },
    "cursor": {
      "type": [
        "string",
        "null"
      ]
    },
    "from": {
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    },
    "limit": {
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "month": {
      "type": [
        "string",
        "null"
      ]
    },
    "pending": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "q": {
      "description": "Substring search on description and note.",
      "type": [
        "string",
        "null"
      ]
    },
    "to": {
      "format": "date",
      "type": [
        "string",
        "null"
      ]
    },
    "uncategorized": {
      "type": [
        "boolean",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>transactions.review_queue</code></summary>

```json
{
  "properties": {
    "limit": {
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "type": "object"
}
```

</details>

<details><summary><code>transactions.set_note</code></summary>

```json
{
  "properties": {
    "note": {
      "type": "string"
    },
    "transaction_id": {
      "format": "uuid",
      "type": "string"
    }
  },
  "required": [
    "transaction_id",
    "note"
  ],
  "type": "object"
}
```

</details>

<!-- END GENERATED -->
