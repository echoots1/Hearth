# Contributing to Hearth

Thanks for helping. Hearth is small on purpose; keep it that way.

- **Read first:** `docs/ARCHITECTURE.md` and `docs/DATA-MODEL.md` are the
  source of truth. A change that touches the data model updates the doc and
  adds a migration in the same PR. Anything undecided goes in
  `docs/OPEN-QUESTIONS.md` with a proposed default.
- **Money is integer cents, dates are ISO strings in the household timezone.**
  No floats, ever. Clippy enforces `float_arithmetic = deny`.
- **Jobs detect, the AI decides.** Don't add judgment calls to jobs.
- **Nothing personal in the repo.** Seed and test data are obviously fake.
- **Before you push:** `cargo fmt --all`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace`, and in `web/`: `npm run lint && npm run build`.
- **Commits:** small, labeled (`core:`, `api:`, `mcp:`, `jobs:`, `web:`,
  `docs:`, `plaid:`, `privacy:`, `notify:`), one concern each.
- **PRs:** describe what changed and how you tested it. CI must be green.
- **Branches and releases:** work happens on `main` through PRs; `main`
  always builds and publishes the `:main` development image. Production
  follows `:latest`, which moves only when a version is tagged: bump the
  version in `Cargo.toml` and `web/package.json` in a PR, merge it, then run
  `scripts/release.sh X.Y.Z`. `CLAUDE.md` has the full checklist.
