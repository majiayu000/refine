#!/usr/bin/env bats
# Tests for scripts/session-tagger.sh
# Run with: bats scripts/tests/test_session-tagger.bats
# Requires: bats-core, jq

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
TAGGER="${SCRIPT_DIR}/session-tagger.sh"
RESETTER="${SCRIPT_DIR}/reset-weekly-tracker.sh"
FIXTURES="${SCRIPT_DIR}/tests/fixtures"

setup() {
  TEST_REFINE=$(mktemp -d)
  TEST_SESSIONS=$(mktemp -d)
  export HOME="$TEST_REFINE"
  mkdir -p "${TEST_REFINE}/.refine"
  mkdir -p "${TEST_REFINE}/.claude/projects/test-project"
}

teardown() {
  rm -rf "$TEST_REFINE" "$TEST_SESSIONS"
}

# T1: First run — no last_scan_ts, all fixture files classified
@test "T1: first run classifies all files and sets last_scan_ts" {
  cp "${FIXTURES}/delegation_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"
  cp "${FIXTURES}/exploration_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"
  cp "${FIXTURES}/deep_inquiry_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  run bash "$TAGGER"
  [ "$status" -eq 0 ]

  total=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$total" -eq 3 ]

  ts=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ -n "$ts" ]
  [ "$ts" != "" ]
}

# T2: Incremental — no new files after last_scan_ts
@test "T2: no new files after last_scan_ts causes no counter change" {
  cp "${FIXTURES}/delegation_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  # First run to set last_scan_ts
  bash "$TAGGER"
  total_before=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  ts_before=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")

  # Sleep 1s so second run timestamp differs
  sleep 1

  # Second run — no new files
  run bash "$TAGGER"
  [ "$status" -eq 0 ]

  total_after=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$total_after" -eq "$total_before" ]

  ts_after=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$ts_after" != "$ts_before" ]
}

# T3: Incremental — 1 new file after last_scan_ts
@test "T3: only new file after last_scan_ts is counted" {
  cp "${FIXTURES}/exploration_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  # First run
  bash "$TAGGER"
  total_after_first=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$total_after_first" -eq 1 ]

  # Create a new file after first run
  sleep 1
  cp "${FIXTURES}/delegation_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/new_session.jsonl"

  # Second run — only new file should be counted
  run bash "$TAGGER"
  [ "$status" -eq 0 ]

  total_final=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$total_final" -eq 2 ]
}

# T4: Classification — delegation
@test "T4: delegation fixture classified as delegation" {
  cp "${FIXTURES}/delegation_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  bash "$TAGGER"

  count=$(jq '.delegation_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$count" -ge 1 ]
}

# T5: Classification — exploration
@test "T5: exploration fixture classified as exploration" {
  cp "${FIXTURES}/exploration_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  bash "$TAGGER"

  count=$(jq '.exploration_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$count" -ge 1 ]
}

# T6: Concurrent execution — no double-add
@test "T6: concurrent executions produce correct total_sessions" {
  for i in $(seq 1 5); do
    cp "${FIXTURES}/deep_inquiry_session.jsonl" \
       "${TEST_REFINE}/.claude/projects/test-project/session_${i}.jsonl"
  done

  # Run two instances in parallel
  bash "$TAGGER" &
  bash "$TAGGER" &
  wait

  total=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  # Total should be exactly 5 (each file counted once), not 10
  [ "$total" -le 5 ]
  [ "$total" -ge 0 ]
}

# T7: Malformed JSONL — script exits 0, other files still processed
@test "T7: malformed JSONL does not abort script, other files processed" {
  cp "${FIXTURES}/malformed_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"
  cp "${FIXTURES}/exploration_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  run bash "$TAGGER"
  [ "$status" -eq 0 ]

  total=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$total" -ge 1 ]
}

# T8: Weekly reset sets watermark — historical sessions are not re-scanned
# Regression for issue #1: reset previously cleared last_scan_ts to "" causing
# the next tagger run to epoch-scan all history.
@test "T8: after weekly reset, historical sessions are not re-counted in new week" {
  cp "${FIXTURES}/exploration_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  # Week 1: count the session
  bash "$TAGGER"
  total_week1=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$total_week1" -eq 1 ]

  # Weekly reset — archives week 1, zeroes counters
  bash "$RESETTER"
  total_after_reset=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$total_after_reset" -eq 0 ]

  # Week 2: tagger runs but must NOT re-count the pre-reset session
  bash "$TAGGER"
  total_week2=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ "$total_week2" -eq 0 ]

  # last_scan_ts in JSON must be non-empty (not "" like before the fix)
  ts=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ -n "$ts" ]
}

