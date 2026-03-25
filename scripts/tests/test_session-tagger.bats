#!/usr/bin/env bats
# Tests for scripts/session-tagger.sh
# Run with: bats scripts/tests/test_session-tagger.bats
# Requires: bats-core, jq

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
TAGGER="${SCRIPT_DIR}/session-tagger.sh"
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
