#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
server_url="${REFINE_SERVER_URL:-http://127.0.0.1:21567}"
failures=0
warnings=0

pass() {
  printf 'PASS %s\n' "$*"
}

warn() {
  warnings=$((warnings + 1))
  printf 'WARN %s\n' "$*"
}

fail() {
  failures=$((failures + 1))
  printf 'FAIL %s\n' "$*"
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

mtime_epoch() {
  local path="$1"
  if stat -f %m "$path" >/dev/null 2>&1; then
    stat -f %m "$path"
  else
    stat -c %Y "$path"
  fi
}

mtime_text() {
  local path="$1"
  if stat -f '%Sm' -t '%Y-%m-%d %H:%M:%S %Z' "$path" >/dev/null 2>&1; then
    stat -f '%Sm' -t '%Y-%m-%d %H:%M:%S %Z' "$path"
  else
    stat -c '%y' "$path"
  fi
}

check_cmd() {
  local cmd="$1"
  if have_cmd "$cmd"; then
    pass "command available: $cmd ($(command -v "$cmd"))"
  else
    fail "missing command: $cmd"
  fi
}

check_launch_agent() {
  local label="$1"
  local plist="${HOME}/Library/LaunchAgents/${label}.plist"

  if [[ "$(uname -s)" != "Darwin" ]]; then
    warn "launchd unavailable on this OS; skipped $label"
    return
  fi

  if [[ ! -f "$plist" ]]; then
    fail "missing LaunchAgent: $plist"
    return
  fi

  if ! plutil -lint "$plist" >/dev/null 2>&1; then
    fail "invalid plist: $plist"
    return
  fi

  local state
  state="$(launchctl print "gui/$(id -u)/${label}" 2>&1 || true)"
  if grep -q 'state = running' <<<"$state"; then
    pass "LaunchAgent running: $label"
  elif grep -q 'state = not running' <<<"$state"; then
    pass "LaunchAgent loaded: $label (not running now)"
  elif grep -q 'spawn scheduled' <<<"$state"; then
    warn "LaunchAgent spawn scheduled: $label"
  else
    fail "LaunchAgent not loaded: $label"
  fi

  if grep -q 'last exit code = [1-9]' <<<"$state"; then
    warn "$label has a non-zero last exit code"
  fi
}

check_http() {
  if ! have_cmd curl; then
    fail "missing curl; cannot check server"
    return
  fi

  local health
  health="$(curl -fsS --max-time 3 "${server_url}/health" 2>/dev/null || true)"
  if grep -q '"success":true' <<<"$health"; then
    pass "server health OK: ${server_url}/health"
  else
    fail "server health failed: ${server_url}/health"
    return
  fi

  local items
  items="$(curl -sS --max-time 3 "${server_url}/v1/items?cursor=0&limit=1" 2>/dev/null || true)"
  if grep -q '"success":true' <<<"$items"; then
    pass "API items endpoint OK"
  elif grep -q 'REFINE_API_TOKEN is not set' <<<"$items"; then
    fail "API auth blocks local dashboard; run scripts/install-local.sh or set REFINE_DEV_ANON=1"
  elif grep -q 'Authorization: Bearer' <<<"$items"; then
    fail "API requires a bearer token; set REFINE_API_TOKEN consistently in server and client"
  else
    fail "API items endpoint failed"
  fi
}

check_db() {
  local db_path="${REFINE_DB_PATH:-}"
  if [[ -z "$db_path" ]]; then
    if [[ "$(uname -s)" == "Darwin" ]]; then
      db_path="${HOME}/Library/Application Support/refine/refine.db"
    else
      db_path="${XDG_DATA_HOME:-$HOME/.local/share}/refine/refine.db"
    fi
  fi

  if [[ ! -f "$db_path" ]]; then
    warn "database not found: $db_path"
    return
  fi

  pass "database exists: $db_path"
  if have_cmd sqlite3; then
    local summary
    summary="$(sqlite3 -readonly "$db_path" "select 'items=' || count(*) || ' last_item=' || coalesce(max(created_at),'none') from items;" 2>/dev/null || true)"
    if [[ -n "$summary" ]]; then
      pass "database query OK: $summary"
    else
      fail "database query failed: $db_path"
    fi
  else
    warn "sqlite3 missing; skipped database query"
  fi
}

check_freshness() {
  local stamp="${HOME}/.refine/last-refresh-ok"
  if [[ ! -f "$stamp" ]]; then
    warn "missing refresh success marker: $stamp"
    return
  fi

  local now last age_hours
  now="$(date +%s)"
  last="$(mtime_epoch "$stamp")"
  age_hours=$(( (now - last) / 3600 ))
  if [[ "$age_hours" -le 36 ]]; then
    pass "daily refresh marker fresh (${age_hours}h old): $(cat "$stamp" 2>/dev/null || true)"
  else
    warn "daily refresh marker stale (${age_hours}h old): $(cat "$stamp" 2>/dev/null || true)"
  fi
}

check_logs() {
  local log_path
  for log_path in \
    "${HOME}/Library/Logs/refine-server.log" \
    "${HOME}/Library/Logs/refine-server.err.log" \
    "${HOME}/Library/Logs/refine-daily-ingest.log" \
    "${HOME}/Library/Logs/refine-insights.log" \
    "${repo_root}/.run/launchd-refine-ui.err.log"; do
    if [[ -f "$log_path" ]]; then
      pass "log exists: $log_path ($(mtime_text "$log_path"))"
    else
      warn "log missing: $log_path"
    fi
  done
}

check_ui_deps() {
  local ui_dir="${repo_root}/apps/desktop/ui"
  if [[ ! -d "$ui_dir" ]]; then
    warn "desktop UI directory missing"
    return
  fi
  if ! have_cmd bun; then
    warn "Bun missing; UI dev service cannot start"
    return
  fi
  if [[ -x "${ui_dir}/node_modules/.bin/vite" ]]; then
    pass "desktop UI dependencies installed"
  else
    warn "desktop UI dependencies missing; run scripts/install-local.sh"
  fi
}

printf 'Refine local doctor\n'
printf 'Repo: %s\n' "$repo_root"
printf 'Server: %s\n\n' "$server_url"

check_cmd refine
check_cmd mirror
check_cmd refine-server

check_launch_agent com.lifcc.refine-server
check_launch_agent com.lifcc.refine-daily-ingest
check_launch_agent com.lifcc.refine-weekly-insights
check_launch_agent com.lifcc.refine-ui-dev

check_http
check_db
check_freshness
check_logs
check_ui_deps

printf '\nSummary: %s failure(s), %s warning(s)\n' "$failures" "$warnings"
if [[ "$failures" -gt 0 ]]; then
  exit 1
fi
