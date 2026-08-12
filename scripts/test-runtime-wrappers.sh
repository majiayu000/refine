#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/refine-runtime-wrapper-test.XXXXXX")
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

home="${TEST_ROOT}/home"
mkdir -p "${home}/.refine"
chmod 700 "${home}/.refine"
printf '%s\n' "export BASE_API_KEY='wrapper-secret'" > "${home}/.refine/llm.env"
chmod 600 "${home}/.refine/llm.env"
printf '%s\n' 'token-secret' > "${home}/.refine/server.token"
chmod 600 "${home}/.refine/server.token"

fake_server="${TEST_ROOT}/fake-server.sh"
cat > "$fake_server" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "${BASE_API_KEY:-}" == 'wrapper-secret' ]]
[[ "${REFINE_API_TOKEN:-}" == 'token-secret' ]]
printf 'fake-server-ok\n'
EOF
chmod 700 "$fake_server"

output=$(env -i HOME="$home" PATH="/usr/bin:/bin" \
  REFINE_API_TOKEN_FILE="${home}/.refine/server.token" \
  bash "${SCRIPT_DIR}/run-refine-server.sh" "$fake_server" "${TEST_ROOT}/missing.env" 2>&1) \
  || fail 'server wrapper did not load secure LLM and token files'
[[ "$output" == *'starting pid='* ]] || fail 'server wrapper did not log a startup boundary'
[[ "$output" == *'LLM source=secure-file'* ]] || fail 'server wrapper did not report redacted LLM source'
[[ "$output" == *'fake-server-ok'* ]] || fail 'server wrapper did not exec the server'
[[ "$output" != *'wrapper-secret'* ]] || fail 'server wrapper leaked the LLM secret'
[[ "$output" != *'token-secret'* ]] || fail 'server wrapper leaked the API token'

chmod 644 "${home}/.refine/server.token"
if env -i HOME="$home" PATH="/usr/bin:/bin" \
  REFINE_API_TOKEN_FILE="${home}/.refine/server.token" \
  bash "${SCRIPT_DIR}/run-refine-server.sh" "$fake_server" "${TEST_ROOT}/missing.env" >/dev/null 2>&1; then
  fail 'server wrapper accepted an insecure token file'
fi
chmod 600 "${home}/.refine/server.token"

empty_home="${TEST_ROOT}/empty-home"
mkdir -p "${empty_home}/.refine"
chmod 700 "${empty_home}/.refine"
no_llm_server="${TEST_ROOT}/no-llm-server.sh"
cat > "$no_llm_server" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ -z "${BASE_API_KEY:-}" ]]
printf 'no-llm-server-ok\n'
EOF
chmod 700 "$no_llm_server"
output=$(env -i HOME="$empty_home" PATH="/usr/bin:/bin" \
  bash "${SCRIPT_DIR}/run-refine-server.sh" "$no_llm_server" "${TEST_ROOT}/missing.env" 2>&1) \
  || fail 'server wrapper should allow query-only operation without LLM credentials'
[[ "$output" == *'extraction is disabled'* ]] || fail 'server wrapper did not explain query-only mode'
[[ "$output" == *'no-llm-server-ok'* ]] || fail 'server wrapper did not start query-only server'

printf 'All runtime wrapper tests passed\n'
