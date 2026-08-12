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

# Scheduled workflows must not overlap and must recover stale locks.
# shellcheck source=scripts/runtime-job-lock.sh
source "${SCRIPT_DIR}/runtime-job-lock.sh"
lock_file="${TEST_ROOT}/runtime-job.lock"
REFINE_RUNTIME_JOB_LOCK_FILE="$lock_file"
REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS=0
acquire_refine_runtime_job_lock || fail 'runtime job lock could not be acquired'
[[ -f "$lock_file" ]] || fail 'runtime job lock did not create its lock file'
release_refine_runtime_job_lock
printf '%s\n' '99999999' > "$lock_file"
acquire_refine_runtime_job_lock || fail 'runtime job lock did not recover a stale owner'
release_refine_runtime_job_lock

# Two simultaneous contenders must remain strictly serialized even when a
# stale lock file already exists on disk.
printf '%s\n' '99999999' > "$lock_file"
critical_log="${TEST_ROOT}/runtime-critical.log"
for _worker in 1 2; do
  env HOME="$home" REFINE_RUNTIME_JOB_LOCK_FILE="$lock_file" \
    REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS=10 \
    RUNTIME_LOCK_HELPER="${SCRIPT_DIR}/runtime-job-lock.sh" \
    RUNTIME_CRITICAL_LOG="$critical_log" \
    bash -c 'source "$RUNTIME_LOCK_HELPER"; acquire_refine_runtime_job_lock; printf "start %s\n" "$$" >> "$RUNTIME_CRITICAL_LOG"; sleep 1; printf "end %s\n" "$$" >> "$RUNTIME_CRITICAL_LOG"; release_refine_runtime_job_lock' &
done
wait
critical_shape="$(awk '{print $1}' "$critical_log" | paste -sd, -)"
[[ "$critical_shape" == 'start,end,start,end' ]] \
  || fail "runtime stale reclaimers overlapped: ${critical_shape}"

# A successful ingest must not publish a success marker when required advice
# fails. The scheduled command must also request strict advice semantics.
daily_home="${TEST_ROOT}/daily-home"
mkdir -p "${daily_home}/.refine" "${daily_home}/.cargo/bin"
chmod 700 "${daily_home}/.refine"
printf '%s\n' "export BASE_API_KEY='daily-secret'" > "${daily_home}/.refine/llm.env"
chmod 600 "${daily_home}/.refine/llm.env"
cat > "${daily_home}/.cargo/bin/refine" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "${daily_home}/.cargo/bin/mirror" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  score)
    [[ "${2:-}" == '--require-advice' ]] || exit 9
    exit "${FAKE_MIRROR_EXIT:-0}"
    ;;
  weekly)
    exit 0
    ;;
  *)
    exit 9
    ;;
esac
EOF
chmod 700 "${daily_home}/.cargo/bin/refine" "${daily_home}/.cargo/bin/mirror"
if env -i HOME="$daily_home" PATH="/usr/bin:/bin" FAKE_MIRROR_EXIT=7 \
  REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS=0 \
  bash "${SCRIPT_DIR}/daily-refresh.sh" >/dev/null 2>&1; then
  fail 'daily refresh succeeded when required advice failed'
fi
[[ ! -e "${daily_home}/.refine/last-refresh-ok" ]] \
  || fail 'daily refresh wrote success marker after required advice failed'
env -i HOME="$daily_home" PATH="/usr/bin:/bin" FAKE_MIRROR_EXIT=0 \
  REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS=0 \
  bash "${SCRIPT_DIR}/daily-refresh.sh" >/dev/null 2>&1 \
  || fail 'daily refresh did not succeed with all required steps healthy'
[[ -f "${daily_home}/.refine/last-refresh-ok" ]] \
  || fail 'daily refresh omitted success marker after complete success'

printf 'All runtime wrapper tests passed\n'
