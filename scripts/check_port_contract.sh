#!/usr/bin/env bash
set -euo pipefail

stale_pattern='localhost:8787|127\.0\.0\.1:8787|localhost:5567|127\.0\.0\.1:5567|5568'

if grep -RInE \
  --exclude-dir=.git \
  --exclude-dir=.run \
  --exclude-dir=dist \
  --exclude-dir=node_modules \
  --exclude-dir=target \
  "$stale_pattern" apps docs CHANGELOG.md scripts/import_claude_code.sh scripts/eval_recommendations.mjs; then
  echo "Found stale local API port references. Use 21567 with fallback ports 21568..21570." >&2
  exit 1
fi

grep -q 'REFINE_SERVER_HOST' apps/server/README.md
grep -q 'REFINE_SERVER_PORT' apps/server/README.md
grep -q '127.0.0.1:21567' apps/server/README.md
grep -q '127.0.0.1:21567' docs/USAGE.md
grep -q '127.0.0.1:21567' docs/11_API_SPEC.md
grep -q '127.0.0.1:21567' scripts/import_claude_code.sh
grep -q '127.0.0.1:21567' scripts/eval_recommendations.mjs
grep -q 'DEFAULT_SERVER_PORT: u16 = 21567' apps/server/src/main.rs
grep -q 'DEFAULT_SERVER_PORT: u16 = 21567' apps/desktop/src-tauri/src/server/mod.rs
grep -q 'http://localhost:21567' apps/extension/package.json
grep -q 'http://127.0.0.1:21567' apps/desktop/ui/src/lib/api/adapters/http.ts
