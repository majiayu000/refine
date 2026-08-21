#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/.." && pwd)
TEST_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/refine-install-local-test.XXXXXX")
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

assert_contains() {
  local text="$1"
  local needle="$2"
  local label="$3"
  case "$text" in
    *"$needle"*) ;;
    *) fail "$label" ;;
  esac
}

assert_not_contains() {
  local text="$1"
  local needle="$2"
  local label="$3"
  case "$text" in
    *"$needle"*) fail "$label" ;;
    *) ;;
  esac
}

fake_bin="${TEST_ROOT}/bin"
test_home="${TEST_ROOT}/home"
test_cargo_home="${test_home}/.cargo"
mkdir -p "$fake_bin" "$test_home" "$test_cargo_home/bin"

cat > "${fake_bin}/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
source_path=''
while [[ $# -gt 0 ]]; do
  if [[ "$1" == '--path' ]]; then
    shift
    source_path="${1:-}"
  fi
  shift
done
case "$(basename "$source_path")" in
  cli) binary=refine ;;
  mirror) binary=mirror ;;
  server) binary=refine-server ;;
  *) exit 2 ;;
esac
mkdir -p "${CARGO_HOME}/bin"
printf '#!/usr/bin/env bash\nexit 0\n' > "${CARGO_HOME}/bin/${binary}"
chmod 700 "${CARGO_HOME}/bin/${binary}"
EOF
cat > "${fake_bin}/uname" <<'EOF'
#!/usr/bin/env bash
printf 'Darwin\n'
EOF
cat > "${fake_bin}/launchctl" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "${fake_bin}/plutil" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "${fake_bin}/codex" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod 700 "${fake_bin}/cargo" "${fake_bin}/uname" \
  "${fake_bin}/launchctl" "${fake_bin}/plutil" "${fake_bin}/codex"

custom_llm_env="${TEST_ROOT}/custom-llm.env"
printf '%s\n' "export BASE_API_KEY='custom-path-secret'" > "$custom_llm_env"
chmod 600 "$custom_llm_env"
install_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  BASE_API_KEY='process-only-secret' \
  REFINE_LLM_ENV_FILE="$custom_llm_env" \
  /bin/bash "${SCRIPT_DIR}/install-local.sh" \
    --no-ui-dev --no-start --cognitive-portrait 2>&1)" \
  || fail 'installer failed with transient credential inputs'
assert_contains "$install_output" 'WARNING: LLM credentials are not configured for LaunchAgents' \
  'transient credentials suppressed the LaunchAgent warning'
assert_not_contains "$install_output" 'custom-path-secret' 'installer leaked a custom-path credential'
assert_not_contains "$install_output" 'process-only-secret' 'installer leaked a process credential'

runtime_scripts=(
  cognitive-portrait.sh
  daily-refresh.sh
  load-llm-env.sh
  quota-time.sh
  run-refine-server.sh
  runtime-job-lock.sh
  weekly-insights.sh
)
for name in "${runtime_scripts[@]}"; do
  installed="${test_home}/.refine/scripts/${name}"
  [[ -x "$installed" ]] || fail "runtime script was not installed: ${name}"
  cmp -s "${SCRIPT_DIR}/${name}" "$installed" \
    || fail "installed runtime script differs from source: ${name}"
done

launch_agents="${test_home}/Library/LaunchAgents"
server_plist="${launch_agents}/com.lifcc.refine-server.plist"
daily_plist="${launch_agents}/com.lifcc.refine-daily-ingest.plist"
weekly_plist="${launch_agents}/com.lifcc.refine-weekly-insights.plist"
portrait_plist="${launch_agents}/com.lifcc.refine-cognitive-portrait.plist"

grep -Fq "${test_home}/.refine/scripts/run-refine-server.sh" "$server_plist" \
  || fail 'server LaunchAgent does not use the installed wrapper'
grep -Fq "${test_cargo_home}/bin/refine-server" "$server_plist" \
  || fail 'server LaunchAgent does not use the installed binary'
grep -Fq "${test_home}/.refine/scripts/daily-refresh.sh" "$daily_plist" \
  || fail 'daily LaunchAgent does not use the installed script'
grep -Fq "${test_home}/.refine/scripts/weekly-insights.sh" "$weekly_plist" \
  || fail 'weekly LaunchAgent does not use the installed script'
grep -Fq "${test_home}/.refine/scripts/cognitive-portrait.sh" "$portrait_plist" \
  || fail 'cognitive portrait LaunchAgent does not use the installed script'
grep -Fq '<key>REFINE_ROOT</key>' "$portrait_plist" \
  || fail 'cognitive portrait LaunchAgent lost its repository workspace'
grep -Fq "$REPO_ROOT" "$portrait_plist" \
  || fail 'cognitive portrait LaunchAgent lost its repository output root'

for plist in "$server_plist" "$daily_plist" "$weekly_plist" "$portrait_plist"; do
  if grep -Fq "${REPO_ROOT}/scripts/" "$plist"; then
    fail "LaunchAgent still executes a script from the checkout: $(basename "$plist")"
  fi
done

