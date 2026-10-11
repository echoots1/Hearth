#!/usr/bin/env bash
# Regenerate the tool reference inside docs/MCP.md from the registry.
set -euo pipefail
cd "$(dirname "$0")/.."
tmp="$(mktemp)"
cargo run -q -p hearth-server -- tools > "$tmp"
python3 - "$tmp" <<'PY'
import sys, re
gen = open(sys.argv[1]).read().rstrip("\n")
path = "docs/MCP.md"
doc = open(path).read()
begin, end = "<!-- BEGIN GENERATED: hearth tools -->", "<!-- END GENERATED -->"
i, j = doc.index(begin), doc.index(end) + len(end)
open(path, "w").write(doc[:i] + gen + doc[j:])
PY
rm -f "$tmp"
echo "docs/MCP.md updated"
