#!/usr/bin/env bash
set -euo pipefail

REFINE_DIR="${HOME}/.refine"
TRACKER_FILE="${REFINE_DIR}/growth-tracker.json"
HISTORY_FILE="${REFINE_DIR}/growth-history.jsonl"
LOG_PREFIX="[refine-reset]"

log() {
  echo "${LOG_PREFIX} $(date '+%Y-%m-%d %H:%M:%S') $*"
}

# 确保目录存在
mkdir -p "$REFINE_DIR"

# 兼容 tracker 文件不存在的情况
if [[ ! -f "$TRACKER_FILE" ]]; then
  log "Tracker file not found, nothing to reset"
  exit 0
fi

# 归档本周数据到历史文件（追加一行 JSONL）
log "Archiving current week to ${HISTORY_FILE}"
archived_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
jq -c --arg ts "$archived_at" '. + {archived_at: $ts}' "$TRACKER_FILE" >> "$HISTORY_FILE"

# 重置本周计数器，更新 week_start 为今天
new_week_start=$(date '+%Y-%m-%d')
log "Resetting tracker with week_start=${new_week_start}"

jq --arg ws "$new_week_start" '{
  week_start: $ws,
  exploration_sessions: 0,
  deep_inquiry_sessions: 0,
  delegation_sessions: 0,
  prediction_before_ask: 0,
  total_sessions: 0,
  last_scan_ts: ""
}' "$TRACKER_FILE" > "${TRACKER_FILE}.tmp" && mv "${TRACKER_FILE}.tmp" "$TRACKER_FILE"

log "Weekly tracker reset complete"