leaf_name='daily-refresh.sh'
leaf_installed="${test_home}/.refine/scripts/${leaf_name}"
leaf_target="${TEST_ROOT}/${leaf_name}"
cp "${SCRIPT_DIR}/${leaf_name}" "$leaf_target"
chmod 700 "$leaf_target"
rm -f "$leaf_installed"
ln -s "$leaf_target" "$leaf_installed"
doctor_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  REFINE_SERVER_URL='http://127.0.0.1:1' \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$doctor_output" 'installed runtime script must be a regular non-symlink executable' \
  'Doctor accepted a source-identical runtime script symlink'

chmod 755 "${test_home}/.refine/scripts"
unsafe_parent_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  REFINE_SERVER_URL='http://127.0.0.1:1' \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$unsafe_parent_output" 'runtime directory has unsafe ownership or mode' \
  'Doctor accepted an unsafe runtime scripts directory mode'
chmod 700 "${test_home}/.refine/scripts"

doctor_symlink_home="${TEST_ROOT}/doctor-symlink-home"
mkdir -p "${doctor_symlink_home}/.refine"
chmod 700 "${doctor_symlink_home}/.refine"
ln -s "${test_home}/.refine/scripts" "${doctor_symlink_home}/.refine/scripts"
symlink_parent_output="$(env -i \
  HOME="$doctor_symlink_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  REFINE_SERVER_URL='http://127.0.0.1:1' \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$symlink_parent_output" 'runtime directory must be a non-symlink directory' \
  'Doctor accepted a symlinked runtime scripts directory'

env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  /bin/bash "${SCRIPT_DIR}/install-local.sh" \
    --no-ui-dev --no-start --cognitive-portrait >/dev/null
[[ ! -L "$leaf_installed" ]] || fail 'installer preserved a runtime script symlink'
cmp -s "${SCRIPT_DIR}/${leaf_name}" "$leaf_installed" \
  || fail 'installer did not heal the runtime script symlink'

directory_leaf="${test_home}/.refine/scripts/quota-time.sh"
rm -f "$directory_leaf"
ln -s "$TEST_ROOT" "$directory_leaf"
directory_leaf_output=''
if directory_leaf_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  /bin/bash "${SCRIPT_DIR}/install-local.sh" \
    --no-ui-dev --no-start 2>&1)"; then
  fail 'installer accepted a runtime script symlink to a directory'
fi
assert_contains "$directory_leaf_output" 'destination is a symlink to a directory' \
  'directory-valued runtime symlink error was not actionable'
rm -f "$directory_leaf"
cp "${SCRIPT_DIR}/quota-time.sh" "$directory_leaf"
chmod 700 "$directory_leaf"

symlink_home="${TEST_ROOT}/symlink-home"
symlink_cargo_home="${symlink_home}/.cargo"
mkdir -p "${symlink_home}/.refine" "$symlink_cargo_home/bin" "${TEST_ROOT}/escape"
ln -s "${TEST_ROOT}/escape" "${symlink_home}/.refine/scripts"
if env -i \
  HOME="$symlink_home" \
  CARGO_HOME="$symlink_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  /bin/bash "${SCRIPT_DIR}/install-local.sh" \
    --no-ui-dev --no-start >/dev/null 2>&1; then
  fail 'installer accepted a symlinked runtime scripts directory'
fi

legacy_repo="${TEST_ROOT}/legacy repo"
mkdir -p "${legacy_repo}/scripts" "${legacy_repo}/apps/cli" \
  "${legacy_repo}/apps/mirror" "${legacy_repo}/apps/server"
for name in install-local.sh local-ui-contract.sh load-llm-env.sh configure-llm-env.sh; do
  cp "${SCRIPT_DIR}/${name}" "${legacy_repo}/scripts/${name}"
done
printf '%s\n' '.env' > "${legacy_repo}/.gitignore"
git -C "$legacy_repo" init -q
git -C "$legacy_repo" add .
git -C "$legacy_repo" -c user.name='Refine Tests' -c user.email='tests@invalid' \
  commit -q -m fixture
printf '%s\n' "export BASE_API_KEY='legacy-repository-secret'" > "${legacy_repo}/.env"
legacy_home="${TEST_ROOT}/legacy-home"
legacy_cargo_home="${legacy_home}/.cargo"
mkdir -p "$legacy_home" "${legacy_cargo_home}/bin"
legacy_output=''
if legacy_output="$(env -i \
  HOME="$legacy_home" \
  CARGO_HOME="$legacy_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  /bin/bash "${legacy_repo}/scripts/install-local.sh" \
    --no-ui-dev --no-start 2>&1)"; then
  fail 'installer silently replaced a legacy repository .env setup'
fi
assert_contains "$legacy_output" '--from-file' 'legacy upgrade failure omitted the migration command'
assert_contains "$legacy_output" 'no LaunchAgents were changed' 'legacy upgrade failure happened too late'
assert_contains "$legacy_output" 'legacy\ repo/.env' 'legacy migration command did not quote whitespace'
assert_not_contains "$legacy_output" 'legacy-repository-secret' 'legacy upgrade failure leaked a credential'
[[ ! -e "${legacy_cargo_home}/bin/refine" ]] \
  || fail 'legacy upgrade guard ran after binary installation'

printf 'All local installer tests passed\n'
