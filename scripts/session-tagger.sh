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
LAST_SCAN_REF="${REFINE_DIR}/.last_scan_ref"
SEEN_SESSIONS_FILE="${REFINE_DIR}/.seen_sessions"
SESSIONS_DIR="${HOME}/.claude/projects"
MAX_LOCK_AGE=120       # seconds; locks older than this are assumed stale (guards PID reuse)
MAX_LOCK_WAIT_TAGGER=5 # seconds to wait for lock in no-flock path before giving up

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

  # Count user-turn lines — matches both production Claude ("type":"user") and
  # legacy/Codex ("role":"user") formats so classification works on real sessions.
  local user_turns
  user_turns=$(echo "$sample" | grep -cE '"(role|type)":"user"' || true)

  if [[ "$user_turns" -eq 0 ]]; then
    echo "uncategorized"
    return
  fi

  # Delegation: imperative verbs in user turns
  local delegation_hits
  delegation_hits=$(echo "$sample" | grep -E '"(role|type)":"user"' | grep -ciE 'implement|create|write|build|fix|refactor|update' || true)
  if [[ "$delegation_hits" -gt "$DELEGATION_KEYWORD_THRESHOLD" ]]; then
    echo "delegation"
    return
  fi

  # Exploration: question marks in user turns (lines ending with ?" before closing quote)
  local question_lines
  question_lines=$(echo "$sample" | grep -E '"(role|type)":"user"' | grep -c '?"' || true)
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
#   • Two concurrent runs cannot claim the same file window (no race / double-count).
#   • upper_ref is created BEFORE enumeration, giving an explicit upper bound so
#     files written during classification fall outside the window and are picked up
#     by the next run — avoids both over-counting and missed-file gaps.
#   • LAST_SCAN_REF persists the watermark at sub-second mtime precision so that
#     files in the same second as the cut-off are never re-selected.

