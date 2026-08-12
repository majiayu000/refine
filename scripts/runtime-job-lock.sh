#!/usr/bin/env bash

# Shared lock for scheduled Refine jobs. Source this file, then call
# acquire_refine_runtime_job_lock before starting expensive work.

release_refine_runtime_job_lock() {
  if [[ -n "${REFINE_RUNTIME_LOCK_HELD:-}" ]]; then
    rm -f "${REFINE_RUNTIME_LOCK_HELD}/pid"
    rmdir "${REFINE_RUNTIME_LOCK_HELD}" 2>/dev/null || true
    REFINE_RUNTIME_LOCK_HELD=""
  fi
}

runtime_lock_mtime_epoch() {
  local path="$1"
  if [[ "$(uname -s)" == "Darwin" ]]; then
    stat -f %m "$path"
  else
    stat -c %Y "$path"
  fi
}

acquire_refine_runtime_job_lock() {
  local lock_dir="${REFINE_RUNTIME_JOB_LOCK_DIR:-${HOME}/.refine/runtime-job.lock}"
  local wait_seconds="${REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS:-14400}"
  local poll_seconds="${REFINE_RUNTIME_JOB_LOCK_POLL_SECONDS:-30}"
  local deadline owner lock_age lock_mtime now

  [[ "$wait_seconds" =~ ^[0-9]+$ ]] || {
    echo "ERROR: REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS must be a non-negative integer" >&2
    return 1
  }
  [[ "$poll_seconds" =~ ^[1-9][0-9]*$ ]] || {
    echo "ERROR: REFINE_RUNTIME_JOB_LOCK_POLL_SECONDS must be a positive integer" >&2
    return 1
  }

  mkdir -p "$(dirname "$lock_dir")"
  chmod 700 "$(dirname "$lock_dir")" 2>/dev/null || true
  deadline=$(( $(date +%s) + wait_seconds ))

  while ! mkdir "$lock_dir" 2>/dev/null; do
    if [[ -L "$lock_dir" ]]; then
      echo "ERROR: refusing symlink runtime lock: $lock_dir" >&2
      return 1
    fi
    if [[ ! -d "$lock_dir" ]]; then
      echo "ERROR: runtime lock path is not a directory: $lock_dir" >&2
      return 1
    fi
    owner="$(cat "$lock_dir/pid" 2>/dev/null || true)"
    if [[ "$owner" =~ ^[0-9]+$ ]] && kill -0 "$owner" 2>/dev/null; then
      if (( $(date +%s) >= deadline )); then
        echo "ERROR: timed out waiting for Refine runtime job pid $owner" >&2
        return 1
      fi
      echo "Waiting for Refine runtime job pid $owner..." >&2
      sleep "$poll_seconds"
      continue
    fi

    # mkdir is the atomic ownership boundary. Give a just-created directory
    # time to receive its pid before deciding it is stale.
    now="$(date +%s)"
    lock_mtime="$(runtime_lock_mtime_epoch "$lock_dir" 2>/dev/null || printf '%s' "$now")"
    lock_age=$(( now - lock_mtime ))
    if [[ -z "$owner" && "$lock_age" -lt 5 ]]; then
      if (( now >= deadline )); then
        echo "ERROR: timed out waiting for Refine runtime job lock initialization" >&2
        return 1
      fi
      sleep "$poll_seconds"
      continue
    fi

    rm -f "$lock_dir/pid"
    if ! rmdir "$lock_dir" 2>/dev/null; then
      echo "ERROR: refusing non-empty runtime lock directory: $lock_dir" >&2
      return 1
    fi
  done

  printf '%s\n' "$$" > "$lock_dir/pid"
  chmod 600 "$lock_dir/pid" 2>/dev/null || true
  REFINE_RUNTIME_LOCK_HELD="$lock_dir"
}
