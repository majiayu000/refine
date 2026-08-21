#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REFINE_BIN="${REFINE_BIN:-${HOME}/.cargo/bin/refine}"
LOG_PREFIX="[refine-weekly]"
FAILED_STEPS=()

log() {
  echo "${LOG_PREFIX} $(date '+%Y-%m-%d %H:%M:%S') $*"
}

# Unattended jobs use process credentials or the canonical secure user file.
# They do not depend on a project checkout or source ~/.zshrc.
# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
if ! load_refine_llm_env; then
  log "ERROR: unattended LLM credentials are unavailable; refusing to start ingest"
  exit 1
fi

# Daily ingestion can run for hours on a backlog. Serialize whole scheduled
# workflows so weekly analysis never competes for SQLite or LLM capacity.
# shellcheck source=scripts/runtime-job-lock.sh
source "${SCRIPT_DIR}/runtime-job-lock.sh"
if [[ "${REFINE_RUNTIME_LOCK_ACTIVE:-}" != "1" ]]; then
  run_refine_runtime_job_locked "${SCRIPT_DIR}/weekly-insights.sh" "$@"
  exit $?
fi

log "=== Weekly Insights Run Start ==="

# Preflight: environment diagnostics for troubleshooting
log "Preflight: PATH=$PATH"
log "Preflight: refine=$(command -v "$REFINE_BIN" 2>/dev/null && echo "$REFINE_BIN" || echo 'NOT FOUND')"
log "Preflight: cwd=$(pwd)"
log "Preflight: LLM source=${REFINE_LLM_ENV_SOURCE:-none} $(refine_llm_env_status)"

# Step 1: 增量导入新会话
log "Step 1: ingest-sessions"
ingest_rc=0
"$REFINE_BIN" ingest-sessions 2>&1 || ingest_rc=$?
if [[ "$ingest_rc" -eq 0 ]]; then
  log "Step 1: ingest-sessions completed successfully"
else
  log "ERROR: Step 1 ingest-sessions failed with exit code ${ingest_rc}"
  FAILED_STEPS+=("ingest-sessions")
fi

# Step 2: 生成处方报告
log "Step 2: insights --prescription"
insights_rc=0
"$REFINE_BIN" insights --prescription 2>&1 || insights_rc=$?
if [[ "$insights_rc" -eq 0 ]]; then
  log "Step 2: insights --prescription completed successfully"
else
  log "ERROR: Step 2 insights --prescription failed with exit code ${insights_rc}"
  FAILED_STEPS+=("insights --prescription")
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
