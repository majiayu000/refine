#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/install-local.sh [OPTIONS]

Install or upgrade the local Refine stack from this checkout.

Options:
  --no-ui-dev       Do not install/start the desktop UI Vite dev service; disable any existing UI LaunchAgent.
  --no-launchd      Install binaries only; skip macOS LaunchAgents.
  --no-start        Write LaunchAgents but do not start/restart services.
  --token-auth      Require REFINE_API_TOKEN instead of local dev anonymous API access.
  --cognitive-portrait
                    Install the opt-in biweekly cognitive portrait LaunchAgent.
  --no-cognitive-portrait
                    Disable and remove the cognitive portrait LaunchAgent.
  -h, --help        Show this help.

Defaults:
  - Installs refine, mirror, and refine-server into Cargo's bin directory.
  - On macOS, writes user LaunchAgents for server, daily ingest, weekly insights,
    and the desktop UI dev service when Bun is available.
  - Uses REFINE_DEV_ANON=1 for loopback-only local dashboard/API access unless
    --token-auth is passed.
  - Cognitive portrait automation is opt-in because it invokes an AI agent
    with workspace write access and persists personal analysis in this repo.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
launchd_enabled=1
start_services=1
ui_dev_enabled=1
auth_mode="dev-anon"
cognitive_portrait_enabled="auto"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-ui-dev)
      ui_dev_enabled=0
      ;;
    --no-launchd)
      launchd_enabled=0
      ;;
    --no-start)
      start_services=0
      ;;
    --token-auth)
      auth_mode="token"
      ;;
    --cognitive-portrait)
      cognitive_portrait_enabled=1
      ;;
    --no-cognitive-portrait)
      cognitive_portrait_enabled=0
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

log() {
  printf '[install-local] %s\n' "$*"
}

