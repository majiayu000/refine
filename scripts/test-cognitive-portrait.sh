#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

run_case() {
  local case_name="$1"
  local agent_exit="$2"
  local update_index="$3"
  local case_root="${TEST_ROOT}/${case_name}"
  local portrait_dir="${case_root}/portraits"
  local bin_dir="${case_root}/bin"
  mkdir -p "$portrait_dir" "$bin_dir" "${case_root}/home"
  printf '# Index\n' > "${portrait_dir}/INDEX.md"
  cat > "${bin_dir}/fake-agent" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
portrait="${REFINE_PORTRAIT_DIR}/cognitive-portrait-2026-08-12-v9.md"
printf '# candidate\n' > "$portrait"
if [[ "$FAKE_UPDATE_INDEX" == "1" ]]; then
  printf '| [2026-08-12](./cognitive-portrait-2026-08-12-v9.md) |\n' >> "${REFINE_PORTRAIT_DIR}/INDEX.md"
fi
if [[ -n "${FAKE_AGENT_SLEEP:-}" ]]; then
  sleep "$FAKE_AGENT_SLEEP"
fi
exit "$FAKE_AGENT_EXIT"
EOF
  chmod 700 "${bin_dir}/fake-agent"

  env -i HOME="${case_root}/home" PATH="${bin_dir}:/usr/bin:/bin" \
    REFINE_PORTRAIT_DIR="$portrait_dir" \
    REFINE_PORTRAIT_AGENT="${bin_dir}/fake-agent" \
    REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
    FAKE_AGENT_EXIT="$agent_exit" FAKE_UPDATE_INDEX="$update_index" \
    bash "${SCRIPT_DIR}/cognitive-portrait.sh"
}

run_case success 0 1 || fail 'complete indexed portrait was rejected'
[[ -f "${TEST_ROOT}/success/portraits/cognitive-portrait-2026-08-12-v9.md" ]] \
  || fail 'complete portrait was not retained'

if run_case nonzero 7 1; then
  fail 'nonzero agent run was accepted'
fi
[[ ! -f "${TEST_ROOT}/nonzero/portraits/cognitive-portrait-2026-08-12-v9.md" ]] \
  || fail 'failed portrait remained eligible for throttling'
find "${TEST_ROOT}/nonzero/portraits/.failed" -type f -name '*.failed' | grep -q . \
  || fail 'failed portrait was not quarantined'
[[ "$(cat "${TEST_ROOT}/nonzero/portraits/INDEX.md")" == '# Index' ]] \
  || fail 'failed run left a dangling index entry'

if run_case missing-index 0 0; then
  fail 'unindexed portrait was accepted'
fi
[[ ! -f "${TEST_ROOT}/missing-index/portraits/cognitive-portrait-2026-08-12-v9.md" ]] \
  || fail 'unindexed portrait remained eligible for throttling'

interrupt_root="${TEST_ROOT}/interrupted"
mkdir -p "${interrupt_root}/portraits" "${interrupt_root}/bin" "${interrupt_root}/home"
printf '# Index\n' > "${interrupt_root}/portraits/INDEX.md"
cp "${TEST_ROOT}/success/bin/fake-agent" "${interrupt_root}/bin/fake-agent"
env -i HOME="${interrupt_root}/home" PATH="${interrupt_root}/bin:/usr/bin:/bin" \
  REFINE_PORTRAIT_DIR="${interrupt_root}/portraits" \
  REFINE_PORTRAIT_AGENT="${interrupt_root}/bin/fake-agent" \
  REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
  FAKE_AGENT_EXIT=0 FAKE_UPDATE_INDEX=1 FAKE_AGENT_SLEEP=10 \
  bash "${SCRIPT_DIR}/cognitive-portrait.sh" >/dev/null 2>&1 &
wrapper_pid=$!
for _attempt in 1 2 3 4 5; do
  [[ -f "${interrupt_root}/portraits/cognitive-portrait-2026-08-12-v9.md" ]] && break
  sleep 1
done
kill -TERM "$wrapper_pid"
if wait "$wrapper_pid"; then
  fail 'interrupted portrait wrapper exited successfully'
fi
[[ ! -f "${interrupt_root}/portraits/cognitive-portrait-2026-08-12-v9.md" ]] \
  || fail 'interrupted portrait remained eligible for throttling'
[[ "$(cat "${interrupt_root}/portraits/INDEX.md")" == '# Index' ]] \
  || fail 'interrupted run left a dangling index entry'

echo 'All cognitive portrait wrapper tests passed'
