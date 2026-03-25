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

# Step 3: 发送 macOS 通知（growth 指标已废弃，不再读取 tracker 文件）
log "Step 3: notification"
# NOTE: refine growth/explore/deep-inquiry commands have been permanently removed;
# growth-tracker.json has no in-tree writer and all counters are permanently stale.
# The mtime-based staleness check that was here was unreliable because
# reset-weekly-tracker.sh rewrites the file each run, keeping mtime fresh while
# data remains semantically dead.  We emit an unconditional deprecation warning
# and omit the stale counters from the notification entirely.
if [[ -f "$TRACKER_FILE" ]]; then
  log "WARNING: ${TRACKER_FILE} is a DEPRECATED artifact — refine growth/explore/deep-inquiry commands have been permanently removed and no longer write to this file. All counters are stale regardless of file mtime. Use 'refine mirror score' for current data."
fi
osascript -e 'display notification "报告已生成（growth 指标已废弃，请使用 mirror score）" with title "Refine Weekly Insights"' 2>&1 || true

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
