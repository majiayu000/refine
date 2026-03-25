#!/usr/bin/env bash
# session-tagger.sh — incremental session classifier for growth-tracker
#
# Runs as a Claude Code Stop hook. Scans only JSONL files with mtime > last_scan_ts
# to keep execution under 1s even with thousands of files.
#
# Manual hook registration (add to ~/.claude/settings.json):
#   "Stop": [{"matcher": "", "hooks": [{"type": "command",
#     "command": "/path/to/scripts/session-tagger.sh 2>> ~/.refine/hooks-error.log"}]}]
#
# Requires: jq, find, grep, touch. flock (optional — falls back to mkdir lock).
# Note: find -newer relies on mtime; files copied/rsynced with old mtime may be missed.

set -euo pipefail

REFINE_DIR="${HOME}/.refine"
TRACKER_FILE="${REFINE_DIR}/growth-tracker.json"
LOCK_FILE="${REFINE_DIR}/.growth-tracker.lock"
LOCK_DIR="${REFINE_DIR}/.growth.lock"
SESSIONS_DIR="${HOME}/.claude/projects"

# Tunable classification thresholds
DELEGATION_KEYWORD_THRESHOLD=8
EXPLORATION_QUESTION_RATIO=50  # percent

log() {
  : # silent in normal operation; redirect stderr to log file from hook
}

# ── Init ─────────────────────────────────────────────────────────────────────

mkdir -p "$REFINE_DIR"

if [[ ! -f "$TRACKER_FILE" ]]; then
  echo '{
  "week_start": "",
  "exploration_sessions": 0,
  "deep_inquiry_sessions": 0,
  "delegation_sessions": 0,
  "prediction_before_ask": 0,
  "total_sessions": 0,
  "last_scan_ts": ""
}' > "$TRACKER_FILE"
fi

# ── Classify ──────────────────────────────────────────────────────────────────

classify_session() {
  local file="$1"
  # Read first 4096 bytes to limit I/O time
  local sample
  sample=$(head -c 4096 "$file" 2>/dev/null || true)

  if [[ -z "$sample" ]]; then
    echo "uncategorized"
    return
  fi

  # Count user-turn lines (lines containing "\"role\":\"user\"")
  local user_turns
  user_turns=$(echo "$sample" | grep -c '"role":"user"' || true)

  if [[ "$user_turns" -eq 0 ]]; then
    echo "uncategorized"
    return
  fi

  # Delegation: imperative verbs in user turns
  local delegation_hits
  delegation_hits=$(echo "$sample" | grep '"role":"user"' | grep -ciE 'implement|create|write|build|fix|refactor|update' || true)
  if [[ "$delegation_hits" -gt "$DELEGATION_KEYWORD_THRESHOLD" ]]; then
    echo "delegation"
    return
  fi

  # Exploration: question marks in user turns (lines ending with ?" before closing quote)
  local question_lines
  question_lines=$(echo "$sample" | grep '"role":"user"' | grep -c '?"' || true)
  local question_pct=$(( question_lines * 100 / user_turns ))
  if [[ "$question_pct" -gt "$EXPLORATION_QUESTION_RATIO" ]]; then
    echo "exploration"
    return
  fi

  # Deep inquiry: longer conversations
  if [[ "$user_turns" -gt 8 ]]; then
    echo "deep_inquiry"
    return
  fi

  echo "uncategorized"
}

# ── Scan + classify + update (runs entirely inside the lock) ──────────────────
#
# All three phases are inside the lock so that:
#   • Two concurrent runs cannot claim the same file window (fixes race / double-count).
#   • now_ts is captured BEFORE candidate enumeration so files written during
#     classification are not skipped permanently — they land after now_ts and
#     will be picked up by the next run (fixes watermark gap).

do_scan_and_update() {
  # Phase A: read watermark and enumerate candidates ──────────────────────────

  local last_scan_ts
  last_scan_ts=$(jq -r '.last_scan_ts // ""' "$TRACKER_FILE")

  # Capture now_ts before enumerating so files created between enumeration and
  # update are guaranteed to appear in the next run's window.
  local now_ts
  now_ts=$(date -u '+%Y-%m-%dT%H:%M:%SZ')

  local ref_file
  ref_file=$(mktemp /tmp/session-tagger-ref.XXXXXX)

  if [[ -z "$last_scan_ts" ]]; then
    # First run: touch to epoch so all files match
    touch -t 197001010000 "$ref_file"
  else
    # touch -d accepts ISO 8601 on both GNU and macOS (with coreutils)
    if ! touch -d "$last_scan_ts" "$ref_file" 2>/dev/null; then
      # macOS BSD touch: convert to format it understands via date
      local ts_local
      ts_local=$(date -j -f "%Y-%m-%dT%H:%M:%SZ" "$last_scan_ts" "+%Y%m%d%H%M.%S" 2>/dev/null || \
                 date -d "$last_scan_ts" "+%Y%m%d%H%M.%S")
      touch -t "$ts_local" "$ref_file"
    fi
  fi

  # Collect candidates — use while+read+print0 instead of mapfile for bash 3.2
  # compatibility (mapfile is a bash 4+ builtin absent on macOS default shell).
  local candidates=()
  if [[ -d "$SESSIONS_DIR" ]]; then
    while IFS= read -r -d '' f; do
      candidates+=("$f")
    done < <(find "$SESSIONS_DIR" -name "*.jsonl" -newer "$ref_file" -print0 2>/dev/null)
  fi

  rm -f "$ref_file"

  # Phase B: classify ─────────────────────────────────────────────────────────

  local delta_exploration=0 delta_deep=0 delta_delegation=0 delta_total=0
  local f tag
  for f in "${candidates[@]+"${candidates[@]}"}"; do
    [[ -f "$f" ]] || continue
    tag=$(classify_session "$f" 2>/dev/null || echo "uncategorized")
    case "$tag" in
      exploration)  delta_exploration=$(( delta_exploration + 1 )) ;;
      deep_inquiry) delta_deep=$(( delta_deep + 1 )) ;;
      delegation)   delta_delegation=$(( delta_delegation + 1 )) ;;
    esac
    # Use $(( )) assignment — never returns non-zero, safe under set -e
    delta_total=$(( delta_total + 1 ))
  done

  # Phase C: atomic tracker update ────────────────────────────────────────────
  # Write now_ts (pre-scan timestamp) as the new watermark.

  jq -c \
    --arg ts "$now_ts" \
    --argjson de "$delta_exploration" \
    --argjson dd "$delta_deep" \
    --argjson dl "$delta_delegation" \
    --argjson dt "$delta_total" \
    '.exploration_sessions += $de |
     .deep_inquiry_sessions += $dd |
     .delegation_sessions += $dl |
     .total_sessions += $dt |
     .last_scan_ts = $ts' \
    "$TRACKER_FILE" > "${TRACKER_FILE}.tmp" && mv "${TRACKER_FILE}.tmp" "$TRACKER_FILE"
}

# ── Lock and run ──────────────────────────────────────────────────────────────

if command -v flock &>/dev/null; then
  (
    flock -x 9
    do_scan_and_update
  ) 9>"$LOCK_FILE"
else
  # Fallback: mkdir-based lock (atomic on POSIX)
  if mkdir "$LOCK_DIR" 2>/dev/null; then
    trap 'rmdir "$LOCK_DIR" 2>/dev/null || true' EXIT
    do_scan_and_update
    rmdir "$LOCK_DIR" 2>/dev/null || true
  else
    # Another instance holds the lock; skip this run (next run will re-scan)
    exit 0
  fi
fi
