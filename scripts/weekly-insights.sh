#!/usr/bin/env bash
set -euo pipefail

REFINE_BIN="${HOME}/.cargo/bin/refine"
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${PROJECT_DIR}/.env"
LOG_PREFIX="[refine-weekly]"

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

log "=== Weekly Insights Run Start ==="

# Preflight: environment diagnostics for troubleshooting
log "Preflight: PATH=$PATH"
log "Preflight: refine=$(command -v "$REFINE_BIN" 2>/dev/null && echo "$REFINE_BIN" || echo 'NOT FOUND')"
log "Preflight: cwd=$(pwd)"
log "Preflight: env REFINE_DB_PATH=${REFINE_DB_PATH:-<unset>} REFINE_ANTHROPIC_MODEL=${REFINE_ANTHROPIC_MODEL:-<unset>}"

# Step 1: 增量导入新会话
log "Step 1: ingest-sessions"
if "$REFINE_BIN" ingest-sessions 2>&1; then
  log "Step 1: ingest-sessions completed successfully"
else
  log "Step 1: ingest-sessions failed with exit code $?"
fi

# Step 2: 生成处方报告
log "Step 2: insights --prescription"
if "$REFINE_BIN" insights --prescription 2>&1; then
  log "Step 2: insights --prescription completed successfully"
else
  log "Step 2: insights --prescription failed with exit code $?"
fi

# Step 3: 发送 macOS 通知
log "Step 3: notification"
osascript -e 'display notification "Weekly insights 报告已生成" with title "Refine Weekly Insights"' 2>&1 || true

log "=== Weekly Insights Run End ==="
