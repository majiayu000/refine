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
  --cognitive-portrait-root PATH
                    Use PATH as the stable portrait workspace and output root.
  --no-cognitive-portrait
                    Disable and remove the cognitive portrait LaunchAgent.
  -h, --help        Show this help.

Defaults:
  - Installs refine, mirror, and refine-server into Cargo's bin directory.
  - Copies unattended runtime scripts into ~/.refine/scripts. LaunchAgents use
    that installed prefix instead of executable paths inside the git checkout.
  - On macOS, writes user LaunchAgents for server, daily ingest, weekly insights,
    and the desktop UI dev service when Bun is available.
  - Uses REFINE_DEV_ANON=1 for loopback-only local dashboard/API access unless
    --token-auth is passed.
  - Cognitive portrait automation is opt-in because it invokes an AI agent
    with workspace write access and persists personal analysis in this repo.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
refine_dir="${HOME}/.refine"
installed_scripts="${refine_dir}/scripts"
# shellcheck source=scripts/local-ui-contract.sh
source "${repo_root}/scripts/local-ui-contract.sh"
launchd_enabled=1
start_services=1
ui_dev_enabled=1
auth_mode="dev-anon"
cognitive_portrait_enabled="auto"
cognitive_portrait_root=""
cognitive_portrait_root_explicit=0
cognitive_portrait_contract_version=2
cognitive_portrait_bundle_schema=2
cognitive_portrait_catalog_schema=2

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
    --cognitive-portrait-root)
      shift
      if [[ $# -eq 0 ]]; then
        echo "--cognitive-portrait-root requires a path" >&2
        exit 2
      fi
      cognitive_portrait_root="$1"
      cognitive_portrait_root_explicit=1
      ;;
    --cognitive-portrait-root=*)
      cognitive_portrait_root="${1#*=}"
      cognitive_portrait_root_explicit=1
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

portrait_skill_tree_sha256() {
  local root="$1" path relative hash
  local paths=()
  [[ -z "$(find "$root" -type l -print -quit)" ]] || return 1
  while IFS= read -r path; do
    paths+=("$path")
  done < <(find "$root" -type f -print | LC_ALL=C sort)
  [[ ${#paths[@]} -gt 0 ]] || return 1
  {
    for path in "${paths[@]}"; do
      relative="${path#${root}/}"
      [[ "$relative" != "$path" && "$relative" != *$'\n'* ]] || return 1
      hash="$(file_sha256 "$path")" || return 1
      printf '%s\t%s\n' "$relative" "$hash"
    done
  } | if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
  else
    sha256sum | awk '{print $1}'
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
  if [[ -L "$path" && -d "$path" ]]; then
    rm -f "$tmp"
    die "destination is a symlink to a directory: ${path}"
  elif [[ -e "$path" && ! -f "$path" && ! -L "$path" ]]; then
    rm -f "$tmp"
    die "destination is not a regular file: ${path}"
  elif [[ ! -L "$path" && -f "$path" ]] && cmp -s "$tmp" "$path"; then
    rm -f "$tmp"
    log "unchanged $path"
  else
    mv "$tmp" "$path"
    log "wrote $path"
  fi
}

install_runtime_scripts() {
  local name
  local runtime_scripts=(
    collect-cognitive-portrait.sh
    cognitive-portrait.sh
    daily-refresh.sh
    load-llm-env.sh
    quota-time.sh
    run-refine-server.sh
    runtime-job-lock.sh
    validate-cognitive-portrait.sh
    weekly-insights.sh
  )

  if [[ -L "$installed_scripts" ]]; then
    die "runtime scripts directory is a symlink and was rejected: ${installed_scripts}"
  fi
  mkdir -p "$installed_scripts"
  chmod 700 "$installed_scripts" || die "cannot secure runtime scripts directory: ${installed_scripts}"

  for name in "${runtime_scripts[@]}"; do
    [[ -f "${repo_root}/scripts/${name}" ]] || die "missing runtime script: ${repo_root}/scripts/${name}"
    write_file "${installed_scripts}/${name}" < "${repo_root}/scripts/${name}"
    chmod 700 "${installed_scripts}/${name}" || die "cannot secure runtime script: ${installed_scripts}/${name}"
  done
  log "installed unattended runtime scripts in ${installed_scripts}"
}

write_server_token_file() {
  local path="$1"

  [[ -n "${REFINE_API_TOKEN:-}" ]] || die "--token-auth requires REFINE_API_TOKEN in the current environment"
  [[ "$REFINE_API_TOKEN" != *[[:cntrl:]]* ]] \
    || die '--token-auth requires a single-line REFINE_API_TOKEN without control characters'
  [[ "$REFINE_API_TOKEN" != [[:space:]]* && "$REFINE_API_TOKEN" != *[[:space:]] ]] \
    || die '--token-auth requires REFINE_API_TOKEN without leading or trailing whitespace'
  LC_ALL=C grep -Eq '^[!-~]+$' <<< "$REFINE_API_TOKEN" \
    || die '--token-auth requires REFINE_API_TOKEN containing visible ASCII characters only'
  write_file "$path" <<EOF
${REFINE_API_TOKEN}
EOF
  chmod 600 "$path"
}

write_server_plist() {
  local path="$1"
  local cargo_bin="$2"
  local path_env="$3"
  local home_xml server_bin_xml wrapper_xml path_xml token_xml="" cors_xml=""
  local server_bin="${cargo_bin}/refine-server"
  local wrapper="${installed_scripts}/run-refine-server.sh"
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
  cors_xml="$(refine_server_trusted_origins_xml "$ui_dev_enabled")"
  server_bin_xml="$(printf '%s' "$server_bin" | xml_escape)"
  wrapper_xml="$(printf '%s' "$wrapper" | xml_escape)"

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
  </array>
  <key>WorkingDirectory</key>
  <string>${home_xml}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>${path_xml}</string>
    ${token_xml}
${cors_xml}
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
  local script_path="${installed_scripts}/cognitive-portrait.sh"
  local portrait_dir="${cognitive_portrait_root}/docs/cognitive-portraits"
  local repo_xml portrait_dir_xml script_xml agent_xml path_xml
  repo_xml="$(printf '%s' "$cognitive_portrait_root" | xml_escape)"
  portrait_dir_xml="$(printf '%s' "$portrait_dir" | xml_escape)"
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
    <key>REFINE_ROOT</key>
    <string>${repo_xml}</string>
    <key>REFINE_PORTRAIT_DIR</key>
    <string>${portrait_dir_xml}</string>
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
  if [[ -e "$path" || -L "$path" ]]; then
    rm -f "$path"
    log "removed $path"
  else
    log "not installed $path"
  fi
}

need_cmd cargo

if [[ -L "$refine_dir" ]]; then
  die "Refine directory is a symlink and was rejected: ${refine_dir}"
fi
mkdir -p "$refine_dir"
chmod 700 "$refine_dir" || die "cannot secure Refine directory: ${refine_dir}"

print_llm_setup_hint() {
  if ! canonical_llm_env_ready; then
    log "WARNING: LLM credentials are not configured for LaunchAgents; review then run: bash ${repo_root}/scripts/configure-llm-env.sh --check"
  fi
}

canonical_llm_env_ready() {
  env -i \
    HOME="$HOME" \
    PATH='/usr/bin:/bin:/usr/sbin:/sbin' \
    /bin/bash -c '
      source "$1"
      load_refine_llm_env
    ' install-local "${repo_root}/scripts/load-llm-env.sh" >/dev/null 2>&1
}

guard_legacy_project_env_upgrade() {
  local project_env="${repo_root}/.env" preflight quoted_project_env

  canonical_llm_env_ready && return 0
  [[ -e "$project_env" || -L "$project_env" ]] || return 0
  [[ ! -L "$project_env" && -f "$project_env" ]] \
    || die "legacy repository LLM env is not a regular non-symlink file: ${project_env}"

  if ! preflight="$(env -i \
    HOME="$HOME" \
    PATH='/usr/bin:/bin:/usr/sbin:/sbin' \
    /bin/bash -c '
      source "$1"
      load_refine_llm_env_optional "$2"
      printf "%s" "${REFINE_LLM_ENV_SOURCE:-none}"
    ' install-local "${repo_root}/scripts/load-llm-env.sh" "$project_env" 2>&1)"; then
    die "legacy repository LLM env is invalid; no LaunchAgents were changed: ${preflight}"
  fi
  if [[ "$preflight" == 'project-env' ]]; then
    printf -v quoted_project_env '%q' "$project_env"
    die "legacy repository .env credentials require explicit migration before this upgrade; no LaunchAgents were changed. Run: bash ${repo_root}/scripts/configure-llm-env.sh --from-file ${quoted_project_env}"
  fi
}

log "repo root: $repo_root"
source_commit="$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || printf 'unknown')"
[[ "$source_commit" != "unknown" ]] || die "install source is not a Git checkout"
[[ -z "$(git -C "$repo_root" status --porcelain 2>/dev/null || true)" ]] \
  || die "install source must be clean; commit or stash changes first"
[[ ! -L "${HOME}/.refine/install-manifest" ]] \
  || die "install manifest is a symlink and was rejected: ${HOME}/.refine/install-manifest"
if [[ "$cognitive_portrait_enabled" != "0" \
  && -L "${HOME}/Library/LaunchAgents/com.lifcc.refine-cognitive-portrait.plist" ]]; then
  die "enabled cognitive portrait plist is a symlink and was rejected: ${HOME}/Library/LaunchAgents/com.lifcc.refine-cognitive-portrait.plist"
fi

if [[ "$launchd_enabled" == "1" && "$(uname -s)" == "Darwin" ]]; then
  guard_legacy_project_env_upgrade
fi

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

plist_value() {
  local path="$1"
  local key="$2"
  if [[ -x /usr/libexec/PlistBuddy ]]; then
    /usr/libexec/PlistBuddy -c "Print :${key}" "$path" 2>/dev/null || true
  elif command -v python3 >/dev/null 2>&1; then
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

validate_cognitive_portrait_root() {
  local root="$1" expected_skill_hash actual_skill_hash
  [[ -n "$root" ]] || die "cognitive portrait root is empty"
  [[ "$root" == /* ]] || die "cognitive portrait root must be absolute: ${root}"
  [[ "$root" != *$'\n'* && "$root" != *$'\r'* && "$root" != *$'\t'* ]] \
    || die "cognitive portrait root must not contain control characters"
  [[ ! -L "$root" && -d "$root" ]] \
    || die "cognitive portrait root must be an existing non-symlink directory: ${root}"
  [[ ! -L "${root}/skills/cognitive-portrait" \
    && -f "${root}/skills/cognitive-portrait/SKILL.md" ]] \
    || die "cognitive portrait root is missing skills/cognitive-portrait/SKILL.md: ${root}"
  expected_skill_hash="$(portrait_skill_tree_sha256 "${repo_root}/skills/cognitive-portrait")" \
    || die "source cognitive portrait skill tree is incomplete"
  actual_skill_hash="$(portrait_skill_tree_sha256 "${root}/skills/cognitive-portrait")" \
    || die "cognitive portrait root has an incomplete or unsafe v2 skill tree: ${root}"
  [[ "$actual_skill_hash" == "$expected_skill_hash" ]] \
    || die "cognitive portrait root skill contract is legacy or mismatched; migrate skills/cognitive-portrait to v2 before installing: ${root}"
  [[ ! -L "${root}/docs/cognitive-portraits" \
    && -d "${root}/docs/cognitive-portraits" \
    && -f "${root}/docs/cognitive-portraits/INDEX.md" ]] \
    || die "cognitive portrait root is missing docs/cognitive-portraits/INDEX.md: ${root}"
}

resolve_cognitive_portrait_root() {
  [[ "$cognitive_portrait_enabled" == "1" ]] || {
    cognitive_portrait_root=""
    return
  }

  if [[ "$cognitive_portrait_root_explicit" != "1" ]]; then
    local install_manifest="${HOME}/.refine/install-manifest"
    local portrait_plist="${HOME}/Library/LaunchAgents/com.lifcc.refine-cognitive-portrait.plist"
    [[ ! -L "$install_manifest" ]] \
      || die "install manifest is a symlink and cannot preserve a portrait root: ${install_manifest}"
    [[ ! -L "$portrait_plist" ]] \
      || die "legacy cognitive portrait plist is a symlink and cannot be preserved: ${portrait_plist}"
    if [[ -f "$install_manifest" ]]; then
      cognitive_portrait_root="$(awk -F= '$1 == "cognitive_portrait_root" {sub(/^[^=]*=/, ""); print; exit}' "$install_manifest")"
    fi
    if [[ -z "$cognitive_portrait_root" && -f "$portrait_plist" ]]; then
      plutil -lint "$portrait_plist" >/dev/null 2>&1 \
        || die "legacy cognitive portrait plist is invalid and cannot be preserved: ${portrait_plist}"
      cognitive_portrait_root="$(plist_value "$portrait_plist" 'EnvironmentVariables:REFINE_ROOT')"
    fi
    if [[ -z "$cognitive_portrait_root" ]]; then
      cognitive_portrait_root="$repo_root"
    fi
  fi
  validate_cognitive_portrait_root "$cognitive_portrait_root"
}

if [[ "$launchd_enabled" == "1" && "$(uname -s)" == "Darwin" ]]; then
  resolve_cognitive_portrait_setting
  resolve_cognitive_portrait_root
else
  cognitive_portrait_enabled=0
  cognitive_portrait_root=""
fi

log "installing Rust binaries"
cargo install --locked --path "${repo_root}/apps/cli"
cargo install --locked --path "${repo_root}/apps/mirror"
cargo install --locked --path "${repo_root}/apps/server"

cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
path_env="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:${cargo_bin}"

write_install_manifest() {
  [[ -z "$(git -C "$repo_root" status --porcelain 2>/dev/null || true)" ]] \
    || die "installation changed tracked source files; refusing a clean-source manifest"
  mkdir -p "${HOME}/.refine"
  local install_manifest="${HOME}/.refine/install-manifest"
  local portrait_collector="" portrait_collector_sha256="" portrait_skill_tree_sha256=""
  local portrait_validator="" portrait_validator_sha256=""
  if [[ -f "${installed_scripts}/collect-cognitive-portrait.sh" ]]; then
    portrait_collector="${installed_scripts}/collect-cognitive-portrait.sh"
    portrait_collector_sha256="$(file_sha256 "$portrait_collector")"
  fi
  if [[ -f "${installed_scripts}/validate-cognitive-portrait.sh" ]]; then
    portrait_validator="${installed_scripts}/validate-cognitive-portrait.sh"
    portrait_validator_sha256="$(file_sha256 "$portrait_validator")"
  fi
  if [[ "$cognitive_portrait_enabled" == "1" ]]; then
    portrait_skill_tree_sha256="$(portrait_skill_tree_sha256 "${cognitive_portrait_root}/skills/cognitive-portrait")" \
      || die "cannot hash cognitive portrait v2 skill tree"
  fi
  write_file "$install_manifest" <<EOF
source_root=${repo_root}
source_commit=${source_commit}
source_dirty=0
refine_sha256=$(file_sha256 "${cargo_bin}/refine")
mirror_sha256=$(file_sha256 "${cargo_bin}/mirror")
refine_server_sha256=$(file_sha256 "${cargo_bin}/refine-server")
refine_bin=${cargo_bin}/refine
mirror_bin=${cargo_bin}/mirror
refine_server_bin=${cargo_bin}/refine-server
installed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cognitive_portrait_enabled=${cognitive_portrait_enabled}
cognitive_portrait_root=${cognitive_portrait_root}
cognitive_portrait_dir=${cognitive_portrait_root:+${cognitive_portrait_root}/docs/cognitive-portraits}
cognitive_portrait_agent=${portrait_agent_bin:-}
cognitive_portrait_collector=${portrait_collector}
cognitive_portrait_collector_sha256=${portrait_collector_sha256}
cognitive_portrait_validator=${portrait_validator}
cognitive_portrait_validator_sha256=${portrait_validator_sha256}
cognitive_portrait_contract_version=${cognitive_portrait_contract_version}
cognitive_portrait_bundle_schema=${cognitive_portrait_bundle_schema}
cognitive_portrait_catalog_schema=${cognitive_portrait_catalog_schema}
cognitive_portrait_skill_tree_sha256=${portrait_skill_tree_sha256}
EOF
}

if [[ "$launchd_enabled" != "1" || "$(uname -s)" != "Darwin" ]]; then
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

install_runtime_scripts

launch_agents="${HOME}/Library/LaunchAgents"
mkdir -p "$launch_agents" "${HOME}/Library/Logs"

server_plist="${launch_agents}/com.lifcc.refine-server.plist"
daily_plist="${launch_agents}/com.lifcc.refine-daily-ingest.plist"
weekly_plist="${launch_agents}/com.lifcc.refine-weekly-insights.plist"
ui_plist="${launch_agents}/com.lifcc.refine-ui-dev.plist"
portrait_plist="${launch_agents}/com.lifcc.refine-cognitive-portrait.plist"

write_server_plist "$server_plist" "$cargo_bin" "$path_env"
write_calendar_plist "$daily_plist" "com.lifcc.refine-daily-ingest" "${installed_scripts}/daily-refresh.sh" "${HOME}/Library/Logs/refine-daily-ingest.log" 8 0
write_calendar_plist "$weekly_plist" "com.lifcc.refine-weekly-insights" "${installed_scripts}/weekly-insights.sh" "${HOME}/Library/Logs/refine-insights.log" 9 0 0
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
