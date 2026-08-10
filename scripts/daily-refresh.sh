#!/usr/bin/env bash
# Daily refresh: ingest new sessions → update mirror score + advice
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${HOME}/.cargo/bin:$PATH"
cd "${SCRIPT_DIR}/.."

# Load .env for LLM API keys
if [ -f .env ]; then
  set -a
  # shellcheck source=/dev/null
  source .env
  set +a
fi

# launchd does not inherit interactive shell variables; fall back to BASE_* from zsh.
# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
load_refine_llm_env

QUOTA_FILE="$HOME/.refine/quota_exhausted_until"
if [ -f "$QUOTA_FILE" ]; then
  UNTIL=$(cat "$QUOTA_FILE")
  NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  if [[ "$UNTIL" > "$NOW" ]]; then
    echo "LLM quota exhausted until $UNTIL — skipping refresh"
    exit 0
  fi
fi

echo "=== $(date) ==="

FAILED_STEPS=()

# Preflight: environment diagnostics for troubleshooting
echo "Preflight: PATH=$PATH"
echo "Preflight: refine=$(command -v refine) mirror=$(command -v mirror)"
echo "Preflight: cwd=$(pwd)"
echo "Preflight: env REFINE_DB_PATH=${REFINE_DB_PATH:-<unset>} REFINE_ANTHROPIC_MODEL=${REFINE_ANTHROPIC_MODEL:-<unset>} REFINE_OPENAI_MODEL=${REFINE_OPENAI_MODEL:-<unset>} BASE_MODEL=${BASE_MODEL:-<unset>}"
echo "Preflight: keys REFINE_ANTHROPIC_API_KEY=$([ -n "${REFINE_ANTHROPIC_API_KEY:-}" ] && echo '<set>' || echo '<unset>') REFINE_OPENAI_API_KEY=$([ -n "${REFINE_OPENAI_API_KEY:-}" ] && echo '<set>' || echo '<unset>') BASE_API_KEY=$([ -n "${BASE_API_KEY:-}" ] && echo '<set>' || echo '<unset>')"

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
score_rc=0
mirror score 2>&1 || score_rc=$?
if [ "$score_rc" -ne 0 ]; then
  echo "ERROR: Step 2 mirror score failed with exit code ${score_rc}" >&2
  FAILED_STEPS+=("mirror score")
fi

# 3. Weekly report on Sundays — generates ~/.mirror/last-weekly.md for Monday MOTD reminder.
# A missing weekly report is user-visible and must affect the final status.
DOW=$(date +%u)  # 1=Monday … 7=Sunday
if [ "$DOW" = "7" ]; then
  echo "Step 3: mirror weekly (Sunday)"
  weekly_rc=0
  mirror weekly 2>&1 || weekly_rc=$?
  if [ "$weekly_rc" -ne 0 ]; then
    echo "ERROR: Step 3 mirror weekly failed with exit code ${weekly_rc}" >&2
    FAILED_STEPS+=("mirror weekly")
  fi
fi

echo "Step 4: wal checkpoint"
db_path="${REFINE_DB_PATH:-$HOME/Library/Application Support/refine/refine.db}"
if command -v sqlite3 >/dev/null 2>&1 && [ -f "$db_path" ]; then
  if ! sqlite3 "$db_path" 'PRAGMA wal_checkpoint(TRUNCATE);' >/dev/null; then
    echo "WARN: WAL checkpoint failed: $db_path" >&2
  fi
else
  echo "WARN: WAL checkpoint skipped: sqlite3 or database missing" >&2
fi

echo "Done."

# Write success timestamp only when ingest had zero failures
if [ "$ingest_ok" -eq 1 ]; then
  mkdir -p ~/.refine
  date -u +%Y-%m-%dT%H:%M:%SZ > ~/.refine/last-refresh-ok
fi

if [ "$ingest_ok" -eq 0 ]; then
  FAILED_STEPS+=("ingest-sessions")
fi

# Propagate every user-visible failure to launchd/cron monitoring.
if [ ${#FAILED_STEPS[@]} -gt 0 ]; then
  echo "ERROR: run finished with failures: ${FAILED_STEPS[*]}" >&2
  exit 1
fi
