#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${HOME}/.cargo/bin:${PATH:-}"
REFINE_BIN="${REFINE_BIN:-${HOME}/.cargo/bin/refine}"
LOG_PREFIX="[refine-weekly]"
FAILED_STEPS=()

log() {
  echo "${LOG_PREFIX} $(date '+%Y-%m-%d %H:%M:%S') $*"
}

# Daily ingestion can run for hours on a backlog. Serialize whole scheduled
# workflows so weekly analysis never competes for SQLite or LLM capacity.
# shellcheck source=scripts/runtime-job-lock.sh
source "${SCRIPT_DIR}/runtime-job-lock.sh"
if [[ "${REFINE_RUNTIME_LOCK_ACTIVE:-}" != "1" ]]; then
  run_refine_runtime_job_locked "${SCRIPT_DIR}/weekly-insights.sh" "$@"
  exit $?
fi

log "=== Weekly Insights Run Start ==="

# Unattended LLM work uses process credentials or the canonical secure user
# file. It does not depend on a project checkout or source ~/.zshrc.
# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
if ! load_refine_llm_env; then
  log "ERROR: unattended LLM credentials are unavailable; refusing to start ingest"
  exit 1
fi

# Preflight: environment diagnostics for troubleshooting
log "Preflight: PATH=$PATH"
log "Preflight: refine=$(command -v "$REFINE_BIN" 2>/dev/null && echo "$REFINE_BIN" || echo 'NOT FOUND')"
log "Preflight: cwd=$(pwd)"
log "Preflight: LLM source=${REFINE_LLM_ENV_SOURCE:-none} $(refine_llm_env_status)"

# Step 1: import a bounded newest-session window from the sole Remem source.
log "Step 1: ingest-sessions"
REFINE_INGEST_LATEST=${REFINE_INGEST_LATEST:-80}
if ! [[ "$REFINE_INGEST_LATEST" =~ ^[1-9][0-9]*$ ]]; then
  log "ERROR: REFINE_INGEST_LATEST must be a positive integer"
  exit 1
fi
if ! "$REFINE_BIN" ingest-sessions --latest "$REFINE_INGEST_LATEST" 2>&1; then
  log "ERROR: ingest-sessions failed; refusing to generate derived insights"
  exit 1
fi
log "Step 1: ingest-sessions completed successfully"

# Step 2: 生成当前 7 天与前一等长 7 天的 delta 处方报告。
# 全历史输出只能由人工显式执行 `refine insights --all`。
log "Step 2: insights --period 7 --prescription"
insights_rc=0
"$REFINE_BIN" insights --period 7 --prescription 2>&1 || insights_rc=$?
if [[ "$insights_rc" -eq 0 ]]; then
  log "Step 2: insights --period 7 --prescription completed successfully"
else
  log "ERROR: Step 2 insights --period 7 --prescription failed with exit code ${insights_rc}"
  FAILED_STEPS+=("insights --period 7 --prescription")
fi

# Step 3: 发送与真实状态一致的 macOS 通知
log "Step 3: notification"
if [[ ${#FAILED_STEPS[@]} -eq 0 ]]; then
  osascript -e 'display notification "Weekly insights 报告已生成" with title "Refine Weekly Insights"' 2>&1 || true
else
  osascript -e "display notification \"失败步骤: ${FAILED_STEPS[*]}，详见 refine-insights.log\" with title \"Refine Weekly Insights 失败\"" 2>&1 || true
fi

log "=== Weekly Insights Run End ==="

if [[ ${#FAILED_STEPS[@]} -gt 0 ]]; then
  log "ERROR: run finished with failures: ${FAILED_STEPS[*]}"
  exit 1
fi