die() {
  printf '[install-local] ERROR: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

file_sha256() {
  local path="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  else
    die "missing shasum or sha256sum; cannot write install manifest"
  fi
}

xml_escape() {
  sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g'
}

write_file() {
  local path="$1"
  local tmp
  tmp="$(mktemp "${path}.tmp.XXXXXX")"
  cat > "$tmp"
  if [[ -f "$path" ]] && cmp -s "$tmp" "$path"; then
    rm -f "$tmp"
    log "unchanged $path"
  else
    mv "$tmp" "$path"
    log "wrote $path"
  fi
}

write_server_token_file() {
  local path="$1"

  [[ -n "${REFINE_API_TOKEN:-}" ]] || die "--token-auth requires REFINE_API_TOKEN in the current environment"
  write_file "$path" <<EOF
${REFINE_API_TOKEN}
EOF
  chmod 600 "$path"
}

write_server_plist() {
  local path="$1"
  local cargo_bin="$2"
  local path_env="$3"
  local home_xml server_bin_xml wrapper_xml project_env_xml path_xml token_xml=""
  local server_bin="${cargo_bin}/refine-server"
  local wrapper="${repo_root}/scripts/run-refine-server.sh"
  local project_env="${repo_root}/.env"
  home_xml="$(printf '%s' "$HOME" | xml_escape)"
  path_xml="$(printf '%s' "$path_env" | xml_escape)"

  if [[ "$auth_mode" == "token" ]]; then
    local token_file="${HOME}/.refine/refine-server.token"
    write_server_token_file "$token_file"
    token_xml="<key>REFINE_API_TOKEN_FILE</key>
    <string>$(printf '%s' "$token_file" | xml_escape)</string>"
  else
    rm -f "${HOME}/.refine/refine-server.token" "${HOME}/.refine/run-refine-server.sh"
    token_xml="<key>REFINE_DEV_ANON</key>
    <string>1</string>"
  fi
  server_bin_xml="$(printf '%s' "$server_bin" | xml_escape)"
  wrapper_xml="$(printf '%s' "$wrapper" | xml_escape)"
  project_env_xml="$(printf '%s' "$project_env" | xml_escape)"

  write_file "$path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.lifcc.refine-server</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/bash</string>
    <string>${wrapper_xml}</string>
    <string>${server_bin_xml}</string>
    <string>${project_env_xml}</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${home_xml}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>${path_xml}</string>
    ${token_xml}
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <!-- Avoid a tight loop if the server repeatedly fails at startup. -->
  <key>ThrottleInterval</key>
  <integer>30</integer>
  <key>StandardOutPath</key>
  <string>${HOME}/Library/Logs/refine-server.log</string>
  <key>StandardErrorPath</key>
  <string>${HOME}/Library/Logs/refine-server.err.log</string>
</dict>
</plist>
EOF
}

write_calendar_plist() {
  local path="$1"
  local label="$2"
  local script_path="$3"
  local log_path="$4"
  local hour="$5"
  local minute="$6"
  local weekday="${7:-}"
  local home_xml script_xml log_xml weekday_block=""
  home_xml="$(printf '%s' "$HOME" | xml_escape)"
  script_xml="$(printf '%s' "$script_path" | xml_escape)"
  log_xml="$(printf '%s' "$log_path" | xml_escape)"
  if [[ -n "$weekday" ]]; then
    weekday_block="    <key>Weekday</key>
    <integer>${weekday}</integer>"
  fi

  write_file "$path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/bash</string>
    <string>${script_xml}</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${home_xml}</string>
  <key>StartCalendarInterval</key>
  <dict>
    <key>Hour</key>
    <integer>${hour}</integer>
    <key>Minute</key>
    <integer>${minute}</integer>
${weekday_block}
  </dict>
  <key>StandardOutPath</key>
  <string>${log_xml}</string>
  <key>StandardErrorPath</key>
  <string>${log_xml}</string>
</dict>
</plist>
EOF
}

write_ui_plist() {
  local path="$1"
  local bun_bin="$2"
  local path_env="$3"
  local ui_dir="${repo_root}/apps/desktop/ui"
  local bun_xml home_xml path_xml ui_xml
  bun_xml="$(printf '%s' "$bun_bin" | xml_escape)"
  home_xml="$(printf '%s' "$HOME" | xml_escape)"
  path_xml="$(printf '%s' "$path_env" | xml_escape)"
  ui_xml="$(printf '%s' "$ui_dir" | xml_escape)"

  mkdir -p "${repo_root}/.run"
  write_file "$path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.lifcc.refine-ui-dev</string>
  <key>ProgramArguments</key>
  <array>
    <string>${bun_xml}</string>
    <string>run</string>
    <string>--cwd</string>
    <string>${ui_xml}</string>
    <string>dev</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${home_xml}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>
    <string>${HOME}</string>
    <key>PATH</key>
    <string>${path_xml}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <!-- A development UI is started once per login. If it exits, launchd keeps
       the failure visible instead of entering an unbounded crash loop. -->
  <key>ThrottleInterval</key>
  <integer>30</integer>
  <key>StandardOutPath</key>
  <string>${repo_root}/.run/launchd-refine-ui.out.log</string>
  <key>StandardErrorPath</key>
  <string>${repo_root}/.run/launchd-refine-ui.err.log</string>
</dict>
</plist>
EOF
}

write_portrait_plist() {
  local path="$1"
  local agent_bin="$2"
  local path_env="$3"
  local script_path="${repo_root}/scripts/cognitive-portrait.sh"
  local repo_xml script_xml agent_xml path_xml
  repo_xml="$(printf '%s' "$repo_root" | xml_escape)"
  script_xml="$(printf '%s' "$script_path" | xml_escape)"
  agent_xml="$(printf '%s' "$agent_bin" | xml_escape)"
  path_xml="$(printf '%s' "$path_env" | xml_escape)"

  write_file "$path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.lifcc.refine-cognitive-portrait</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/bash</string>
    <string>${script_xml}</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${repo_xml}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>
    <string>${HOME}</string>
    <key>PATH</key>
    <string>${path_xml}</string>
    <key>REFINE_PORTRAIT_AGENT</key>
    <string>${agent_xml}</string>
  </dict>
  <key>StartCalendarInterval</key>
  <dict>
    <key>Weekday</key>
    <integer>0</integer>
    <key>Hour</key>
    <integer>10</integer>
    <key>Minute</key>
    <integer>0</integer>
  </dict>
  <key>StandardOutPath</key>
  <string>${HOME}/Library/Logs/refine-portrait.log</string>
  <key>StandardErrorPath</key>
  <string>${HOME}/Library/Logs/refine-portrait.log</string>
</dict>
</plist>
EOF
}

load_plist() {
  local path="$1"
  local label="$2"
  local kickstart="${3:-0}"
  local domain
  domain="gui/$(id -u)"

  launchctl bootout "$domain" "$path" >/dev/null 2>&1 || true
  launchctl bootstrap "$domain" "$path"
  if [[ "$kickstart" == "1" ]]; then
    launchctl kickstart -k "${domain}/${label}" >/dev/null 2>&1 || true
  fi
}

disable_plist() {
  local path="$1"
  local label="$2"
  local domain
  domain="gui/$(id -u)"

  launchctl bootout "$domain" "$path" >/dev/null 2>&1 || true
  launchctl bootout "${domain}/${label}" >/dev/null 2>&1 || true
  if [[ -f "$path" ]]; then
    rm -f "$path"
    log "removed $path"
  else
    log "not installed $path"
  fi
}

need_cmd cargo

refine_dir="${HOME}/.refine"
if [[ -L "$refine_dir" ]]; then
  die "Refine directory is a symlink and was rejected: ${refine_dir}"
fi
mkdir -p "$refine_dir"
chmod 700 "$refine_dir" || die "cannot secure Refine directory: ${refine_dir}"

print_llm_setup_hint() {
  local llm_env_file="${REFINE_LLM_ENV_FILE:-${refine_dir}/llm.env}"
  if [[ -z "${REFINE_ANTHROPIC_API_KEY:-}${ANTHROPIC_AUTH_TOKEN:-}${ANTHROPIC_API_KEY:-}${REFINE_OPENAI_API_KEY:-}${OPENAI_API_KEY:-}${BASE_API_KEY:-}" && \
    ! -f "$llm_env_file" ]]; then
    log "LLM credentials are not configured for unattended jobs; review then run: bash ${repo_root}/scripts/configure-llm-env.sh --check"
  fi
}

log "repo root: $repo_root"
source_commit="$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || printf 'unknown')"
[[ "$source_commit" != "unknown" ]] || die "install source is not a Git checkout"
[[ -z "$(git -C "$repo_root" status --porcelain 2>/dev/null || true)" ]] \
  || die "install source must be clean; commit or stash changes first"

log "installing Rust binaries"
cargo install --locked --path "${repo_root}/apps/cli"
cargo install --locked --path "${repo_root}/apps/mirror"
cargo install --locked --path "${repo_root}/apps/server"

cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
path_env="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:${cargo_bin}"

resolve_cognitive_portrait_setting() {
  if [[ "$cognitive_portrait_enabled" != "auto" ]]; then
    return
  fi
  local portrait_plist="${HOME}/Library/LaunchAgents/com.lifcc.refine-cognitive-portrait.plist"
  if [[ -f "$portrait_plist" ]]; then
    cognitive_portrait_enabled=1
  else
    cognitive_portrait_enabled=0
  fi
}

write_install_manifest() {
  [[ -z "$(git -C "$repo_root" status --porcelain 2>/dev/null || true)" ]] \
    || die "installation changed tracked source files; refusing a clean-source manifest"
  mkdir -p "${HOME}/.refine"
  local install_manifest="${HOME}/.refine/install-manifest"
  write_file "$install_manifest" <<EOF
source_root=${repo_root}
source_commit=${source_commit}
source_dirty=0
refine_sha256=$(file_sha256 "${cargo_bin}/refine")
mirror_sha256=$(file_sha256 "${cargo_bin}/mirror")
refine_server_sha256=$(file_sha256 "${cargo_bin}/refine-server")
installed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cognitive_portrait_enabled=${cognitive_portrait_enabled}
EOF
}

if [[ "$launchd_enabled" != "1" || "$(uname -s)" != "Darwin" ]]; then
  cognitive_portrait_enabled=0
  write_install_manifest
  print_llm_setup_hint
  if [[ "$launchd_enabled" != "1" ]]; then
    log "launchd disabled; binaries installed only"
  else
    log "launchd is only supported on macOS; binaries installed only"
  fi
  exit 0
fi

if [[ "$ui_dev_enabled" == "1" && -d "${repo_root}/apps/desktop/ui" ]]; then
  if command -v bun >/dev/null 2>&1; then
    log "installing desktop UI dependencies"
    (cd "${repo_root}/apps/desktop/ui" && bun install --frozen-lockfile)
    [[ -x "${repo_root}/apps/desktop/ui/node_modules/.bin/vite" ]] \
      || die "desktop UI install completed without an executable Vite binary"
    log "verifying desktop UI build"
    (cd "${repo_root}/apps/desktop/ui" && bun run build)
  else
    log "Bun not found; skipping desktop UI dev service"
    ui_dev_enabled=0
  fi
fi

need_cmd launchctl
need_cmd plutil

launch_agents="${HOME}/Library/LaunchAgents"
mkdir -p "$launch_agents" "${HOME}/Library/Logs"

server_plist="${launch_agents}/com.lifcc.refine-server.plist"
daily_plist="${launch_agents}/com.lifcc.refine-daily-ingest.plist"
weekly_plist="${launch_agents}/com.lifcc.refine-weekly-insights.plist"
ui_plist="${launch_agents}/com.lifcc.refine-ui-dev.plist"
portrait_plist="${launch_agents}/com.lifcc.refine-cognitive-portrait.plist"

resolve_cognitive_portrait_setting

write_server_plist "$server_plist" "$cargo_bin" "$path_env"
write_calendar_plist "$daily_plist" "com.lifcc.refine-daily-ingest" "${repo_root}/scripts/daily-refresh.sh" "${HOME}/Library/Logs/refine-daily-ingest.log" 8 0
write_calendar_plist "$weekly_plist" "com.lifcc.refine-weekly-insights" "${repo_root}/scripts/weekly-insights.sh" "${HOME}/Library/Logs/refine-insights.log" 9 0 0
if [[ "$ui_dev_enabled" == "1" ]]; then
  write_ui_plist "$ui_plist" "$(command -v bun)" "$path_env"
fi
if [[ "$cognitive_portrait_enabled" == "1" ]]; then
  need_cmd codex
  portrait_agent_bin="$(command -v codex)"
  if command -v realpath >/dev/null 2>&1; then
    portrait_agent_bin="$(realpath "$portrait_agent_bin")"
  fi
  portrait_node_bin="$(command -v node || true)"
  if [[ -n "$portrait_node_bin" ]] && command -v realpath >/dev/null 2>&1; then
    portrait_node_bin="$(realpath "$portrait_node_bin")"
  fi
  write_portrait_plist "$portrait_plist" "$portrait_agent_bin" "$(dirname "$portrait_node_bin"):${path_env}"
fi

for plist in "$server_plist" "$daily_plist" "$weekly_plist"; do
  plutil -lint "$plist" >/dev/null
done
if [[ "$ui_dev_enabled" == "1" ]]; then
  plutil -lint "$ui_plist" >/dev/null
else
  disable_plist "$ui_plist" "com.lifcc.refine-ui-dev"
fi
if [[ "$cognitive_portrait_enabled" == "1" ]]; then
  plutil -lint "$portrait_plist" >/dev/null
else
  disable_plist "$portrait_plist" "com.lifcc.refine-cognitive-portrait"
fi

if [[ "$start_services" == "1" ]]; then
  log "loading LaunchAgents"
  load_plist "$server_plist" "com.lifcc.refine-server" 1
  load_plist "$daily_plist" "com.lifcc.refine-daily-ingest" 0
  load_plist "$weekly_plist" "com.lifcc.refine-weekly-insights" 0
  if [[ "$ui_dev_enabled" == "1" ]]; then
    load_plist "$ui_plist" "com.lifcc.refine-ui-dev" 1
  fi
  if [[ "$cognitive_portrait_enabled" == "1" ]]; then
    load_plist "$portrait_plist" "com.lifcc.refine-cognitive-portrait" 0
  fi
fi

write_install_manifest
print_llm_setup_hint
log "done"
log "Run scripts/doctor-local.sh to verify the local stack."
