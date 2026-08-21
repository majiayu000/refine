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

file_mode() {
  local path="$1"
  if stat -f %Lp "$path" >/dev/null 2>&1; then
    stat -f %Lp "$path"
  else
    stat -c %a "$path"
  fi
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
cat > "${fake_bin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
method='GET'
header=''
header_transport='none'
url=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    -X)
      method="${2:-}"
      shift 2
      ;;
    -H)
      if [[ "${2:-}" == '@-' ]]; then
        IFS= read -r header || true
        header_transport='stdin'
      else
        header="${2:-}"
        if [[ "$header" == Authorization:* ]]; then
          printf 'literal-authorization-rejected\n' >> "${CURL_LOG:-/dev/null}"
          exit 86
        fi
      fi
      shift 2
      ;;
    --max-time|-D|-o|-w)
      shift 2
      ;;
    http://*|https://*)
      url="$1"
      shift
      ;;
    *)
      shift
      ;;
  esac
done

case "$url" in
  */health)
    [[ -z "$header" ]] && printf 'GET-health:no-auth\n' >> "${CURL_LOG:-/dev/null}" \
      || printf 'GET-health:auth\n' >> "${CURL_LOG:-/dev/null}"
    printf '%s\n' '{"success":true,"llm_configured":true}'
    ;;
  */v1/items\?*)
    if [[ -n "${EXPECTED_TOKEN:-}" && "$header_transport" == 'stdin' && "$header" == "Authorization: Bearer ${EXPECTED_TOKEN}" ]]; then
      printf 'GET-items:auth-stdin\n' >> "${CURL_LOG:-/dev/null}"
      printf '%s\n' '{"success":true,"items":[]}'
    elif [[ "${EXPECT_ANON:-0}" == '1' && -z "$header" ]]; then
      printf 'GET-items:no-auth\n' >> "${CURL_LOG:-/dev/null}"
      printf '%s\n' '{"success":true,"items":[]}'
    else
      printf 'GET-items:rejected\n' >> "${CURL_LOG:-/dev/null}"
      printf '%s\n' '{"success":false,"message":"Unauthorized: provide Authorization: Bearer <token>"}'
    fi
    ;;
  */v1/items)
    if [[ "$method" == 'OPTIONS' ]]; then
      printf 'HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: http://127.0.0.1:8987\r\nAccess-Control-Allow-Methods: GET\r\n\r\n'
    fi
    ;;
  *)
    exit 1
    ;;
esac
EOF
cat > "${fake_bin}/stat" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${!#}"
if [[ -n "${FAKE_TOKEN_OWNER:-}" && "$last_arg" == "${FAKE_TOKEN_PATH:-}" && \
  "${2:-}" == '%u' && ( "${1:-}" == '-f' || "${1:-}" == '-c' ) ]]; then
  printf '%s\n' "$FAKE_TOKEN_OWNER"
  exit 0
fi
exec /usr/bin/stat "$@"
EOF
chmod 700 "${fake_bin}/cargo" "${fake_bin}/uname" \
  "${fake_bin}/launchctl" "${fake_bin}/plutil" "${fake_bin}/codex" "${fake_bin}/curl" \
  "${fake_bin}/stat"

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

for invalid_token in $'two\nlines' ' leading-space' 'trailing-space ' '中文-token'; do
  invalid_token_output=''
  if invalid_token_output="$(env -i \
    HOME="$test_home" \
    CARGO_HOME="$test_cargo_home" \
    PATH="${fake_bin}:/usr/bin:/bin" \
    REFINE_API_TOKEN="$invalid_token" \
    /bin/bash "${SCRIPT_DIR}/install-local.sh" \
      --no-ui-dev --no-start --token-auth 2>&1)"; then
    fail 'installer accepted a non-header-safe API token'
  fi
  assert_not_contains "$invalid_token_output" "$invalid_token" 'invalid token appeared in installer output'
done

api_token='doctor-token-secret'
token_install_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  REFINE_API_TOKEN="$api_token" \
  /bin/bash "${SCRIPT_DIR}/install-local.sh" \
    --no-ui-dev --no-start --token-auth 2>&1)" \
  || fail 'token-mode install failed'
token_file="${test_home}/.refine/refine-server.token"
[[ -f "$token_file" && ! -L "$token_file" ]] || fail 'token-mode install did not write a regular token file'
[[ "$(file_mode "$token_file")" == '600' ]] \
  || fail 'token-mode install did not use mode 0600'
[[ "$(<"$token_file")" == "$api_token" ]] || fail 'token-mode token file has the wrong value'
assert_not_contains "$token_install_output" "$api_token" 'token-mode installer output leaked the API token'
if grep -Fq "$api_token" "${test_home}/Library/LaunchAgents/com.lifcc.refine-server.plist"; then
  fail 'token-mode plist contains the API token value'
