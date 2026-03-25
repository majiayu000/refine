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

# 3. Weekly report on Sundays — generates ~/.mirror/last-weekly.md for Monday MOTD reminder.
# Non-fatal: weekly requires LLM API access and may fail without network/key.
DOW=$(date +%u)  # 1=Monday … 7=Sunday
if [ "$DOW" = "7" ]; then
  echo "Step 3: mirror weekly (Sunday)"
  mirror weekly 2>&1 || echo "Step 3: mirror weekly failed (non-fatal)"
fi

echo "Done."
