#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REFINE_BIN="${REFINE_BIN:-${HOME}/.cargo/bin/refine}"
PROJECT_DIR="${SCRIPT_DIR}/.."
ENV_FILE="${PROJECT_DIR}/.env"
LOG_PREFIX="[refine-weekly]"
FAILED_STEPS=()

log() {
  echo "${LOG_PREFIX} $(date '+%Y-%m-%d %H:%M:%S') $*"
}

# 加载 .env 配置（不修改文件，仅 export 到当前进程）
if [[ -f "$ENV_FILE" ]]; then
  log "Loading env from ${ENV_FILE}"
  set -a
  # shellcheck source=/dev/null
  source "$ENV_FILE"
  set +a
else
  log "WARNING: env file not found: ${ENV_FILE}"
fi

# launchd does not inherit interactive shell variables; fall back to BASE_* from zsh.
# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
load_refine_llm_env

log "=== Weekly Insights Run Start ==="

# Preflight: environment diagnostics for troubleshooting
log "Preflight: PATH=$PATH"
log "Preflight: refine=$(command -v "$REFINE_BIN" 2>/dev/null && echo "$REFINE_BIN" || echo 'NOT FOUND')"
log "Preflight: cwd=$(pwd)"
log "Preflight: env REFINE_DB_PATH=${REFINE_DB_PATH:-<unset>} REFINE_ANTHROPIC_MODEL=${REFINE_ANTHROPIC_MODEL:-<unset>} REFINE_OPENAI_MODEL=${REFINE_OPENAI_MODEL:-<unset>} BASE_MODEL=${BASE_MODEL:-<unset>}"
log "Preflight: keys REFINE_ANTHROPIC_API_KEY=$([[ -n "${REFINE_ANTHROPIC_API_KEY:-}" ]] && echo '<set>' || echo '<unset>') REFINE_OPENAI_API_KEY=$([[ -n "${REFINE_OPENAI_API_KEY:-}" ]] && echo '<set>' || echo '<unset>') BASE_API_KEY=$([[ -n "${BASE_API_KEY:-}" ]] && echo '<set>' || echo '<unset>')"

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