fi

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

curl_log="${TEST_ROOT}/doctor-curl.log"
: > "$curl_log"
doctor_token_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:${test_cargo_home}/bin:/usr/bin:/bin" \
  CURL_LOG="$curl_log" \
  EXPECTED_TOKEN="$api_token" \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$doctor_token_output" 'PASS API items endpoint OK' 'Doctor did not authenticate its protected probe'
assert_not_contains "$doctor_token_output" "$api_token" 'Doctor output leaked the API token'
grep -Fxq 'GET-health:no-auth' "$curl_log" || fail 'Doctor attached auth to the public health probe'
grep -Fxq 'GET-items:auth-stdin' "$curl_log" || fail 'Doctor did not pass protected auth through curl stdin'
if grep -Fq "$api_token" "$curl_log"; then
  fail 'Doctor curl log leaked the API token'
fi

rm -f "$token_file"
: > "$curl_log"
doctor_anon_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:${test_cargo_home}/bin:/usr/bin:/bin" \
  CURL_LOG="$curl_log" \
  EXPECT_ANON=1 \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$doctor_anon_output" 'PASS API items endpoint OK' 'Doctor broke dev-anon probing'
grep -Fxq 'GET-items:no-auth' "$curl_log" || fail 'Doctor unexpectedly authenticated a dev-anon probe'

printf '%s\n' "$api_token" > "$token_file"
chmod 644 "$token_file"
: > "$curl_log"
unsafe_token_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:${test_cargo_home}/bin:/usr/bin:/bin" \
  CURL_LOG="$curl_log" \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$unsafe_token_output" 'installed API token has unsafe ownership or mode' \
  'Doctor accepted an unsafe token file mode'
assert_not_contains "$unsafe_token_output" "$api_token" 'unsafe-token diagnostic leaked the API token'
if grep -Fq 'GET-items:' "$curl_log"; then
  fail 'Doctor fell back to a protected probe after rejecting an unsafe token file'
fi

chmod 600 "$token_file"
: > "$curl_log"
wrong_owner_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:${test_cargo_home}/bin:/usr/bin:/bin" \
  CURL_LOG="$curl_log" \
  FAKE_TOKEN_OWNER=999999 \
  FAKE_TOKEN_PATH="$token_file" \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$wrong_owner_output" 'installed API token has unsafe ownership or mode' \
  'Doctor accepted a token file owned by another user'
if grep -Fq 'GET-items:' "$curl_log"; then
  fail 'Doctor probed protected API after rejecting a foreign-owned token file'
fi

rm -f "$token_file"
ln -s "${TEST_ROOT}/custom-llm.env" "$token_file"
: > "$curl_log"
symlink_token_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:${test_cargo_home}/bin:/usr/bin:/bin" \
  CURL_LOG="$curl_log" \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$symlink_token_output" 'installed API token must be a regular non-symlink file' \
  'Doctor accepted a symlinked token file'
if grep -Fq 'GET-items:' "$curl_log"; then
  fail 'Doctor probed protected API after rejecting a symlinked token file'
fi

rm -f "$token_file"
: > "$token_file"
chmod 600 "$token_file"
: > "$curl_log"
empty_token_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:${test_cargo_home}/bin:/usr/bin:/bin" \
  CURL_LOG="$curl_log" \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$empty_token_output" 'installed API token is empty or not header-safe' \
  'Doctor accepted an empty token file'
if grep -Fq 'GET-items:' "$curl_log"; then
  fail 'Doctor probed protected API after rejecting an empty token file'
fi

printf '%s\n%s\n' "$api_token" 'second-line' > "$token_file"
chmod 600 "$token_file"
: > "$curl_log"
multiline_token_output="$(env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:${test_cargo_home}/bin:/usr/bin:/bin" \
  CURL_LOG="$curl_log" \
  /bin/bash "${SCRIPT_DIR}/doctor-local.sh" --no-ui-dev 2>&1 || true)"
assert_contains "$multiline_token_output" 'installed API token is empty or not header-safe' \
  'Doctor accepted a multiline token file'
assert_not_contains "$multiline_token_output" "$api_token" 'multiline-token diagnostic leaked the API token'
if grep -Fq 'GET-items:' "$curl_log"; then
  fail 'Doctor probed protected API after rejecting a multiline token file'
fi

env -i \
  HOME="$test_home" \
  CARGO_HOME="$test_cargo_home" \
  PATH="${fake_bin}:/usr/bin:/bin" \
  /bin/bash "${SCRIPT_DIR}/install-local.sh" --no-ui-dev --no-start >/dev/null
[[ ! -e "$token_file" && ! -L "$token_file" ]] || fail 'dev-anon reinstall did not remove the token file'

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
