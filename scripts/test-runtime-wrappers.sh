#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/refine-runtime-wrapper-test.XXXXXX")
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

# Legacy second-only and precise markers must sort correctly within the same
# whole second; this is the boundary that plain RFC 3339 string comparison gets wrong.
# shellcheck source=scripts/quota-time.sh
source "${SCRIPT_DIR}/quota-time.sh"
second_key=$(quota_timestamp_sort_key '2026-08-13T12:34:57Z')
precise_key=$(quota_timestamp_sort_key '2026-08-13T12:34:57.789123456Z')
[[ "$precise_key" > "$second_key" ]] \
  || fail 'precise quota marker did not sort after the same whole second'
[[ "$second_key" == '2026-08-13T12:34:57.000000000Z' ]] \
  || fail 'legacy quota marker did not normalize to fixed precision'
if quota_timestamp_sort_key '2026-02-31T12:34:57Z' >/dev/null 2>&1; then
  fail 'quota timestamp parser accepted an invalid calendar date'
fi

home="${TEST_ROOT}/home"
mkdir -p "${home}/.refine"
chmod 700 "${home}/.refine"
printf '%s\n' "export BASE_API_KEY='wrapper-secret'
export BASE_URL='https://wrapper.example.invalid'" > "${home}/.refine/llm.env"
chmod 600 "${home}/.refine/llm.env"
printf '%s\n' 'token-secret' > "${home}/.refine/server.token"
chmod 600 "${home}/.refine/server.token"

fake_server="${TEST_ROOT}/fake-server.sh"
cat > "$fake_server" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "${BASE_API_KEY:-}" == 'wrapper-secret' ]]
[[ "${BASE_URL:-}" == 'https://wrapper.example.invalid' ]]
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
run_refine_runtime_job_locked true || fail 'runtime job lock could not run a child command'
[[ -f "$lock_file" ]] || fail 'runtime job lock did not create its lock file'
printf '%s\n' '99999999' > "$lock_file"
run_refine_runtime_job_locked true || fail 'runtime job lock treated a stale file as an owner'
if run_refine_runtime_job_locked bash -c 'exit 17'; then
  fail 'runtime job lock discarded the child exit status'
fi

# Exercise every installed backend. Two simultaneous contenders must remain
# strictly serialized even when a stale lock file already exists on disk.
lock_worker="${TEST_ROOT}/runtime-lock-worker.sh"
cat > "$lock_worker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
source "$RUNTIME_LOCK_HELPER"
run_refine_runtime_job_locked bash -c \
  'printf "start %s\n" "$$" >> "$RUNTIME_CRITICAL_LOG"; sleep 1; printf "end %s\n" "$$" >> "$RUNTIME_CRITICAL_LOG"'
EOF
chmod 700 "$lock_worker"
for lock_backend in flock lockf; do
  command -v "$lock_backend" >/dev/null 2>&1 || continue
  printf '%s\n' '99999999' > "$lock_file"
  critical_log="${TEST_ROOT}/runtime-critical-${lock_backend}.log"
  for _worker in 1 2; do
    env HOME="$home" REFINE_RUNTIME_JOB_LOCK_FILE="$lock_file" \
      REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS=10 \
      REFINE_RUNTIME_LOCK_BACKEND="$lock_backend" \
      RUNTIME_LOCK_HELPER="${SCRIPT_DIR}/runtime-job-lock.sh" \
      RUNTIME_CRITICAL_LOG="$critical_log" \
      bash "$lock_worker" &
  done
  wait
  critical_shape="$(awk '{print $1}' "$critical_log" | paste -sd, -)"
  [[ "$critical_shape" == 'start,end,start,end' ]] \
    || fail "runtime ${lock_backend} contenders overlapped: ${critical_shape}"
done

# A successful ingest must not publish a success marker when required advice
# fails. The scheduled command must also request strict advice semantics.
daily_home="${TEST_ROOT}/daily-home"
mkdir -p "${daily_home}/.refine" "${daily_home}/.cargo/bin"
chmod 700 "${daily_home}/.refine"
printf '%s\n' "export BASE_API_KEY='daily-secret'
export BASE_URL='https://daily.example.invalid'" > "${daily_home}/.refine/llm.env"
chmod 600 "${daily_home}/.refine/llm.env"
cat > "${daily_home}/.cargo/bin/refine" <<'EOF'
#!/usr/bin/env bash
if [[ -n "${FAKE_REFINE_LOG:-}" ]]; then
  printf 'called\n' >> "$FAKE_REFINE_LOG"
fi
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

# Quota markers are written by Rust with optional nanosecond precision. The
# wrapper must compare both legacy second-only and precise forms correctly.
quota_refine_log="${TEST_ROOT}/quota-refine.log"
future_year=$((10#$(date -u +%Y) + 1))
printf '%04d-01-01T00:00:00.000000001Z\n' "$future_year" \
  > "${daily_home}/.refine/quota_exhausted_until"
output=$(env -i HOME="$daily_home" PATH="/usr/bin:/bin" \
  FAKE_REFINE_LOG="$quota_refine_log" REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS=0 \
  bash "${SCRIPT_DIR}/daily-refresh.sh" 2>&1) \
  || fail 'daily refresh failed while honoring a precise future quota marker'
[[ "$output" == *'skipping refresh'* ]] \
  || fail 'daily refresh ignored a precise future quota marker'
[[ ! -e "$quota_refine_log" ]] \
  || fail 'daily refresh called refine despite a precise future quota marker'

printf '%s\n' '2000-01-01T00:00:00Z' \
  > "${daily_home}/.refine/quota_exhausted_until"
env -i HOME="$daily_home" PATH="/usr/bin:/bin" \
  FAKE_REFINE_LOG="$quota_refine_log" REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS=0 \
  bash "${SCRIPT_DIR}/daily-refresh.sh" >/dev/null 2>&1 \
  || fail 'daily refresh rejected a legacy expired quota marker'
[[ -s "$quota_refine_log" ]] \
  || fail 'daily refresh skipped work for a legacy expired quota marker'

: > "$quota_refine_log"
printf '%s\n' 'not-a-timestamp' > "${daily_home}/.refine/quota_exhausted_until"
output=$(env -i HOME="$daily_home" PATH="/usr/bin:/bin" \
  FAKE_REFINE_LOG="$quota_refine_log" REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS=0 \
  bash "${SCRIPT_DIR}/daily-refresh.sh" 2>&1) \
  || fail 'daily refresh did not fail open for a malformed quota marker'
[[ "$output" == *'WARN: ignoring malformed quota marker'* ]] \
  || fail 'daily refresh did not report a malformed quota marker'
[[ -s "$quota_refine_log" ]] \
  || fail 'daily refresh skipped work for a malformed quota marker'

printf 'All runtime wrapper tests passed\n'
