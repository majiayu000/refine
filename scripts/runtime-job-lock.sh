#!/usr/bin/env bash

# Run a complete scheduled Refine workflow under one kernel-backed lock.
# Linux flock and macOS lockf have different command-line interfaces, so the
# caller is re-executed as the child command instead of trying to share an FD.

run_refine_runtime_job_locked() {
  local lock_file="${REFINE_RUNTIME_JOB_LOCK_FILE:-${HOME}/.refine/runtime-job.lock}"
  local wait_seconds="${REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS:-14400}"
  local backend="${REFINE_RUNTIME_LOCK_BACKEND:-auto}"

  [[ "$wait_seconds" =~ ^[0-9]+$ ]] || {
    echo "ERROR: REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS must be a non-negative integer" >&2
    return 1
  }
  [[ "$backend" == "auto" || "$backend" == "flock" || "$backend" == "lockf" ]] || {
    echo "ERROR: REFINE_RUNTIME_LOCK_BACKEND must be auto, flock, or lockf" >&2
    return 1
  }
  [[ "$#" -gt 0 ]] || {
    echo "ERROR: runtime lock requires a child command" >&2
    return 1
  }
  if [[ -L "$lock_file" ]]; then
    echo "ERROR: refusing symlink runtime lock: $lock_file" >&2
    return 1
  fi

  mkdir -p "$(dirname "$lock_file")"
  chmod 700 "$(dirname "$lock_file")" 2>/dev/null || true
  (umask 077; : >> "$lock_file")
  chmod 600 "$lock_file" 2>/dev/null || true

  if [[ "$backend" != "lockf" ]] && command -v flock >/dev/null 2>&1; then
    flock -w "$wait_seconds" "$lock_file" env REFINE_RUNTIME_LOCK_ACTIVE=1 "$@"
    return $?
  fi

  if [[ "$backend" != "flock" ]] && command -v lockf >/dev/null 2>&1; then
    lockf -k -s -t "$wait_seconds" "$lock_file" env REFINE_RUNTIME_LOCK_ACTIVE=1 "$@"
    return $?
  fi

  echo "ERROR: requested runtime lock backend is unavailable" >&2
  return 1
}
