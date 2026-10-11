# Hearth — Setup

**Hearth — household money, on plan.**

One Docker container plus Postgres, behind your own reverse proxy. This guide
takes you from nothing to a running app with the Claude app connected, and
then covers keeping it running: updates, backups, logs, settings.
Claude-app menu labels below were checked against Anthropic's help center in
October 2026; if a label has moved, the idea is the same.

**Pick a path:**

| You have | Follow | Settings live in |
| --- | --- | --- |
| A Linux box, VM or NAS with Docker and Compose | [Docker host](#docker-host-with-a-env-file) | `.env` next to `docker-compose.yml` (or literals in the compose file) |
| TrueNAS SCALE | [TrueNAS SCALE](#truenas-scale) | the YAML you paste into the Apps screen |
| A reason to compile it yourself | [Build from source](#build-from-source) | `.env`, same as the Docker host |

**What it needs:** Docker Engine 24 or newer with the Compose plugin (v2.24+;
`docker compose version` shows it), about 300 MB of RAM for the app and
Postgres together, a gigabyte of disk to start, and `linux/amd64` or
`linux/arm64`. A Raspberry Pi 4 or any NAS from the last decade is enough.

## 1. Run it

You need Docker with Compose (or TrueNAS SCALE, below) and a hostname with
HTTPS that the household's phones can reach. HTTPS is required for the
installable app and push notifications, and the Claude app must be able to
reach the MCP endpoint over the public internet for the connector to work.

Hearth ships as a prebuilt image for `linux/amd64` and `linux/arm64` with
three channels: `ghcr.io/echoots1/hearth:latest` is the **release** channel
(it moves only when a version is released, and the compose files pull it),
`:qa` is the **QA** channel (it moves when a release candidate is cut, for
a second copy of the app you try things on before they are released), and
`:main` is the **development** build that follows every merge. A version
such as `:0.1.0`, a candidate such as `:0.2.0-rc.1`, or a commit's
`:sha-<short>` pins one build. Postgres 16 runs beside it. Nothing needs
compiling.

### Docker host with a `.env` file

```sh
git clone https://github.com/echoots1/Hearth.git && cd Hearth
cp .env.example .env
```

Edit `.env`. Only two lines matter on day one:

| Setting | What to put |
| --- | --- |
| `POSTGRES_PASSWORD` | Change it from the example. Any characters are fine; the app and the bundled Postgres both read it. |
| `PORT` | The host port to publish if 3000 is taken (the container always listens on 3000). |

Everything else is set inside the app after the first visit (**More →
Server** for the public URL, reverse proxy and schedules; **More →
Integrations** and **More → Notifications** for the feeds and channels), and
the session and encryption keys are generated on first start into the
`hearth-data` volume. The remaining variables in `.env.example` are optional
overrides for people who prefer the environment.

Then check the settings and start the stack:

```sh
scripts/preflight.sh            # checks the settings for the usual mistakes
docker compose up -d
scripts/preflight.sh            # now also probes /api/health and tells you what's next
```

`docker-compose.yml` reads `.env`; every `${VAR:-default}` in it can also be
replaced with a literal value if you would rather keep everything in the
compose file. The preflight reads whatever Compose resolves, so both ways get
checked.

Open `http://<host>:3000`: the first visit creates the owner account and
names the household. It asks for the **setup token**: while no account
exists, every start prints one line like `setup token: …` (`docker compose
logs app | grep "setup token"`), so a stranger who finds the address before
you do cannot become the owner. Then **More → Server** and set the **Public
URL** to what people will type (the `https://` address once you have one,
see below). Add people under **More → People**; the owner can remove them
there too.

To update later:

```sh
docker compose pull && docker compose up -d
```

### TrueNAS SCALE

TrueNAS installs Compose apps from a YAML editor and does not read a `.env`
file, so [`deploy/truenas/docker-compose.yml`](../deploy/truenas/docker-compose.yml)
has every setting written inline.

1. **Apps → Discover Apps**, then the **⋮** menu at the top right → **Install
   via YAML**.
2. Name the app `hearth`, paste the file, and edit the lines marked `CHANGE`:
   the database password (one line at the top, used by both services), and
   the host port if 3000 is taken (TrueNAS reports `port is already
   allocated` at install time; pick another, such as `3456`, and change only
   the left side of the `ports:` line).
3. **Install.** The app shows healthy once Postgres is up and Hearth has run
   its migrations. Open `http://<nas-ip>:<port>`, create the owner account
   with the setup token from **Installed → hearth → Logs** (the line that
   says `setup token`), then **More → Server** and set the **Public URL**.

Later edits to those values go through **Installed → hearth → Edit**, which
reopens the same YAML. Saving it also pulls the newest release (the file
sets `pull_policy: always` on the `latest` tag), so that is the update
button as well.

To load the demo household or run any other `hearth` command, open the
TrueNAS shell (**System → Shell**) and run it inside the app container:

```sh
docker exec -it "$(docker ps -q -f name=hearth-app)" hearth seed
```

The TrueNAS file also works unchanged on any Docker host:
`docker compose -f deploy/truenas/docker-compose.yml up -d`.

### Demo data

To look around with fake data first, load the demo household (it refuses a
database that already has users):

```sh
docker compose exec app hearth seed
```

Sign in as `owner@example.com` / `demo-password`. Wipe it later with
`hearth seed --force` on a fresh database, or just start over.

### Build from source

If you would rather not pull the published image, add the build override.
The first build compiles the Rust backend and takes a few minutes; later
builds reuse the cache.

```sh
docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

### Reverse proxy

Point your proxy at port `3000` (or `PORT`) and turn on **Behind a reverse
proxy** under **More → Server**. With that on, Hearth takes the client
address from the *last* `X-Forwarded-For` entry, the one your proxy appended
(both snippets below do that), so a client cannot spoof its way past the
sign-in rate limit. Keep the `Host` header intact: the same-origin check on
writes and the MCP host allow-list both compare against it. The
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

### Getting HTTPS

Plain `http://<lan-ip>:3000` is fine for setting up, loading the demo and
importing CSVs. Three things need a certificate the phones and Anthropic's
servers trust: installing the app to a home screen, push notifications, and
the Claude connector. A self-signed certificate satisfies none of them, so
Hearth doesn't generate one. Pick one of these instead; all three are apps on
TrueNAS and packages everywhere else:

- **Cloudflare Tunnel.** A public hostname with a valid certificate and no
  port forwarding. Run `cloudflared` beside Hearth (on TrueNAS, the
  Cloudflared app) and point the tunnel's public hostname at
  `http://hearth-app:3000` (or `http://<nas-ip>:3000`). Works with the Claude
  connector out of the box.
- **Tailscale.** `tailscale serve --bg 3000` gives you
  `https://<host>.<tailnet>.ts.net` with a trusted certificate for your own
  devices. For the Claude connector the endpoint must be public:
  `tailscale funnel --bg 3000`.
- **Caddy or Nginx Proxy Manager with Let's Encrypt.** The snippets above.
  If port 80 can't be opened, use the DNS challenge for your DNS provider.

Whichever you pick, open **More → Server**, set **Public URL** to the
resulting `https://` address and turn on **Behind a reverse proxy**. No
restart needed.

### Operate it

**Update.** The compose files pull `latest`, which moves only when a
release is tagged, so an update is always a published release and never a
half-finished merge. Each release is listed with its notes at
<https://github.com/echoots1/Hearth/releases>.

```sh
docker compose pull && docker compose up -d
```

On TrueNAS the YAML sets `pull_policy: always`, so **Installed → hearth →
Edit → Save** pulls the newest release and redeploys. Database migrations run
automatically when the new version starts; take a backup first (below),
because migrations are forward-only and the way back from a bad update is
the backup, not an older image.

**Run a QA copy.** A second, separate Hearth that pulls `qa` lets you try a
release candidate against a copy of your data before it reaches `latest`.
It is its own stack: own database, own keys, own port, own public URL (or
none). On a Docker host, a second compose project does it:

```sh
mkdir hearth-qa && cd hearth-qa
cp /path/to/Hearth/docker-compose.yml /path/to/Hearth/.env.example .
cp .env.example .env     # set POSTGRES_PASSWORD, HEARTH_TAG=qa, PORT=3001
docker compose -p hearth-qa up -d
```

The project name keeps its volumes (`hearth-qa_hearth-db`, …) apart from
production's. On TrueNAS, install the same YAML a second time as an app
named `hearth-qa` with `image: ghcr.io/echoots1/hearth:qa` and another host
port; TrueNAS scopes the volumes per app. To test against real data,
restore a production backup into the QA stack (below) rather than pointing
QA at the production database. Each candidate is listed as a pre-release at
<https://github.com/echoots1/Hearth/releases>; `docker compose -p hearth-qa
pull && docker compose -p hearth-qa up -d` moves QA to the newest one.

**Follow development instead.** `HEARTH_TAG=main` in `.env` (or `:main` on
the TrueNAS `image:` line) follows every merge to `main` that changes the
app; it is where things land before they are cut as a candidate.
Documentation and deploy-file changes do not rebuild it.

**Pin or roll back.** Every release is also tagged by version (`0.1.0`,
`0.1`) and every build by commit (`sha-<short>`, the first seven characters
of the commit it was built from). Set `HEARTH_TAG=0.1.0` or
`HEARTH_TAG=sha-abc1234` in `.env` (or edit the `image:` line on TrueNAS)
to stay on a known build. The tags are listed at
<https://github.com/echoots1/Hearth/pkgs/container/hearth>.

**Back up.** Everything lives in two volumes: `hearth-db` (Postgres) and
`hearth-data` (the generated session and encryption keys, two small files).
Without the encryption key the secrets in a dump (integration keys, Plaid
tokens, push keys, the Anthropic key) can't be read, so back up both. If you
set `APP_SECRET` and `APP_ENCRYPTION_KEY` yourself, keep a copy of your
`.env` or YAML instead.

```sh
docker compose exec -T db pg_dump -U hearth hearth > hearth-$(date +%F).sql
docker compose exec app sh -c 'cd /app/data && tar c .' > hearth-keys-$(date +%F).tar
```

Put that line in cron and copy the files off the box. On TrueNAS a ZFS
snapshot of the apps dataset covers the volume as well.

**Restore** onto fresh volumes:

```sh
docker compose down
docker volume rm hearth_hearth-db hearth_hearth-data   # the project name is the folder name; `docker volume ls` shows it
docker compose up -d db
docker compose exec -T db psql -U hearth hearth < hearth-2026-10-11.sql
docker compose up -d
docker compose exec -T app sh -c 'cd /app/data && tar x' < hearth-keys-2026-10-11.tar
docker compose restart app
```

**Logs and health.** `docker compose logs -f app` (TrueNAS: **Installed →
hearth → Workloads → logs**). `LOG_FORMAT=json` for a log shipper,
`LOG_LEVEL=debug,sqlx=warn` when something is odd. `GET /api/health` returns
`{"status":"ok"}` with `200` while the database answers and `503` otherwise;
point your uptime monitor at it. **More → Jobs** shows every scheduled job's
last run, summary or error.

**Start over.** `docker compose down -v` removes the containers and the
database volume (TrueNAS: **Installed → hearth → ⋮ → Delete** with the
volume). The first visit after `up -d` creates a new owner again, with the
fresh setup token from the log.

**Run a command inside the app:** `docker compose exec app hearth --help`
lists them (`seed`, `migrate`, `tools`). On TrueNAS use the shell:
`docker exec -it "$(docker ps -q -f name=hearth-app)" hearth --help`.

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
same, but the key then sits in the client's config, in your proxy's access
log and in any log the client keeps. Prefer the header form where you can,
give a key-in-URL connector a `read` key unless it must write, and revoke it
from **More → API keys** if that ever worries you. Hearth itself redacts the
key from its own log and sends `Referrer-Policy: no-referrer`.

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
- **ntfy.** **More → Notifications → ntfy**: server (ntfy.sh or your own),
  topic, and an access token if the topic is protected. **Send a test**
  confirms it.
- **Email.** **More → Notifications → Email**: SMTP host, port, username,
  password, from address and where alerts go. **Send a test email**
  confirms it.

Both are owner-only and stored encrypted in the database. The matching
environment variables (`NTFY_*`, `SMTP_*`, `ALERT_EMAIL_TO`) still work as a
fallback and show as "from environment"; saving in the app overrides them.

Quiet hours and the nag schedule live under **More → Settings**; a critical
alert can override quiet hours.

## 4. Getting transactions in

- **CSV** needs nothing: **More → Integrations → Import a CSV**, pick the
  account, upload the bank's export, map the columns once (save them as a
  preset), preview, import. Duplicates are skipped; every import can be
  rolled back.
- **Plaid.** **More → Integrations → Bank feeds**: paste the client ID and
  secret from the Plaid dashboard, pick sandbox or production, **Save**, then
  **Check the keys**. **Link a bank** opens Plaid Link (its script loads from
  Plaid's CDN only when you tap the button). Transactions and balances sync
  every `PLAID_SYNC_MINUTES`.
- **Privacy.com.** **More → Integrations → Virtual cards**: paste the API key,
  **Save**, **Check the key**. Cards become accounts, charges sync every
  `PRIVACY_SYNC_MINUTES`, declines become alerts, and the bank's later line for
  a card charge is linked so cash isn't counted twice.

Both are owner-only and stored encrypted (so `APP_ENCRYPTION_KEY` must be
set). The `PLAID_*` and `PRIVACY_*` environment variables still work as a
fallback; the app's copy wins when both exist, and **Remove** goes back to
the environment.

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
| `403` from `/mcp` | The `Host` header isn't in the allow-list. The default is the host of the public URL; add other names under **More → Server → Extra MCP host names**. |
| `401` from `/mcp` | Key missing, mistyped, or revoked. Create a new one. |
| Push toggle fails | The public URL is not HTTPS, or the app isn't installed to the home screen (iPhone). |
| Nothing is syncing | **More → Jobs** shows every job's last run, summary or error. `JOBS_ENABLED=false` turns the scheduler off on purpose. |
| `Bind for …:3000 failed: port is already allocated` | Another app on the host owns that port. Change the left side of the `ports:` line (for example `"3456:3000"`) and put the same port in `APP_URL` (`http://<host>:3456`) so links and the connector URL match. |
| `connecting to Postgres … invalid port number` (or `invalid URL`) | `DATABASE_URL` is set and its password contains `@ : / # %`. Unset `DATABASE_URL` and use `POSTGRES_PASSWORD`, which takes any characters, or percent-encode the password in the URL. |
| Container is `Up (unhealthy)` and connections are reset | The app is listening on another port inside the container, usually because `PORT` was changed to the host port. `docker logs` shows `hearth listening addr=0.0.0.0:NNNN`. Keep `PORT` at 3000 (or unset) and change only the left side of the `ports:` line. |
| Container restarts in a loop | `docker compose logs app`. Usual causes: a wrong database password, `APP_SECRET` set but shorter than 32 characters, the data directory not writable (mount the `hearth-data` volume or set the keys), or Postgres not ready yet (it settles within a minute). |
| `429 Too Many Requests` | Sign-in and setup allow 10 attempts per 5 minutes per address and `/mcp` 300 requests per minute. Behind a proxy without `TRUST_PROXY=true` every client shares the proxy's address. |
| `wrong setup token` on the first visit | The token is printed by the app container every time it starts with no accounts: `docker compose logs app \| grep "setup token"` (TrueNAS: **Installed → hearth → Logs**). Restarting the container prints a new one. |
| `cross-site request refused` | A write reached `/api` from another origin. Usually the proxy rewrote the `Host` header; pass it through unchanged (`proxy_set_header Host $host`). |
| Plaid webhooks answer `401` | Deliveries must carry Plaid's `Plaid-Verification` signature; only Plaid can produce it. A `404` means Plaid is not configured. |
| Push subscription refused | Browsers subscribe through their vendor's push service; Hearth refuses endpoints that look like a private host (an IP address, `localhost`, `.local`, `.lan`, `.internal`). |
| Plaid link fails before Link opens | The environment chosen under **Bank feeds** doesn't match the key pair; **Check the keys** shows Plaid's own error. |

The tool reference for the connector is in `docs/MCP.md`.

## 7. Settings reference

Every setting is an environment variable. `.env.example` is the canonical
list with the same defaults; nothing is read from any other file. Blank means
off for the optional sections.

**App**

Set these in the app under **More → Server** (owner only). The variables are
fallbacks: a value saved in the app wins, blank in the app means the
environment or the default applies.

| Variable | Default | Meaning |
| --- | --- | --- |
| `APP_URL` | `http://localhost:3000` | Public URL. `https` makes cookies Secure; notification links and the connector URL are built from it. In the app: **Public URL**. |
| `TRUST_PROXY` | `false` | Trust `X-Forwarded-For` for the client address (rate limits); the last entry, which the proxy appended, is used. In the app: **Behind a reverse proxy**. |
| `MCP_ALLOWED_HOSTS` | derived from the public URL + localhost | Comma-separated `Host` values the MCP endpoint accepts. Setting it replaces the derived list; **Extra MCP host names** in the app adds to either. |
| `PORT` | `3000` | Host port Compose publishes. The container always listens on 3000. |
| `HEARTH_TAG` | `latest` | Image tag Compose pulls: `latest` (releases), `qa` (release candidates), `main` (every merge), a version such as `0.1.0` or `0.2.0-rc.1`, or `sha-<short>`. |
| `APP_SECRET` | generated | 32+ random characters. Signs sessions and one-tap notification links. Generated on first start into `HEARTH_DATA_DIR` when unset; changing it signs everyone out. |
| `APP_ENCRYPTION_KEY` | generated | 32+ random characters. Encrypts everything secret at rest; the actual key is derived with argon2id and a per-install salt kept in the database, so a passphrase you typed yourself is not brute-forceable from a dump. Generated on first start into `HEARTH_DATA_DIR` when unset; changing it means re-entering the secrets. |
| `HEARTH_DATA_DIR` | `/app/data` in the image, `./data` otherwise | Where generated keys are kept. The compose files mount the `hearth-data` volume there. |
| `LOG_LEVEL` | `info,sqlx=warn` | Tracing filter. |
| `LOG_FORMAT` | `text` | `json` for structured logs. |

**Database**

| Variable | Default | Meaning |
| --- | --- | --- |
| `POSTGRES_HOST` / `POSTGRES_PORT` | `db` / `5432` | Where Postgres is. The defaults point at the bundled `db` service. |
| `POSTGRES_USER` / `POSTGRES_PASSWORD` / `POSTGRES_DB` | `hearth` / required / `hearth` | Credentials for the app and the bundled Postgres. The password can contain any characters. |
| `DATABASE_URL` | blank | Full `postgres://` URL for an external database. Overrides the `POSTGRES_*` values when set. A password with `@ : / # %` must be percent-encoded here. |

**Jobs**

| Variable | Default | Meaning |
| --- | --- | --- |
| `JOBS_ENABLED` | `true` | Run the scheduler in this process. `false` for a web-only container. |
| `SCHEDULER_TICK_SECONDS` | `30` | How often the scheduler looks for due jobs. |
| `EVALUATE_MINUTES` | `15` | Alert evaluation interval. In the app: **More → Server → Schedules**. |
| `PLAID_SYNC_MINUTES` | `60` | Plaid transaction sync interval. Also under **Schedules**. |
| `PRIVACY_SYNC_MINUTES` | `10` | Privacy.com sync interval. Also under **Schedules**. |

**Integrations (all optional, and all configurable in the app instead)**

Every variable here can be left unset and entered under **More →
Integrations** or **More → Notifications**, where it is stored encrypted.
The environment is a fallback for people who prefer it; a value saved in the
app takes precedence.

| Variable | Default | Meaning |
| --- | --- | --- |
| `PLAID_CLIENT_ID` / `PLAID_SECRET` | blank | Enables Plaid when both are set. Needs `APP_ENCRYPTION_KEY`. |
| `PLAID_ENV` | `sandbox` | `sandbox`, `development` or `production`; must match the key pair. |
| `PLAID_PRODUCTS` | `transactions` | Products requested at link time. |
| `PLAID_COUNTRY_CODES` | `US` | Countries offered in Plaid Link. |
| `PRIVACY_API_KEY` | blank | Enables Privacy.com card sync. |
| `PRIVACY_ENV` | `sandbox` | `sandbox` or `production`. |
| `NTFY_TOPIC` | blank | Enables ntfy notifications to this topic. |
| `NTFY_URL` | `https://ntfy.sh` | Self-hosted ntfy server. |
| `NTFY_TOKEN` | blank | Access token if the topic needs one. |
| `SMTP_HOST` | blank | Enables email notifications. |
| `SMTP_PORT` | `587` | STARTTLS port. |
| `SMTP_USER` / `SMTP_PASS` | blank | SMTP credentials, if any. |
| `SMTP_FROM` | required with SMTP | Sender address. |
| `ALERT_EMAIL_TO` | required with SMTP | Where alerts go. |

API mode (the Anthropic key, models, pass time, monthly cap) is configured in
the app under **More → Assistant**, not in the environment, so that the key
lives encrypted in the database and non-owners never see it.

