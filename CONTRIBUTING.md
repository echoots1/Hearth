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
- **Releases:** none until features are close. `main` always builds.