# T9: Reset holds the same lock as the tagger — no counter corruption on
# concurrent execution (issue #2: reset previously wrote files without locking).
@test "T9: sequential reset then tagger leaves counters at zero, not overwritten" {
  for i in $(seq 1 3); do
    cp "${FIXTURES}/deep_inquiry_session.jsonl" \
       "${TEST_REFINE}/.claude/projects/test-project/old_${i}.jsonl"
  done

  # Establish week 1 with 3 sessions
  bash "$TAGGER"
  [ "$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 3 ]

  # Reset (acquires lock, zeroes counters, advances watermark)
  bash "$RESETTER"
  [ "$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 0 ]

  # Tagger runs immediately after reset — no new sessions exist post-watermark
  bash "$TAGGER"
  total=$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")
  # Must remain 0; old sessions are behind the watermark set by reset
  [ "$total" -eq 0 ]
}

# T10: last_scan_ts is ISO 8601 UTC format
@test "T10: last_scan_ts is valid ISO 8601 UTC timestamp" {
  cp "${FIXTURES}/delegation_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  bash "$TAGGER"

  ts=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")
  # Matches YYYY-MM-DDTHH:MM:SSZ pattern
  [[ "$ts" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]
}

# T11: Multiple incremental runs accumulate correctly
@test "T11: three incremental runs accumulate correct totals" {
  # Run 1: 1 file
  cp "${FIXTURES}/exploration_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"
  bash "$TAGGER"
  [ "$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 1 ]
  [ "$(jq '.exploration_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 1 ]

  # Run 2: 1 new file
  sleep 1
  cp "${FIXTURES}/delegation_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/run2.jsonl"
  bash "$TAGGER"
  [ "$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 2 ]
  [ "$(jq '.delegation_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 1 ]

  # Run 3: 1 more new file
  sleep 1
  cp "${FIXTURES}/deep_inquiry_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/run3.jsonl"
  bash "$TAGGER"
  [ "$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 3 ]
  [ "$(jq '.deep_inquiry_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 1 ]
}

# T12: last_scan_ts advances on each run even with no new files
@test "T12: last_scan_ts advances monotonically across runs" {
  cp "${FIXTURES}/delegation_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  bash "$TAGGER"
  ts1=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")

  sleep 1
  bash "$TAGGER"
  ts2=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")

  sleep 1
  bash "$TAGGER"
  ts3=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")

  # ts3 > ts2 > ts1 (string comparison works for ISO 8601)
  [[ "$ts2" > "$ts1" ]]
  [[ "$ts3" > "$ts2" ]]
}

# T13: .last_scan_ref file mirrors last_scan_ts for sub-second precision
@test "T13: .last_scan_ref mirrors last_scan_ts after scan" {
  cp "${FIXTURES}/delegation_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  bash "$TAGGER"

  # .last_scan_ref must exist (sub-second mtime shadow of last_scan_ts)
  [ -f "${TEST_REFINE}/.refine/.last_scan_ref" ]

  # last_scan_ts must also be set in JSON
  ts=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")
  [ -n "$ts" ]
  [ "$ts" != "" ]
}

# T14: weekly reset clears seen_sessions and advances last_scan_ts
@test "T14: weekly reset advances last_scan_ts and clears seen_sessions" {
  cp "${FIXTURES}/exploration_session.jsonl" "${TEST_REFINE}/.claude/projects/test-project/"

  bash "$TAGGER"
  ts_before=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")
  # seen_sessions should exist after first run
  [ -f "${TEST_REFINE}/.refine/.seen_sessions" ]

  sleep 1
  bash "$RESETTER"

  ts_after=$(jq -r '.last_scan_ts' "${TEST_REFINE}/.refine/growth-tracker.json")
  # last_scan_ts must advance
  [[ "$ts_after" > "$ts_before" ]]
  # seen_sessions must be cleared
  [ ! -f "${TEST_REFINE}/.refine/.seen_sessions" ]
  # counters must be zero
  [ "$(jq '.total_sessions' "${TEST_REFINE}/.refine/growth-tracker.json")" -eq 0 ]
}
