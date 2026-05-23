#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/install-local.sh [OPTIONS]

Install or upgrade the local Refine stack from this checkout.

Options:
  --no-ui-dev       Do not install/start the desktop UI Vite dev service.
  --no-launchd      Install binaries only; skip macOS LaunchAgents.
  --no-start        Write LaunchAgents but do not start/restart services.
  --token-auth      Require REFINE_API_TOKEN instead of local dev anonymous API access.
  -h, --help        Show this help.

Defaults:
  - Installs refine, mirror, and refine-server into Cargo's bin directory.
  - On macOS, writes user LaunchAgents for server, daily ingest, weekly insights,
    and the desktop UI dev service when Bun is available.
  - Uses REFINE_DEV_ANON=1 for loopback-only local dashboard/API access unless
    --token-auth is passed.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
launchd_enabled=1
start_services=1
ui_dev_enabled=1
auth_mode="dev-anon"

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

write_server_plist() {
  local path="$1"
  local cargo_bin="$2"
  local repo="$3"
  local path_env="$4"
  local repo_xml cargo_bin_xml path_xml token_xml=""
  repo_xml="$(printf '%s' "$repo" | xml_escape)"
  cargo_bin_xml="$(printf '%s' "$cargo_bin" | xml_escape)"
  path_xml="$(printf '%s' "$path_env" | xml_escape)"

  if [[ "$auth_mode" == "token" ]]; then
    [[ -n "${REFINE_API_TOKEN:-}" ]] || die "--token-auth requires REFINE_API_TOKEN in the current environment"
    token_xml="<key>REFINE_API_TOKEN</key>
    <string>$(printf '%s' "$REFINE_API_TOKEN" | xml_escape)</string>"
  else
    token_xml="<key>REFINE_DEV_ANON</key>
    <string>1</string>"
  fi

  write_file "$path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.lifcc.refine-server</string>
  <key>ProgramArguments</key>
  <array>
    <string>${cargo_bin_xml}/refine-server</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${repo_xml}</string>
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
  local repo_xml script_xml log_xml weekday_block=""
  repo_xml="$(printf '%s' "$repo_root" | xml_escape)"
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
  <string>${repo_xml}</string>
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
  local bun_xml path_xml ui_xml
  bun_xml="$(printf '%s' "$bun_bin" | xml_escape)"
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
    <string>dev</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${ui_xml}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>
    <string>${HOME}</string>
    <key>PATH</key>
    <string>${path_xml}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>${repo_root}/.run/launchd-refine-ui.out.log</string>
  <key>StandardErrorPath</key>
  <string>${repo_root}/.run/launchd-refine-ui.err.log</string>
</dict>
</plist>
EOF
}

load_plist() {
  local path="$1"
  local label="$2"
  local kickstart="${3:-0}"
  local domain="gui/$(id -u)"

  launchctl bootout "$domain" "$path" >/dev/null 2>&1 || true
  launchctl bootstrap "$domain" "$path"
  if [[ "$kickstart" == "1" ]]; then
    launchctl kickstart -k "${domain}/${label}" >/dev/null 2>&1 || true
  fi
}

need_cmd cargo

log "repo root: $repo_root"
log "installing Rust binaries"
cargo install --path "${repo_root}/apps/cli"
cargo install --path "${repo_root}/apps/mirror"
cargo install --path "${repo_root}/apps/server"

cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
path_env="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:${cargo_bin}"

if [[ "$ui_dev_enabled" == "1" && -d "${repo_root}/apps/desktop/ui" ]]; then
  if command -v bun >/dev/null 2>&1; then
    log "installing desktop UI dependencies"
    (cd "${repo_root}/apps/desktop/ui" && bun install)
  else
    log "Bun not found; skipping desktop UI dev service"
    ui_dev_enabled=0
  fi
fi

if [[ "$launchd_enabled" != "1" ]]; then
  log "launchd disabled; binaries installed only"
  exit 0
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  log "launchd is only supported on macOS; binaries installed only"
  exit 0
fi

need_cmd launchctl
need_cmd plutil

launch_agents="${HOME}/Library/LaunchAgents"
mkdir -p "$launch_agents" "${HOME}/Library/Logs" "${HOME}/.refine"

server_plist="${launch_agents}/com.lifcc.refine-server.plist"
daily_plist="${launch_agents}/com.lifcc.refine-daily-ingest.plist"
weekly_plist="${launch_agents}/com.lifcc.refine-weekly-insights.plist"
ui_plist="${launch_agents}/com.lifcc.refine-ui-dev.plist"

write_server_plist "$server_plist" "$cargo_bin" "$repo_root" "$path_env"
write_calendar_plist "$daily_plist" "com.lifcc.refine-daily-ingest" "${repo_root}/scripts/daily-refresh.sh" "${HOME}/Library/Logs/refine-daily-ingest.log" 8 0
write_calendar_plist "$weekly_plist" "com.lifcc.refine-weekly-insights" "${repo_root}/scripts/weekly-insights.sh" "${HOME}/Library/Logs/refine-insights.log" 9 0 1
if [[ "$ui_dev_enabled" == "1" ]]; then
  write_ui_plist "$ui_plist" "$(command -v bun)" "$path_env"
fi

for plist in "$server_plist" "$daily_plist" "$weekly_plist"; do
  plutil -lint "$plist" >/dev/null
done
if [[ "$ui_dev_enabled" == "1" ]]; then
  plutil -lint "$ui_plist" >/dev/null
fi

if [[ "$start_services" == "1" ]]; then
  log "loading LaunchAgents"
  load_plist "$server_plist" "com.lifcc.refine-server" 1
  load_plist "$daily_plist" "com.lifcc.refine-daily-ingest" 0
  load_plist "$weekly_plist" "com.lifcc.refine-weekly-insights" 0
  if [[ "$ui_dev_enabled" == "1" ]]; then
    load_plist "$ui_plist" "com.lifcc.refine-ui-dev" 1
  fi
fi

log "done"
log "Run scripts/doctor-local.sh to verify the local stack."
