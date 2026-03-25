#!/usr/bin/env bash
# Daily refresh: ingest new sessions → update mirror score + advice
set -euo pipefail

export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/Users/lifcc/.cargo/bin:$PATH"
cd /Users/lifcc/Desktop/code/AI/tools/refine

# Load .env for LLM API keys
if [ -f .env ]; then
  set -a
  source .env
  set +a
fi

echo "=== $(date) ==="

# 1. Ingest new sessions (capture exit code without aborting the script)
echo "Step 1: ingest-sessions"
if refine ingest-sessions 2>&1; then
  ingest_ok=1
else
  ingest_ok=0
  echo "⚠️  ingest-sessions reported failures; success timestamp will not be updated"
fi

# 2. Refresh mirror score + LLM advice (run regardless of ingest result)
echo "Step 2: mirror score"
mirror score 2>&1

echo "Done."

# Write success timestamp only when ingest had zero failures
if [ "$ingest_ok" -eq 1 ]; then
  mkdir -p ~/.refine
  date -u +%Y-%m-%dT%H:%M:%SZ > ~/.refine/last-refresh-ok
fi
