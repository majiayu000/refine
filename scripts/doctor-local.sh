#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/doctor-local.sh [OPTIONS]

Check the machine-local Refine install.

Options:
  --no-ui-dev   Skip desktop UI dev LaunchAgent, log, and dependency checks.
  -h, --help    Show this help.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
server_url="${REFINE_SERVER_URL:-http://127.0.0.1:21567}"
failures=0
warnings=0
ui_dev_enabled=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-ui-dev)
      ui_dev_enabled=0
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

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

file_sha256() {
  local path="$1"
  if have_cmd shasum; then
    shasum -a 256 "$path" | awk '{print $1}'
  elif have_cmd sha256sum; then
    sha256sum "$path" | awk '{print $1}'
  else
    return 1
  fi
}

manifest_value() {
  local key="$1"
  local path="$2"
  awk -F= -v key="$key" '$1 == key {sub(/^[^=]*=/, ""); print; exit}' "$path"
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

  if grep -q '"llm_configured":true' <<<"$health"; then
    pass "server LLM extraction configured"
  else
    fail "server is healthy but LLM extraction is not configured; reinstall to refresh the server wrapper"
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

check_install_manifest() {
  local manifest="${HOME}/.refine/install-manifest"
  if [[ ! -f "$manifest" ]]; then
    fail "missing install manifest: $manifest"
    return
  fi

  local expected_root expected_commit current_commit installed_dirty current_status current_dirty
  expected_root="$(manifest_value source_root "$manifest")"
  expected_commit="$(manifest_value source_commit "$manifest")"
  installed_dirty="$(manifest_value source_dirty "$manifest")"
  current_commit="$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || printf 'unknown')"
  if current_status="$(git -C "$repo_root" status --porcelain 2>/dev/null)"; then
    if [[ -z "$current_status" ]]; then
      current_dirty=0
    else
      current_dirty=1
    fi
  else
    current_dirty=unknown
  fi

  if [[ "$expected_root" == "$repo_root" && "$expected_commit" == "$current_commit" && "$installed_dirty" == "0" && "$current_dirty" == "0" ]]; then
    pass "installed source matches clean checkout: ${current_commit}"
  else
    fail "installed source mismatch: root=${expected_root} commit=${expected_commit} installed_dirty=${installed_dirty}; current=${repo_root}@${current_commit} current_dirty=${current_dirty}"
  fi

  local name manifest_key binary expected_hash actual_hash
  for name in refine mirror refine-server; do
    binary="$(command -v "$name" 2>/dev/null || true)"
    manifest_key="${name//-/_}_sha256"
    expected_hash="$(manifest_value "$manifest_key" "$manifest")"
    actual_hash="$(file_sha256 "$binary" 2>/dev/null || true)"
    if [[ -n "$binary" && -n "$expected_hash" && "$expected_hash" == "$actual_hash" ]]; then
      pass "installed binary hash matches: $name"
    else
      fail "installed binary hash mismatch: $name"
    fi
  done
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

check_unattended_llm_env() {
  local llm_env_file="${REFINE_LLM_ENV_FILE:-${HOME}/.refine/llm.env}"
  local preflight

  # Reproduce launchd's relevant property: no interactive/process credentials
  # while using the same project .env fallback as scheduled jobs.
  # shellcheck disable=SC2016
  if preflight="$(env -i \
    HOME="$HOME" \
    PATH='/usr/bin:/bin:/usr/sbin:/sbin' \
    REFINE_LLM_ENV_FILE="$llm_env_file" \
    /bin/bash -c '
      set -u
      source "$1"
      if ! load_refine_llm_env "$2"; then
        exit 1
      fi
      printf "source=%s " "${REFINE_LLM_ENV_SOURCE:-none}"
      refine_llm_env_status
    ' doctor-local "$repo_root/scripts/load-llm-env.sh" "$repo_root/.env" 2>&1)"; then
    pass "unattended LLM credentials: ${preflight}"
  else
    fail "unattended LLM credential preflight failed: ${preflight}"
  fi
}

check_logs() {
  local log_path
  local log_paths=(
    "${HOME}/Library/Logs/refine-server.log" \
    "${HOME}/Library/Logs/refine-server.err.log" \
    "${HOME}/Library/Logs/refine-daily-ingest.log" \
    "${HOME}/Library/Logs/refine-insights.log"
  )
  if [[ "$ui_dev_enabled" == "1" ]]; then
    log_paths+=("${repo_root}/.run/launchd-refine-ui.err.log")
  fi

  for log_path in "${log_paths[@]}"; do
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

check_ui_http() {
  local ui_url="${REFINE_UI_URL:-http://127.0.0.1:8987}"
  if curl -fsS --max-time 3 "$ui_url" >/dev/null 2>&1; then
    pass "desktop UI reachable: $ui_url"
  else
    fail "desktop UI unreachable: $ui_url"
  fi
}

printf 'Refine local doctor\n'
printf 'Repo: %s\n' "$repo_root"
printf 'Server: %s\n\n' "$server_url"

check_cmd refine
check_cmd mirror
check_cmd refine-server
check_install_manifest

check_launch_agent com.lifcc.refine-server
check_launch_agent com.lifcc.refine-daily-ingest
check_launch_agent com.lifcc.refine-weekly-insights
if [[ "$ui_dev_enabled" == "1" ]]; then
  check_launch_agent com.lifcc.refine-ui-dev
else
  pass "desktop UI dev service skipped"
fi
install_manifest="${HOME}/.refine/install-manifest"
if [[ -f "$install_manifest" && "$(manifest_value cognitive_portrait_enabled "$install_manifest")" == "1" ]]; then
  check_launch_agent com.lifcc.refine-cognitive-portrait
fi

check_unattended_llm_env
check_http
check_db
check_freshness
check_logs
if [[ "$ui_dev_enabled" == "1" ]]; then
  check_ui_deps
  check_ui_http
else
  pass "desktop UI dependency check skipped"
fi

printf '\nSummary: %s failure(s), %s warning(s)\n' "$failures" "$warnings"
if [[ "$failures" -gt 0 ]]; then
  exit 1
fi
