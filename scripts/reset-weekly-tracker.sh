#!/usr/bin/env bash
set -euo pipefail

REFINE_DIR="${HOME}/.refine"
TRACKER_FILE="${REFINE_DIR}/growth-tracker.json"
HISTORY_FILE="${REFINE_DIR}/growth-history.jsonl"
LAST_SCAN_REF="${REFINE_DIR}/.last_scan_ref"
SEEN_SESSIONS_FILE="${REFINE_DIR}/.seen_sessions"
# Same lock identifiers as session-tagger.sh — both paths must be kept in sync.
LOCK_FILE="${REFINE_DIR}/.growth-tracker.lock"
LOCK_DIR="${REFINE_DIR}/.growth.lock"
LOG_PREFIX="[refine-reset]"
MAX_LOCK_WAIT=15  # seconds to wait for the tagger to release the lock

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

do_reset() {
  # 归档本周数据到历史文件（追加一行 JSONL）
  log "Archiving current week to ${HISTORY_FILE}"
  local archived_at
  archived_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
  jq -c --arg ts "$archived_at" '. + {archived_at: $ts}' "$TRACKER_FILE" >> "$HISTORY_FILE"

  # 重置本周计数器，更新 week_start 为今天。
  # last_scan_ts 设为当前 UTC 时间（非空字符串）——这与 LAST_SCAN_REF 的新
  # mtime 一起，确保下一次增量扫描只拾取重置之后的新会话，而不会把历史会话
  # 重新计入本周指标（issue #1）。
  local new_week_start now_ts
  new_week_start=$(date '+%Y-%m-%d')
  now_ts=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
  log "Resetting tracker with week_start=${new_week_start}, watermark=${now_ts}"

  jq --arg ws "$new_week_start" --arg ts "$now_ts" '{
    week_start: $ws,
    exploration_sessions: 0,
    deep_inquiry_sessions: 0,
    delegation_sessions: 0,
    prediction_before_ask: 0,
    total_sessions: 0,
    last_scan_ts: $ts
  }' "$TRACKER_FILE" > "${TRACKER_FILE}.tmp" && mv "${TRACKER_FILE}.tmp" "$TRACKER_FILE"

  # 将 LAST_SCAN_REF 的 mtime 推进到"现在"，作为子秒精度的扫描下界水位线。
  # 上一周的会话文件 mtime < 现在，不再落入新扫描窗口，因此可以安全清空
  # SEEN_SESSIONS_FILE（该文件只在同一周内防止重复计数）。
  touch "$LAST_SCAN_REF"
  rm -f "$SEEN_SESSIONS_FILE"

  log "Weekly tracker reset complete"
}

# ── Acquire the same lock used by session-tagger.sh ───────────────────────────
# This prevents a racing tagger run from overwriting the zeroed counters with
# pre-reset increments (issue #2).

if command -v flock &>/dev/null; then
  (
    flock -x -w "$MAX_LOCK_WAIT" 9 || {
      log "Failed to acquire lock after ${MAX_LOCK_WAIT}s, aborting reset"
      exit 1
    }
    do_reset
  ) 9>"$LOCK_FILE"
else
  # Fallback: mkdir-based lock (mirrors session-tagger.sh logic).
  _try_acquire_lock() {
    if mkdir "$LOCK_DIR" 2>/dev/null; then
      echo $$ > "${LOCK_DIR}/pid"
      date +%s > "${LOCK_DIR}/created"
      return 0
    fi
    # Lock dir exists — check if the holder is still alive.
    # Without this check an abnormal exit (crash/SIGKILL) leaves a permanent
    # stale lock and all subsequent weekly resets fail indefinitely (issue #4).
    local p
    p=$(cat "${LOCK_DIR}/pid" 2>/dev/null || true)
    if [[ -n "$p" ]] && kill -0 "$p" 2>/dev/null; then
      return 1  # live process holds the lock
    fi
    # Stale lock — remove and retry once.
    rm -rf "$LOCK_DIR"
    if mkdir "$LOCK_DIR" 2>/dev/null; then
      echo $$ > "${LOCK_DIR}/pid"
      date +%s > "${LOCK_DIR}/created"
      return 0
    fi
    return 1
  }

  waited=0
  until _try_acquire_lock; do
    if [[ $waited -ge $MAX_LOCK_WAIT ]]; then
      log "Failed to acquire lock after ${MAX_LOCK_WAIT}s, aborting reset"
      exit 1
    fi
    sleep 1
    waited=$(( waited + 1 ))
  done
  trap 'rm -rf "$LOCK_DIR" 2>/dev/null || true' EXIT
  do_reset
  rm -rf "$LOCK_DIR" 2>/dev/null || true
fi
