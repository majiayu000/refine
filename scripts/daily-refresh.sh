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

# 1. Ingest new sessions
echo "Step 1: ingest-sessions"
refine ingest-sessions 2>&1

# 2. Refresh mirror score + LLM advice
echo "Step 2: mirror score"
mirror score 2>&1

echo "Done."

# Write success timestamp for cognitive-reminder staleness check
mkdir -p ~/.refine
date -u +%Y-%m-%dT%H:%M:%SZ > ~/.refine/last-refresh-ok
