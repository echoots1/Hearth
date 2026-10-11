#!/usr/bin/env bash
# Pre-deploy checks for a Hearth instance, then a health probe if it is running.
#
#   scripts/preflight.sh            # check .env, then probe the running stack
#
# Exit code is non-zero when something must be fixed before `docker compose up`.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
ok()   { printf '  ok    %s\n' "$1"; }
warn() { printf '  warn  %s\n' "$1"; }
bad()  { printf '  FAIL  %s\n' "$1"; fail=1; }

echo "Hearth preflight"

# ── tooling ───────────────────────────────────────────────────────────────
if command -v docker >/dev/null 2>&1; then
  ok "docker: $(docker --version | sed 's/,.*//')"
else
  bad "docker is not installed"
fi
if docker compose version >/dev/null 2>&1; then
  ok "docker compose: $(docker compose version --short 2>/dev/null || echo present)"
else
  bad "docker compose plugin is missing"
fi

# ── .env ──────────────────────────────────────────────────────────────────
if [ ! -f .env ]; then
  bad ".env is missing (cp .env.example .env, then edit it)"
  echo; echo "Fix the items marked FAIL, then run again."; exit 1
fi
ok ".env present"

# Read KEY=VALUE lines without executing them.
getenv() { sed -n "s/^[[:space:]]*$1=//p" .env | tail -1 | sed 's/^"\(.*\)"$/\1/'; }

APP_URL="$(getenv APP_URL)"
APP_SECRET="$(getenv APP_SECRET)"
APP_ENCRYPTION_KEY="$(getenv APP_ENCRYPTION_KEY)"
POSTGRES_PASSWORD="$(getenv POSTGRES_PASSWORD)"
TRUST_PROXY="$(getenv TRUST_PROXY)"
PORT="$(getenv PORT)"; PORT="${PORT:-3000}"

case "$APP_URL" in
  "")        bad "APP_URL is empty" ;;
  https://*) ok "APP_URL=$APP_URL" ;;
  http://localhost*|http://127.0.0.1*)
             warn "APP_URL=$APP_URL (fine for a local try; the installable app, push and the Claude connector need https)" ;;
  http://*)  warn "APP_URL=$APP_URL is plain http: cookies won't be Secure, push and the connector won't work" ;;
  *)         bad "APP_URL=$APP_URL must start with http:// or https://" ;;
esac

if [ -z "$APP_SECRET" ]; then
  bad "APP_SECRET is empty (openssl rand -base64 48)"
elif [ "$APP_SECRET" = "change-me-to-a-long-random-string-at-least-32-chars" ]; then
  bad "APP_SECRET is still the example value"
elif [ "${#APP_SECRET}" -lt 32 ]; then
  bad "APP_SECRET is shorter than 32 characters"
else
  ok "APP_SECRET set (${#APP_SECRET} chars)"
fi

if [ -z "$APP_ENCRYPTION_KEY" ]; then
  warn "APP_ENCRYPTION_KEY is empty: push notifications, Plaid and API mode stay off until it is set (set it now; rotating later means re-linking)"
elif [ "${#APP_ENCRYPTION_KEY}" -lt 32 ]; then
  bad "APP_ENCRYPTION_KEY is shorter than 32 characters"
elif [ "$APP_ENCRYPTION_KEY" = "$APP_SECRET" ]; then
  warn "APP_ENCRYPTION_KEY equals APP_SECRET; use two different random strings"
else
  ok "APP_ENCRYPTION_KEY set"
fi

if [ -z "$POSTGRES_PASSWORD" ] || [ "$POSTGRES_PASSWORD" = "hearth" ]; then
  warn "POSTGRES_PASSWORD is the default; Postgres is not published outside the compose network, but change it anyway"
else
  ok "POSTGRES_PASSWORD changed from the default"
fi

case "$APP_URL" in
  https://*)
    if [ "$TRUST_PROXY" = "true" ]; then ok "TRUST_PROXY=true (behind a reverse proxy)"
    else warn "APP_URL is https but TRUST_PROXY is not true; rate limits will see the proxy's address, not the client's"; fi ;;
esac

if grep -Eq '^[[:space:]]*(PLAID_CLIENT_ID|PLAID_SECRET)=.+' .env && [ -z "$APP_ENCRYPTION_KEY" ]; then
  bad "Plaid is configured but APP_ENCRYPTION_KEY is empty (the server refuses to start)"
fi

if [ "$fail" -ne 0 ]; then
  echo; echo "Fix the items marked FAIL, then run again."; exit 1
fi

# ── running stack ─────────────────────────────────────────────────────────
echo
if ! docker compose ps --status running --services 2>/dev/null | grep -qx app; then
  echo "Stack is not running. Next:"
  echo "  docker compose up -d --build"
  echo "  scripts/preflight.sh          # again, to probe health"
  exit 0
fi

health="$(curl -fsS --max-time 5 "http://127.0.0.1:${PORT}/api/health" 2>/dev/null)"
if [ -n "$health" ]; then
  ok "app healthy on :${PORT}: $health"
else
  bad "app is running but http://127.0.0.1:${PORT}/api/health did not answer (docker compose logs app)"
  exit 1
fi

status="$(curl -fsS --max-time 5 "http://127.0.0.1:${PORT}/api/auth/status" 2>/dev/null)"
case "$status" in
  *'"needs_setup":true'*)  echo; echo "Ready. Open ${APP_URL} to create the owner account,"
                            echo "or load the demo first: docker compose exec app hearth seed" ;;
  *'"needs_setup":false'*) echo; echo "Ready. Owner account exists; open ${APP_URL} to sign in." ;;
  *)                        warn "could not read /api/auth/status" ;;
esac
