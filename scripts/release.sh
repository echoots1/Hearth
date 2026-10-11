#!/usr/bin/env bash
# Cut a release: tag the commit at the tip of main as vX.Y.Z.
#
#   scripts/release.sh 0.2.0
#
# The version bump itself goes through a normal PR first (Cargo.toml
# [workspace.package] version and web/package.json "version"), so CI has
# checked the exact commit being released. This script only verifies and
# tags; the tag push makes the Image workflow publish ghcr.io/<owner>/hearth
# as :X.Y.Z, :X.Y and :latest and create the GitHub Release.
#
# Without push rights to tags (cloud agent sessions, for one) dispatch the
# workflow instead; it does the same checks and creates the tag itself:
#   gh workflow run image.yml --ref main -f release=X.Y.Z
set -euo pipefail
cd "$(dirname "$0")/.."

fail() { printf 'release: %s\n' "$1" >&2; exit 1; }

v="${1:-}"
v="${v#v}"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "usage: scripts/release.sh X.Y.Z"

[[ "$(git rev-parse --abbrev-ref HEAD)" == "main" ]] || fail "switch to main first"
git diff --quiet && git diff --cached --quiet || fail "the working tree has changes; commit or stash them"
git fetch origin main -q
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] \
  || fail "main is not at origin/main; pull (or push) first"

cargo_v="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)"
npm_v="$(python3 -c 'import json; print(json.load(open("web/package.json"))["version"])')"
[[ "$cargo_v" == "$v" && "$npm_v" == "$v" ]] \
  || fail "Cargo.toml says $cargo_v and web/package.json says $npm_v; bump both to $v in a PR and merge it first"

git rev-parse -q --verify "refs/tags/v$v" >/dev/null && fail "tag v$v already exists"
git ls-remote --exit-code --tags origin "v$v" >/dev/null 2>&1 && fail "tag v$v already exists on origin"

if command -v gh >/dev/null 2>&1; then
  state="$(gh run list --workflow CI --commit "$(git rev-parse HEAD)" --json conclusion -q '.[0].conclusion' 2>/dev/null || true)"
  case "$state" in
    success) echo "CI is green on $(git rev-parse --short HEAD)" ;;
    "") echo "warning: no CI run found for this commit (docs-only merges skip CI); make sure the last code change was green" ;;
    *) fail "CI on $(git rev-parse --short HEAD) is '$state'; release a green commit" ;;
  esac
fi

git tag -a "v$v" -m "Hearth v$v"
git push origin "v$v"
echo "tagged v$v; the Image workflow publishes :$v, :${v%.*} and :latest and creates the GitHub Release:"
echo "  https://github.com/$(git remote get-url origin | sed -E 's#.*github.com[:/]##; s#\.git$##')/actions"
