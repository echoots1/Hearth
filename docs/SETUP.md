# Hearth — Setup

**Hearth — household money, on plan.**

One Docker container plus Postgres, behind your own reverse proxy. This guide
takes you from a clone to a running app with the Claude app connected.
Claude-app menu labels below were checked against Anthropic's help center in
October 2026; if a label has moved, the idea is the same.

## 1. Run it

You need Docker with Compose, and a hostname with HTTPS that the household's
phones can reach. HTTPS is required for the installable app and push
notifications, and the Claude app must be able to reach the MCP endpoint over
the public internet for the connector to work.

```sh
git clone https://github.com/echoots1/Hearth.git && cd Hearth
cp .env.example .env
```

Edit `.env`. The lines that matter on day one:

| Setting | What to put |
| --- | --- |
| `APP_URL` | The public URL, e.g. `https://money.example.net`. Cookies go Secure when it starts with `https`. Notification links and the connector URL are built from it. |
| `APP_SECRET` | A random string of at least 32 characters (`openssl rand -base64 48`). Signs sessions and one-tap notification links. |
| `APP_ENCRYPTION_KEY` | Another random string. Encrypts the Plaid tokens, the VAPID push key and the optional Anthropic key at rest. Set it now; rotating it later means re-linking. |
| `POSTGRES_PASSWORD` | Change it from the example. `DATABASE_URL` in compose follows it. |
| `TRUST_PROXY` | `true` when behind a reverse proxy. |

Then check the file and start the stack. The first build compiles the Rust
backend from source and takes a few minutes; later builds reuse the cache.

```sh
scripts/preflight.sh            # checks .env for the usual mistakes
docker compose up -d --build
scripts/preflight.sh            # now also probes /api/health and tells you what's next
```

Open the app URL: the first visit creates the owner account and names the
household. Add people under **More → People**.

### Reverse proxy

Point your proxy at port `3000` (or `PORT`) and set `TRUST_PROXY=true`. The
MCP endpoint streams responses, so the proxy must pass `text/event-stream`
through without buffering. Caddy does all of this with one block and fetches
the certificate itself:

```
money.example.net {
    reverse_proxy localhost:3000
}
```

The nginx equivalent:

```nginx
server {
    server_name money.example.net;
    listen 443 ssl; # plus your certificate lines
    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_buffering off;
        proxy_read_timeout 300s;
        client_max_body_size 20m; # CSV imports
    }
}
```

Only the proxy should be reachable from the internet; Postgres is never
published, and port `3000` need not be either when the proxy runs on the
same host.

To look around with fake data first, load the demo household (it refuses a
database that already has users):

```sh
docker compose exec app hearth seed
```

Sign in as `owner@example.com` / `demo-password`. Wipe it later with
`hearth seed --force` on a fresh database, or just start over.

### Keep it

Everything lives in the `hearth-db` volume. Back it up with
`docker compose exec db pg_dump -U hearth hearth > hearth.sql`. The
`.env` file is the other half: without `APP_ENCRYPTION_KEY` the encrypted
secrets in the dump can't be read.

## 2. Connect the Claude app (Pull mode, the default)

Pull mode costs nothing beyond a Claude subscription: Claude reaches Hearth
through a custom connector when you chat, and a scheduled task does the
daily tending.

### Create a key

**More → API keys → Create a key.** Name it (`claude app`), scope
**Read and write** (writes still pass through the Backseat / Driver switch),
and copy the key: it is shown once. The page also shows the endpoint,
`https://<your host>/mcp`.

### Add the connector

In the Claude app (Pro, Max, Team, Enterprise; Free allows one custom
connector):

1. **Customize → Connectors → + Add → Add custom connector.**
2. Name: `Hearth`. Remote MCP server URL: `https://<your host>/mcp`. **Continue.**
3. Authentication: **No sign in.**
4. Request headers: add `Authorization` with the value `Bearer hearth_…`
   (your key). **Add.**

If your Claude client has no request-headers field, use the key-in-URL form
instead: `https://<your host>/mcp/hearth_…` with no header. It works the
same; the key then sits in the client's config and your proxy's access log,
so revoke it from **More → API keys** if that ever worries you.

In a conversation, open the **+** menu at the lower left → **Connectors**
and switch Hearth on. Then just talk: "we added a streaming service, $11.99
on the 14th", "how are groceries doing", "can we buy new tires this week".
Claude reads and writes through the same tools the app uses, and
**More → Assistant** decides whether its writes apply at once (Driver) or
wait for your approval (Backseat).

On Team and Enterprise an Owner adds the connector under **Organization
settings → Connectors** and members click **Connect** under
**Customize → Connectors**.

### The daily pass (scheduled task)

Scheduled tasks run remotely on Anthropic's side, so they run even when your
computer is asleep. They use the connectors you have already set up.

1. Click **Scheduled** in the left sidebar, then **New task → Set up manually**
   (or **Create with Claude** and describe the task).
