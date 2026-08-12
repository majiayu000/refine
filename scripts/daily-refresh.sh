#!/usr/bin/env bash
# Daily refresh: ingest new sessions → update mirror score + advice
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_DIR="${SCRIPT_DIR}/.."
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${HOME}/.cargo/bin:${PATH:-}"
cd "$PROJECT_DIR"

# The loader applies process -> secure user file -> explicit project fallback.
# It never sources ~/.zshrc or evaluates either env file.
# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
if ! load_refine_llm_env "${PROJECT_DIR}/.env"; then
  echo "ERROR: unattended LLM credentials are unavailable; refusing to start ingest" >&2
  exit 1
fi

# shellcheck source=scripts/runtime-job-lock.sh
source "${SCRIPT_DIR}/runtime-job-lock.sh"
acquire_refine_runtime_job_lock
trap release_refine_runtime_job_lock EXIT
trap 'release_refine_runtime_job_lock; exit 129' HUP
trap 'release_refine_runtime_job_lock; exit 130' INT
trap 'release_refine_runtime_job_lock; exit 143' TERM

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
echo "Preflight: LLM source=${REFINE_LLM_ENV_SOURCE:-none} $(refine_llm_env_status)"

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
mirror score --require-advice 2>&1 || score_rc=$?
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
if [ -n "${REFINE_DB_PATH:-}" ]; then
  db_path="$REFINE_DB_PATH"
elif [ "$(uname -s)" = "Darwin" ]; then
  db_path="$HOME/Library/Application Support/refine/refine.db"
else
  db_path="${XDG_DATA_HOME:-$HOME/.local/share}/refine/refine.db"
fi
if command -v sqlite3 >/dev/null 2>&1 && [ -f "$db_path" ]; then
  if ! sqlite3 "$db_path" 'PRAGMA wal_checkpoint(TRUNCATE);' >/dev/null; then
    echo "WARN: WAL checkpoint failed: $db_path" >&2
  fi
else
  echo "WARN: WAL checkpoint skipped: sqlite3 or database missing" >&2
fi

echo "Done."

if [ "$ingest_ok" -eq 0 ]; then
  FAILED_STEPS+=("ingest-sessions")
fi

# This marker represents the complete scheduled refresh, not ingestion alone.
if [ ${#FAILED_STEPS[@]} -eq 0 ]; then
  mkdir -p ~/.refine
  date -u +%Y-%m-%dT%H:%M:%SZ > ~/.refine/last-refresh-ok
fi

# Propagate every user-visible failure to launchd/cron monitoring.
if [ ${#FAILED_STEPS[@]} -gt 0 ]; then
  echo "ERROR: run finished with failures: ${FAILED_STEPS[*]}" >&2
  exit 1
fi