do_scan_and_update() {
  # Create upper-bound ref BEFORE enumeration — its mtime is the exact cut-off.
  # Any .jsonl whose mtime > upper_ref is excluded from this run and will be
  # found in the next run (its mtime > LAST_SCAN_REF after we advance it).
  local upper_ref lower_ref
  upper_ref=$(mktemp /tmp/session-tagger-upper.XXXXXX)
  lower_ref=$(mktemp /tmp/session-tagger-lower.XXXXXX)
  # shellcheck disable=SC2064
  trap "rm -f '$upper_ref' '$lower_ref'" RETURN

  # Phase A: establish lower bound ─────────────────────────────────────────────

  if [[ -f "$LAST_SCAN_REF" ]]; then
    # Authoritative sub-second watermark: copy its mtime directly to lower_ref.
    touch -r "$LAST_SCAN_REF" "$lower_ref"
  else
    # Bootstrap: LAST_SCAN_REF absent — derive from JSON timestamp (first run or
    # migration from an older version of this script that lacked LAST_SCAN_REF).
    local last_scan_ts
    last_scan_ts=$(jq -r '.last_scan_ts // ""' "$TRACKER_FILE")
    if [[ -z "$last_scan_ts" ]]; then
      # First ever run: epoch so every existing file is scanned.
      touch -t 197001010000 "$lower_ref"
    elif ! touch -d "$last_scan_ts" "$lower_ref" 2>/dev/null; then
      # macOS BSD touch lacks -d. Parse the UTC timestamp and set mtime in UTC
      # (TZ=UTC on both date and touch prevents a local-timezone offset shift
      # that would rescan already-counted files — issue #3).
      local ts_fmt
      ts_fmt=$(TZ=UTC date -j -f "%Y-%m-%dT%H:%M:%SZ" "$last_scan_ts" "+%Y%m%d%H%M.%S" 2>/dev/null || \
               TZ=UTC date -d "$last_scan_ts" "+%Y%m%d%H%M.%S")
      TZ=UTC touch -t "$ts_fmt" "$lower_ref"
    fi
  fi

  # Collect candidates in half-open window (lower_ref, upper_ref].
  # -not -newer upper_ref closes the upper bound (issue #1 + #2).
  # while+read+print0 instead of mapfile for bash 3.2 compatibility.
  local candidates=()
  if [[ -d "$SESSIONS_DIR" ]]; then
    while IFS= read -r -d '' f; do
      candidates+=("$f")
    done < <(find "$SESSIONS_DIR" -name "*.jsonl" \
               -newer "$lower_ref" -not -newer "$upper_ref" -print0 2>/dev/null)
  fi

  # Phase B: classify ─────────────────────────────────────────────────────────
  # Skip files already counted in a prior run so that an active .jsonl whose
  # mtime advances on every append is never double-counted (issue #1 in review).

  local delta_exploration=0 delta_deep=0 delta_delegation=0 delta_total=0
  local new_files=()
  local f tag
  for f in "${candidates[@]+"${candidates[@]}"}"; do
    [[ -f "$f" ]] || continue
    # grep -qxF: exact whole-line literal match — bash 3.2 safe, no assoc arrays
    if [[ -f "$SEEN_SESSIONS_FILE" ]] && grep -qxF "$f" "$SEEN_SESSIONS_FILE" 2>/dev/null; then
      continue
    fi
    tag=$(classify_session "$f" 2>/dev/null || echo "uncategorized")
    case "$tag" in
      exploration)  delta_exploration=$(( delta_exploration + 1 )) ;;
      deep_inquiry) delta_deep=$(( delta_deep + 1 )) ;;
      delegation)   delta_delegation=$(( delta_delegation + 1 )) ;;
    esac
    delta_total=$(( delta_total + 1 ))
    new_files+=("$f")
  done

  # Phase C: atomic tracker update ────────────────────────────────────────────

  local now_ts
  now_ts=$(date -u '+%Y-%m-%dT%H:%M:%SZ')

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

  # Advance the sub-second watermark to upper_ref's exact mtime.
  # Done AFTER the JSON write so a failed write leaves the watermark un-advanced
  # and candidates are safely re-processed on the next run.
  touch -r "$upper_ref" "$LAST_SCAN_REF"

  # Record newly counted files to prevent re-counting on future runs.
  if [[ ${#new_files[@]} -gt 0 ]]; then
    printf '%s\n' "${new_files[@]}" >> "$SEEN_SESSIONS_FILE"
  fi
}

# ── Lock and run ──────────────────────────────────────────────────────────────

if command -v flock &>/dev/null; then
  (
    flock -x 9
    do_scan_and_update
  ) 9>"$LOCK_FILE"
else
  # Fallback: mkdir-based lock (atomic on POSIX).
  # A PID file inside the lock dir enables stale-lock recovery: if the process
  # that created the lock is no longer alive, the dir is removed and we retry.
  _try_acquire_lock() {
    if mkdir "$LOCK_DIR" 2>/dev/null; then
      echo $$ > "${LOCK_DIR}/pid"
      date +%s > "${LOCK_DIR}/created"
      return 0
    fi
    # Lock dir exists — check if holder is still alive AND the lock is fresh.
    # Using only kill -0 is insufficient: after a crash the PID can be reused by
    # an unrelated live process, causing tagger to silently skip forever.
    # MAX_LOCK_AGE bounds that window: a lock older than MAX_LOCK_AGE seconds is
    # treated as stale regardless of whether kill -0 succeeds.
    local p created_ts now_ts lock_age
    p=$(cat "${LOCK_DIR}/pid" 2>/dev/null || true)
    created_ts=$(cat "${LOCK_DIR}/created" 2>/dev/null || echo 0)
    now_ts=$(date +%s)
    lock_age=$(( now_ts - created_ts ))
    if [[ -n "$p" ]] && kill -0 "$p" 2>/dev/null && [[ "$lock_age" -lt "$MAX_LOCK_AGE" ]]; then
      return 1  # live process holds a fresh lock
    fi
    # Stale lock (process gone, no PID/created file, or age >= MAX_LOCK_AGE) — remove and retry once.
    rm -rf "$LOCK_DIR"
    if mkdir "$LOCK_DIR" 2>/dev/null; then
      echo $$ > "${LOCK_DIR}/pid"
      date +%s > "${LOCK_DIR}/created"
      return 0
    fi
    return 1
  }

  # Wait up to MAX_LOCK_WAIT_TAGGER seconds rather than exiting immediately when
  # the lock is busy.  Exiting right away means sessions written after the
  # winner's upper_ref cutoff (but before the lock is released) are only captured
  # by the next hook invocation.  A short wait lets this instance run with its
  # own upper_ref, closing that gap.
  waited=0
  until _try_acquire_lock; do
    if [[ $waited -ge $MAX_LOCK_WAIT_TAGGER ]]; then
      # Timed out — next hook invocation will re-scan from current LAST_SCAN_REF.
      exit 0
    fi
    sleep 1
    waited=$(( waited + 1 ))
  done
  trap 'rm -rf "$LOCK_DIR" 2>/dev/null || true' EXIT
  do_scan_and_update
  rm -rf "$LOCK_DIR" 2>/dev/null || true
fi
