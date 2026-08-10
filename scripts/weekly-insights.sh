#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REFINE_BIN="${REFINE_BIN:-${HOME}/.cargo/bin/refine}"
PROJECT_DIR="${SCRIPT_DIR}/.."
ENV_FILE="${PROJECT_DIR}/.env"
LOG_PREFIX="[refine-weekly]"

# 失败步骤记账：任一步骤失败都必须体现在通知文案和退出码里（U-29），
# 不能像旧实现那样把非零退出降级成一行普通 log 后照常弹"已生成"。
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

# ingest 判定 / 日志轮转的共享函数
# shellcheck source=scripts/ingest-lib.sh
source "${SCRIPT_DIR}/ingest-lib.sh"

rotate_log_if_needed "${REFINE_INSIGHTS_LOG:-$HOME/Library/Logs/refine-insights.log}"

log "=== Weekly Insights Run Start ==="

# Preflight: environment diagnostics for troubleshooting
log "Preflight: PATH=$PATH"
log "Preflight: refine=$(command -v "$REFINE_BIN" 2>/dev/null && echo "$REFINE_BIN" || echo 'NOT FOUND')"
log "Preflight: cwd=$(pwd)"
log "Preflight: env REFINE_DB_PATH=${REFINE_DB_PATH:-<unset>} REFINE_ANTHROPIC_MODEL=${REFINE_ANTHROPIC_MODEL:-<unset>} REFINE_OPENAI_MODEL=${REFINE_OPENAI_MODEL:-<unset>} BASE_MODEL=${BASE_MODEL:-<unset>}"
log "Preflight: keys REFINE_ANTHROPIC_API_KEY=$([[ -n "${REFINE_ANTHROPIC_API_KEY:-}" ]] && echo '<set>' || echo '<unset>') REFINE_OPENAI_API_KEY=$([[ -n "${REFINE_OPENAI_API_KEY:-}" ]] && echo '<set>' || echo '<unset>') BASE_API_KEY=$([[ -n "${BASE_API_KEY:-}" ]] && echo '<set>' || echo '<unset>')"

# Step 1: 增量导入新会话
# ingest 失败仍继续跑 insights（ingest 可续传），但必须记账并影响最终退出码。
log "Step 1: ingest-sessions"
INGEST_OUT=$(mktemp "${TMPDIR:-/tmp}/refine-ingest.XXXXXX")
trap 'rm -f "$INGEST_OUT"' EXIT
rc=0
"$REFINE_BIN" ingest-sessions >"$INGEST_OUT" 2>&1 || rc=$?
cat "$INGEST_OUT"
if evaluate_ingest_result "$INGEST_OUT" "$rc"; then
  log "Step 1: ingest-sessions completed within failure threshold"
else
  log "ERROR: Step 1 ingest-sessions failure rate exceeded threshold (raw exit code ${rc})"
  FAILED_STEPS+=("ingest-sessions")
fi

# Step 2: 生成处方报告
log "Step 2: insights --prescription"
rc=0
"$REFINE_BIN" insights --prescription 2>&1 || rc=$?
if [[ "$rc" -eq 0 ]]; then
  log "Step 2: insights --prescription completed successfully"
else
  log "ERROR: Step 2 insights --prescription failed with exit code ${rc}"
  FAILED_STEPS+=("insights --prescription")
fi

# Step 3: 发送 macOS 通知（按真实结果分叉，禁止无条件报成功）
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