2. **Task name:** `Hearth daily pass`.
3. **Prompt:**

   > Take a Hearth pass. Call `household.today`, then `assistant.work`. Work
   > through what it returns: categorize what you can with confidence, ask
   > with `questions.ask` when a category is genuinely ambiguous, note
   > overdue bills and open alerts, and respect your autonomy position
   > (`household.today` tells you Backseat or Driver per area). Finish with
   > `assistant.summary_write`: two or three plain lines about what you did
   > and what needs a person.

4. **Frequency:** daily. **Approval mode:** your call; Hearth's own switch
   already gates writes. **Save.**

If your message box has no Chat / Cowork choice, describe the task and its
frequency in any conversation and click **Schedule** when Claude proposes it.

"Do it now" requests stay in the Claude app: open a chat and say
"take a Hearth pass". The **Let Claude take a pass** button in Activity
reminds you of that in Pull mode.

## 3. Notifications

- **Push on your phone (built in).** Open the app URL on the phone and add it
  to the home screen (on iPhone: Share → Add to Home Screen; push only works
  from the installed app). Then **More → Notifications → Push on this device.**
  Hearth generates its own VAPID keys; no Firebase, no app store.
  Notifications carry **Acknowledge** and **Snooze** buttons that work
  without signing in (signed, expiring links).
- **ntfy.** Set `NTFY_TOPIC` (and `NTFY_URL` for a self-hosted server,
  `NTFY_TOKEN` if it needs one).
- **Email.** Set `SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`, `SMTP_PASS`,
  `SMTP_FROM`, `ALERT_EMAIL_TO`.

Quiet hours and the nag schedule live under **More → Settings**; a critical
alert can override quiet hours.

## 4. Getting transactions in

- **CSV** needs nothing: **More → Integrations → Import a CSV**, pick the
  account, upload the bank's export, map the columns once (save them as a
  preset), preview, import. Duplicates are skipped; every import can be
  rolled back.
- **Plaid.** Set `PLAID_CLIENT_ID`, `PLAID_SECRET`, `PLAID_ENV`
  (`sandbox` or `production`) and restart. **More → Integrations → Link a
  bank** opens Plaid Link (its script loads from Plaid's CDN only when you tap
  the button). Transactions and balances sync every `PLAID_SYNC_MINUTES`.
- **Privacy.com.** Set `PRIVACY_API_KEY` and `PRIVACY_ENV`. Cards become
  accounts, charges sync every `PRIVACY_SYNC_MINUTES`, declines become alerts,
  and the bank's later line for a card charge is linked so cash isn't counted
  twice.

## 5. API mode (optional, you pay Anthropic)

Switch **More → Assistant → Mode** to **API (your key)** and paste an
Anthropic API key. You get a chat inside Hearth (the sparkle button on the
Budget screen), a daily pass Hearth runs itself at the time you set, and
**Let Claude take a pass** that really does it now.

- **Models:** pick one for chat and one for the pass. Opus for chat and Haiku
  for the pass is the default; the pass is cheap, the chat is where quality
  shows.
- **Monthly cap:** Hearth stops calling Claude when the month's spend reaches
  it and raises an info alert. Spend and call counts show on the same page.
- **Data:** every call sends the context Claude needs, which is your
  household's budget data, to Anthropic's API under your key and Anthropic's
  API terms. Pull mode sends the same data through the connector, but only
  while you are chatting or the task runs. Nothing else leaves the box.

## 6. Troubleshooting

| Symptom | Likely cause |
| --- | --- |
| Connector says it can't reach the server | `APP_URL` host must be public over HTTPS. `curl -i https://<host>/mcp` should return `401`, not a proxy error. |
| `403` from `/mcp` | The `Host` header isn't in the allow-list. The default is the host of `APP_URL`; set `MCP_ALLOWED_HOSTS` if you reach Hearth under another name. |
| `401` from `/mcp` | Key missing, mistyped, or revoked. Create a new one. |
| Push toggle fails | Not HTTPS, not installed to the home screen (iPhone), or `APP_ENCRYPTION_KEY` unset. |
| Nothing is syncing | **More → Jobs** shows every job's last run, summary or error. `JOBS_ENABLED=false` turns the scheduler off on purpose. |
| Container restarts in a loop | `docker compose logs app`. Usual causes: `APP_SECRET` shorter than 32 characters, Plaid keys set without `APP_ENCRYPTION_KEY`, or Postgres not ready yet (it settles within a minute). |
| `429 Too Many Requests` | Sign-in allows 10 attempts per 5 minutes per address and `/mcp` 300 requests per minute. Behind a proxy without `TRUST_PROXY=true` every client shares the proxy's address. |
| Plaid link fails before Link opens | `APP_ENCRYPTION_KEY` unset, or `PLAID_ENV` doesn't match the key pair. |

The tool reference for the connector is in `docs/MCP.md`.
