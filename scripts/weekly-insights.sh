#!/usr/bin/env bash
set -euo pipefail

REFINE_BIN="/Users/lifcc/.cargo/bin/refine"
PROJECT_DIR="/Users/lifcc/Desktop/code/AI/tools/refine"
ENV_FILE="${PROJECT_DIR}/.env"
TRACKER_FILE="${HOME}/.refine/growth-tracker.json"
RESET_SCRIPT="${PROJECT_DIR}/scripts/reset-weekly-tracker.sh"
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

# Step 3: 读取认知指标并发送 macOS 通知
log "Step 3: growth metrics & notification"
if [[ -f "$TRACKER_FILE" ]]; then
  total_sessions=$(jq -r '.total_sessions // 0' "$TRACKER_FILE")
  exploration=$(jq -r '.exploration_sessions // 0' "$TRACKER_FILE")

  if [[ "$total_sessions" -gt 0 ]]; then
    exploration_rate=$(( exploration * 100 / total_sessions ))
  else
    exploration_rate=0
  fi

  log "Metrics: total_sessions=${total_sessions}, exploration=${exploration}, exploration_rate=${exploration_rate}%"

  osascript -e "display notification \"本周会话: ${total_sessions}, 探索率: ${exploration_rate}%, 报告已生成\" with title \"Refine Weekly Insights\"" 2>&1 || true
else
  log "WARNING: tracker file not found: ${TRACKER_FILE}, skipping metrics"
  osascript -e 'display notification "报告已生成（无认知指标数据）" with title "Refine Weekly Insights"' 2>&1 || true
fi

# Step 4: 重置本周计数器（归档历史 + 重置）
log "Step 4: reset weekly tracker"
if [[ -x "$RESET_SCRIPT" ]]; then
  if "$RESET_SCRIPT" 2>&1; then
    log "Step 4: weekly tracker reset completed"
  else
    log "Step 4: weekly tracker reset failed with exit code $?"
  fi
else
  log "WARNING: reset script not found or not executable: ${RESET_SCRIPT}"
fi

log "=== Weekly Insights Run End ==="
