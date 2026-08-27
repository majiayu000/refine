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
# shellcheck source=scripts/local-ui-contract.sh
source "${repo_root}/scripts/local-ui-contract.sh"
server_url="${REFINE_SERVER_URL:-http://127.0.0.1:21567}"
ui_url="${REFINE_UI_URL:-$REFINE_INSTALLED_UI_ORIGIN}"
ui_origin=""
ui_origin_error=""
if ! ui_origin="$(refine_url_origin "$ui_url")"; then
  ui_origin_error="invalid desktop UI URL: $ui_url"
fi
failures=0
warnings=0
ui_dev_enabled=1
runtime_scripts_valid=1

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

file_owner_uid() {
  local path="$1"
  if stat -f %u "$path" >/dev/null 2>&1; then
    stat -f %u "$path"
  else
    stat -c %u "$path"
  fi
}

file_mode() {
  local path="$1"
  if stat -f %Lp "$path" >/dev/null 2>&1; then
    stat -f %Lp "$path"
  else
    stat -c %a "$path"
  fi
}

manifest_value() {
  local key="$1"
  local path="$2"
  awk -F= -v key="$key" '$1 == key {sub(/^[^=]*=/, ""); print; exit}' "$path"
}

plist_value() {
  local path="$1"
  local key="$2"
  if [[ -x /usr/libexec/PlistBuddy ]]; then
    /usr/libexec/PlistBuddy -c "Print :${key}" "$path" 2>/dev/null || true
  elif have_cmd python3; then
    python3 - "$path" "$key" <<'PY' 2>/dev/null || true
import plistlib
import sys

try:
    with open(sys.argv[1], "rb") as handle:
        value = plistlib.load(handle)
    for component in sys.argv[2].split(":"):
        value = value[int(component)] if isinstance(value, list) else value[component]
    print(value)
except (IndexError, KeyError, OSError, ValueError):
    pass
PY
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
  shift
  local plist="${HOME}/Library/LaunchAgents/${label}.plist"

  if [[ "$(uname -s)" != "Darwin" ]]; then
    warn "launchd unavailable on this OS; skipped $label"
    return
  fi

  if [[ -L "$plist" || ! -f "$plist" ]]; then
    fail "LaunchAgent plist must be a regular non-symlink file: $plist"
    return
  fi

  if ! plutil -lint "$plist" >/dev/null 2>&1; then
    fail "invalid plist: $plist"
    return
  fi

  local index=0 expected actual extra
  for expected in "$@"; do
    actual="$(plist_value "$plist" "ProgramArguments:${index}")"
    if [[ "$actual" == "$expected" ]]; then
      pass "LaunchAgent binding matches: ${label} ProgramArguments[${index}]"
    else
      fail "LaunchAgent binding mismatch: ${label} ProgramArguments[${index}]"
    fi
    index=$((index + 1))
  done
  if [[ "$index" -gt 0 ]]; then
    extra="$(plist_value "$plist" "ProgramArguments:${index}")"
    if [[ -n "$extra" ]]; then
      fail "LaunchAgent has unexpected ProgramArguments[${index}]: ${label}"
    fi
  fi

  local state
  state="$(launchctl print "gui/$(id -u)/${label}" 2>&1 || true)"
  if [[ $# -gt 0 ]]; then
    local live_args=()
    while IFS= read -r actual; do
      live_args[${#live_args[@]}]="$actual"
    done < <(awk '
      /^[[:space:]]*arguments = \{/ {in_arguments=1; next}
      in_arguments && /^[[:space:]]*\}/ {exit}
      in_arguments {sub(/^[[:space:]]*/, ""); print}
    ' <<<"$state")
    index=0
    for expected in "$@"; do
      if [[ "${live_args[$index]:-}" == "$expected" ]]; then
        pass "LaunchAgent live binding matches: ${label} ProgramArguments[${index}]"
      else
        fail "LaunchAgent live binding mismatch: ${label} ProgramArguments[${index}]"
      fi
      index=$((index + 1))
    done
    if [[ "${#live_args[@]}" -ne "$index" ]]; then
      fail "LaunchAgent live ProgramArguments count mismatch: ${label}"
    fi
  fi
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

check_disabled_launch_agent() {
  local label="$1"
  local port="${2:-}"
  local plist="${HOME}/Library/LaunchAgents/${label}.plist"
  local state

  if [[ -e "$plist" || -L "$plist" ]]; then
    fail "disabled LaunchAgent plist still exists: ${plist}"
  else
    pass "disabled LaunchAgent plist absent: ${label}"
  fi

  if [[ "$(uname -s)" != "Darwin" ]]; then
    warn "launchd unavailable on this OS; skipped disabled-label check for $label"
  elif state="$(launchctl print "gui/$(id -u)/${label}" 2>&1)"; then
    fail "disabled LaunchAgent is still loaded: ${label}"
  else
    pass "disabled LaunchAgent label unloaded: ${label}"
  fi

  if [[ -n "$port" ]]; then
    if ! have_cmd lsof; then
      fail "missing lsof; cannot prove disabled service port ${port} is closed"
    elif lsof -nP -iTCP:"${port}" -sTCP:LISTEN 2>/dev/null | grep -q .; then
      fail "disabled service still listens on TCP port ${port}: ${label}"
    else
      pass "disabled service port is not listening: ${port}"
    fi
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

  local items token_file="${HOME}/.refine/refine-server.token" token='' token_extra='' token_multiline=0 token_owner token_mode
  if [[ -L "$token_file" || -e "$token_file" ]]; then
    if [[ -L "$token_file" || ! -f "$token_file" ]]; then
      fail "installed API token must be a regular non-symlink file: ${token_file}"
      return
    fi
    token_owner="$(file_owner_uid "$token_file" 2>/dev/null || true)"
    token_mode="$(file_mode "$token_file" 2>/dev/null || true)"
    if [[ "$token_owner" != "$(id -u)" || "$token_mode" != '600' ]]; then
      fail "installed API token has unsafe ownership or mode: ${token_file} (owner=${token_owner:-unknown} mode=${token_mode:-unknown}; expected current user/600)"
      return
    fi
    {
      IFS= read -r token || true
      if IFS= read -r token_extra || [[ -n "$token_extra" ]]; then
        token_multiline=1
      fi
    } < "$token_file"
    if [[ -z "$token" || "$token_multiline" == '1' || "$token" == *[[:cntrl:]]* || "$token" == [[:space:]]* || "$token" == *[[:space:]] ]]; then
      fail "installed API token is empty or not header-safe: ${token_file}"
      return
    fi
  fi

  if [[ -n "$token" ]]; then
    items="$(printf 'Authorization: Bearer %s\n' "$token" \
      | curl -sS --max-time 3 -H @- "${server_url}/v1/items?cursor=0&limit=1" 2>/dev/null || true)"
  else
    items="$(curl -sS --max-time 3 "${server_url}/v1/items?cursor=0&limit=1" 2>/dev/null || true)"
  fi
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
  if [[ -L "$manifest" || ! -f "$manifest" ]]; then
    fail "install manifest must be a regular non-symlink file: $manifest"
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

  local name manifest_key manifest_bin_key binary expected_binary expected_hash actual_hash
  for name in refine mirror refine-server; do
    binary="$(command -v "$name" 2>/dev/null || true)"
    manifest_key="${name//-/_}_sha256"
    manifest_bin_key="${name//-/_}_bin"
    expected_binary="$(manifest_value "$manifest_bin_key" "$manifest")"
    expected_hash="$(manifest_value "$manifest_key" "$manifest")"
    actual_hash="$(file_sha256 "$binary" 2>/dev/null || true)"
    if [[ -n "$binary" && "$binary" == "$expected_binary" ]]; then
      pass "installed binary path matches manifest: $name"
    else
      fail "installed binary path mismatch: $name"
    fi
    if [[ -n "$binary" && -n "$expected_hash" && "$expected_hash" == "$actual_hash" ]]; then
      pass "installed binary hash matches: $name"
    else
      fail "installed binary hash mismatch: $name"
    fi
  done
}

validate_portrait_root() {
  local root="$1"
  [[ -n "$root" && "$root" == /* ]] || return 1
  [[ "$root" != *$'\n'* && "$root" != *$'\r'* && "$root" != *$'\t'* ]] || return 1
  [[ ! -L "$root" && -d "$root" ]] || return 1
  [[ ! -L "${root}/skills/cognitive-portrait" \
    && -f "${root}/skills/cognitive-portrait/SKILL.md" ]] || return 1
  [[ ! -L "${root}/docs/cognitive-portraits" \
    && -d "${root}/docs/cognitive-portraits" \
    && -f "${root}/docs/cognitive-portraits/INDEX.md" ]] || return 1
}

check_cognitive_portrait() {
  local manifest="$1"
  local plist="${HOME}/Library/LaunchAgents/com.lifcc.refine-cognitive-portrait.plist"
  local root portrait_dir agent log_path latest
  root="$(manifest_value cognitive_portrait_root "$manifest")"
  portrait_dir="$(manifest_value cognitive_portrait_dir "$manifest")"
  agent="$(manifest_value cognitive_portrait_agent "$manifest")"
  log_path="${HOME}/Library/Logs/refine-portrait.log"

  if validate_portrait_root "$root"; then
    pass "cognitive portrait root valid: ${root}"
  else
    fail "cognitive portrait root invalid: ${root:-missing}"
  fi
  if [[ "$portrait_dir" == "${root}/docs/cognitive-portraits" && -d "$portrait_dir" ]]; then
    pass "cognitive portrait output directory valid: ${portrait_dir}"
  else
    fail "cognitive portrait output directory mismatch: ${portrait_dir:-missing}"
  fi
  if [[ -n "$agent" && "$agent" == /* && ! -L "$agent" && -f "$agent" && -x "$agent" ]]; then
    pass "cognitive portrait agent executable valid"
  else
    fail "cognitive portrait agent executable invalid: ${agent:-missing}"
  fi

  check_launch_agent com.lifcc.refine-cognitive-portrait \
    /bin/bash "${HOME}/.refine/scripts/cognitive-portrait.sh"
  if [[ -f "$plist" ]]; then
    if [[ "$(plist_value "$plist" WorkingDirectory)" == "$root" ]]; then
      pass "cognitive portrait WorkingDirectory matches manifest"
    else
      fail "cognitive portrait WorkingDirectory mismatches manifest"
    fi
    if [[ "$(plist_value "$plist" 'EnvironmentVariables:REFINE_ROOT')" == "$root" ]]; then
      pass "cognitive portrait REFINE_ROOT matches manifest"
    else
      fail "cognitive portrait REFINE_ROOT mismatches manifest"
    fi
    if [[ "$(plist_value "$plist" 'EnvironmentVariables:REFINE_PORTRAIT_DIR')" == "$portrait_dir" ]]; then
      pass "cognitive portrait REFINE_PORTRAIT_DIR matches manifest"
    else
      fail "cognitive portrait REFINE_PORTRAIT_DIR mismatches manifest"
    fi
    if [[ "$(plist_value "$plist" 'EnvironmentVariables:REFINE_PORTRAIT_AGENT')" == "$agent" ]]; then
      pass "cognitive portrait agent matches manifest"
    else
      fail "cognitive portrait agent mismatches manifest"
    fi
    if [[ "$(plist_value "$plist" StandardOutPath)" == "$log_path" \
      && "$(plist_value "$plist" StandardErrorPath)" == "$log_path" ]]; then
      pass "cognitive portrait log binding valid"
    else
      fail "cognitive portrait log binding mismatch"
    fi
  fi

  if [[ ! -L "$log_path" && -f "$log_path" ]]; then
    pass "cognitive portrait log exists: ${log_path} ($(mtime_text "$log_path"))"
  else
    fail "cognitive portrait log missing or unsafe: ${log_path}"
  fi
  latest="$(find "$portrait_dir" -maxdepth 1 -type f -name 'cognitive-portrait-*.md' 2>/dev/null | sort | tail -1 || true)"
  if [[ -n "$latest" ]]; then
    pass "cognitive portrait latest artifact: ${latest} ($(mtime_text "$latest"))"
  else
    fail "cognitive portrait latest artifact missing: ${portrait_dir}"
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

check_unattended_llm_env() {
  local installed_loader="${HOME}/.refine/scripts/load-llm-env.sh"
  local preflight

  if [[ "$runtime_scripts_valid" != "1" ]]; then
    warn "unattended LLM credential preflight skipped because installed runtime scripts failed integrity checks"
    return
  fi

  # Reproduce launchd's relevant property: no interactive/process credentials
  # while using the installed loader and no checkout fallback.
  if [[ ! -f "$installed_loader" ]]; then
    fail "installed LLM credential loader missing: ${installed_loader}; reinstall Refine"
    return
  fi
  # shellcheck disable=SC2016
  if preflight="$(env -i \
    HOME="$HOME" \
    PATH='/usr/bin:/bin:/usr/sbin:/sbin' \
    /bin/bash -c '
      set -u
      source "$1"
      if ! load_refine_llm_env; then
        exit 1
      fi
      printf "source=%s " "${REFINE_LLM_ENV_SOURCE:-none}"
      refine_llm_env_status
    ' doctor-local "$installed_loader" 2>&1)"; then
    pass "unattended LLM credentials: ${preflight}"
  else
    fail "unattended LLM credential preflight failed: ${preflight}"
  fi
}

check_runtime_scripts() {
  local name source installed source_hash installed_hash owner_uid mode directory
  local runtime_scripts=(
    cognitive-portrait.sh
    daily-refresh.sh
    load-llm-env.sh
    quota-time.sh
    run-refine-server.sh
    runtime-job-lock.sh
    weekly-insights.sh
  )

  for directory in "${HOME}/.refine" "${HOME}/.refine/scripts"; do
    if [[ -L "$directory" || ! -d "$directory" ]]; then
      fail "runtime directory must be a non-symlink directory: ${directory}"
      runtime_scripts_valid=0
      return
    fi
    owner_uid="$(file_owner_uid "$directory" 2>/dev/null || true)"
    mode="$(file_mode "$directory" 2>/dev/null || true)"
    if [[ "$owner_uid" != "$(id -u)" || "$mode" != '700' ]]; then
      fail "runtime directory has unsafe ownership or mode: ${directory} (owner=${owner_uid:-unknown} mode=${mode:-unknown}; expected current user/700)"
      runtime_scripts_valid=0
      return
    fi
  done

  for name in "${runtime_scripts[@]}"; do
    source="${repo_root}/scripts/${name}"
    installed="${HOME}/.refine/scripts/${name}"
    if [[ -L "$installed" || ! -f "$installed" || ! -x "$installed" ]]; then
      fail "installed runtime script must be a regular non-symlink executable: ${installed}"
      runtime_scripts_valid=0
      continue
    fi
    owner_uid="$(file_owner_uid "$installed" 2>/dev/null || true)"
    mode="$(file_mode "$installed" 2>/dev/null || true)"
    if [[ "$owner_uid" != "$(id -u)" || "$mode" != '700' ]]; then
      fail "installed runtime script has unsafe ownership or mode: ${installed} (owner=${owner_uid:-unknown} mode=${mode:-unknown}; expected current user/700)"
      runtime_scripts_valid=0
      continue
    fi
    source_hash="$(file_sha256 "$source" 2>/dev/null || true)"
    installed_hash="$(file_sha256 "$installed" 2>/dev/null || true)"
    if [[ -n "$source_hash" && "$source_hash" == "$installed_hash" ]]; then
      pass "installed runtime script matches checkout: $name"
    else
      fail "installed runtime script is stale: ${installed}; reinstall Refine"
      runtime_scripts_valid=0
    fi
  done
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
  if curl -fsS --max-time 3 "$ui_url" >/dev/null 2>&1; then
    pass "desktop UI reachable: $ui_url"
  else
    fail "desktop UI unreachable: $ui_url"
  fi
}

check_ui_cors() {
  local headers
  if [[ -n "$ui_origin_error" ]]; then
    fail "$ui_origin_error"
    return
  fi
  if ! headers="$(curl -sS --max-time 3 -D - -o /dev/null \
    -X OPTIONS \
    -H "Origin: ${ui_origin}" \
    -H 'Access-Control-Request-Method: GET' \
    "${server_url}/v1/items" 2>/dev/null)"; then
    fail "server CORS preflight request failed: ${server_url}/v1/items"
    return
  fi
  if refine_cors_preflight_succeeds "$headers" "$ui_origin" GET; then
    pass "server permits desktop UI origin: $ui_origin"
  else
    fail "server CORS blocks desktop UI origin: $ui_origin"
  fi
}

printf 'Refine local doctor\n'
printf 'Repo: %s\n' "$repo_root"
printf 'Server: %s\n\n' "$server_url"

check_cmd refine
check_cmd mirror
check_cmd refine-server
check_install_manifest

install_manifest="${HOME}/.refine/install-manifest"
refine_server_bin=""
if [[ -f "$install_manifest" ]]; then
  refine_server_bin="$(manifest_value refine_server_bin "$install_manifest")"
fi
check_launch_agent com.lifcc.refine-server \
  /bin/bash "${HOME}/.refine/scripts/run-refine-server.sh" "$refine_server_bin"
check_launch_agent com.lifcc.refine-daily-ingest \
  /bin/bash "${HOME}/.refine/scripts/daily-refresh.sh"
check_launch_agent com.lifcc.refine-weekly-insights \
  /bin/bash "${HOME}/.refine/scripts/weekly-insights.sh"
check_runtime_scripts
if [[ "$ui_dev_enabled" == "1" ]]; then
  check_launch_agent com.lifcc.refine-ui-dev
else
  check_disabled_launch_agent com.lifcc.refine-ui-dev 8987
fi
if [[ -f "$install_manifest" && "$(manifest_value cognitive_portrait_enabled "$install_manifest")" == "1" ]]; then
  check_cognitive_portrait "$install_manifest"
else
  check_disabled_launch_agent com.lifcc.refine-cognitive-portrait
fi

check_unattended_llm_env
check_http
check_db
check_freshness
check_logs
if [[ "$ui_dev_enabled" == "1" ]]; then
  check_ui_deps
  check_ui_http
  check_ui_cors
else
  pass "desktop UI dependency check skipped"
fi

printf '\nSummary: %s failure(s), %s warning(s)\n' "$failures" "$warnings"
if [[ "$failures" -gt 0 ]]; then
  exit 1
fi
