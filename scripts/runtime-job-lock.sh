#!/usr/bin/env bash

# Shared lock for scheduled Refine jobs. BSD lockf and Linux flock both hold
# an open file descriptor for the complete workflow, so stale PID reclamation
# is delegated to the kernel instead of implemented with racy path deletion.

release_refine_runtime_job_lock() {
  case "${REFINE_RUNTIME_LOCK_MODE:-}" in
    flock)
      flock -u 9 2>/dev/null || true
      exec 9>&-
      ;;
    lockf)
      exec 9>&-
      ;;
  esac
  REFINE_RUNTIME_LOCK_MODE=""
  REFINE_RUNTIME_LOCK_FILE=""
}

acquire_refine_runtime_job_lock() {
  local lock_file="${REFINE_RUNTIME_JOB_LOCK_FILE:-${HOME}/.refine/runtime-job.lock}"
  local wait_seconds="${REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS:-14400}"

  [[ "$wait_seconds" =~ ^[0-9]+$ ]] || {
    echo "ERROR: REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS must be a non-negative integer" >&2
    return 1
  }
  if [[ -L "$lock_file" ]]; then
    echo "ERROR: refusing symlink runtime lock: $lock_file" >&2
    return 1
  fi

  mkdir -p "$(dirname "$lock_file")"
  chmod 700 "$(dirname "$lock_file")" 2>/dev/null || true

  if command -v flock >/dev/null 2>&1; then
    exec 9>"$lock_file"
    chmod 600 "$lock_file" 2>/dev/null || true
    if ! flock -w "$wait_seconds" 9; then
      exec 9>&-
      echo "ERROR: timed out waiting for Refine runtime job lock" >&2
      return 1
    fi
    REFINE_RUNTIME_LOCK_MODE="flock"
    REFINE_RUNTIME_LOCK_FILE="$lock_file"
    return 0
  fi

  if ! command -v lockf >/dev/null 2>&1; then
    echo "ERROR: neither flock nor lockf is available for runtime serialization" >&2
    return 1
  fi

  exec 9>"$lock_file"
  if ! lockf -s -t "$wait_seconds" 9; then
    exec 9>&-
    echo "ERROR: timed out waiting for Refine runtime job lock" >&2
    return 1
  fi
  chmod 600 "$lock_file" 2>/dev/null || true
  REFINE_RUNTIME_LOCK_MODE="lockf"
  REFINE_RUNTIME_LOCK_FILE="$lock_file"
}
