# syntax=docker/dockerfile:1.7
# ── 1. Web UI ──────────────────────────────────────────────────────────────
FROM node:22-alpine AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# ── 2. Rust build with cargo-chef for dependency caching ───────────────────
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /web/dist ./web/dist
RUN cargo build --release --bin hearth && strip target/release/hearth

# ── 3. Runtime ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /app hearth \
 && mkdir -p /app/data && chown hearth:hearth /app/data
WORKDIR /app
COPY --from=builder /app/target/release/hearth /usr/local/bin/hearth
USER hearth
# Generated APP_SECRET / APP_ENCRYPTION_KEY live here when not provided; mount a volume.
ENV PORT=3000 HEARTH_DATA_DIR=/app/data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s \
  CMD curl -fsS http://localhost:3000/api/health || exit 1
ENTRYPOINT ["hearth"]
CMD ["serve"]
