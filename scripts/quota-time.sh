#!/usr/bin/env bash

# Normalize Refine's UTC RFC 3339 quota timestamps to a fixed-width key.
# Accepts the legacy second-only form and the current 1-9 digit fractional form.
quota_timestamp_sort_key() {
  local value="$1"
  if [[ ! "$value" =~ ^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})(\.([0-9]{1,9}))?Z$ ]]; then
    return 1
  fi

  local whole_seconds="${BASH_REMATCH[1]}"
  local fraction="${BASH_REMATCH[3]:-}000000000"
  local parsed=""
  if date --version >/dev/null 2>&1; then
    parsed=$(date -u -d "${whole_seconds}Z" +%Y-%m-%dT%H:%M:%S 2>/dev/null) || return 1
  else
    parsed=$(date -j -u -f '%Y-%m-%dT%H:%M:%SZ' "${whole_seconds}Z" +%Y-%m-%dT%H:%M:%S 2>/dev/null) || return 1
  fi
  [[ "$parsed" == "$whole_seconds" ]] || return 1

  printf '%s.%sZ\n' "$whole_seconds" "${fraction:0:9}"
}
