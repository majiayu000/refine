#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_ROOT=$(mktemp -d)
TEST_ROOT=$(cd "$TEST_ROOT" && pwd -P)
REPORT_DATE=$(date '+%Y-%m-%d')
REPORT_BASE="cognitive-portrait-${REPORT_DATE}-v4"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

prepare_case() {
  local name="$1"
  local root="${TEST_ROOT}/${name}"
  mkdir -p "$root/project/skills/cognitive-portrait" "$root/portraits/evidence" \
    "$root/bin" "$root/home" "$root/state"
  printf '# Index\n' > "$root/portraits/INDEX.md"
  printf '# prior\n' > "$root/portraits/cognitive-portrait-2026-01-01-v3.md"
  printf '# trusted skill\n' > "$root/project/skills/cognitive-portrait/SKILL.md"
  cat > "$root/bin/fake-agent" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ -z "${FAKE_AGENT_LOG:-}" ]] || printf 'started\n' >> "$FAKE_AGENT_LOG"
if [[ "${FAKE_ASSERT_ISOLATION:-0}" == "1" ]]; then
  for secret in ANTHROPIC_API_KEY ANTHROPIC_BASE_URL GOOGLE_API_KEY GEMINI_API_KEY XAI_API_KEY GROK_API_KEY BASE_API_KEY BASE_URL OPENAI_BASE_URL OPENAI_API_BASE; do
    [[ -z "${!secret:-}" ]] || exit 88
  done
  for required in --ephemeral --ignore-user-config --ignore-rules --skip-git-repo-check; do
    [[ " $* " == *" ${required} "* ]] || exit 89
  done
fi
case "${FAKE_AGENT_MODE:-normal}" in
  normal) printf '# candidate\n' > "$REFINE_COGNITIVE_PORTRAIT_OUTPUT" ;;
  exit) printf '# incomplete\n' > "$REFINE_COGNITIVE_PORTRAIT_OUTPUT"; exit 7 ;;
  tamper-bundle)
    printf '{"metric":999}\n' > "$REFINE_COGNITIVE_PORTRAIT_BUNDLE"
    printf '# candidate\n' > "$REFINE_COGNITIVE_PORTRAIT_OUTPUT"
    ;;
  tamper-history)
    printf 'attacker index\n' > "$FAKE_INDEX_TARGET"
    printf 'attacker history\n' > "$FAKE_HISTORY_TARGET"
    printf '# candidate\n' > "$REFINE_COGNITIVE_PORTRAIT_OUTPUT"
    ;;
  index-symlink)
    rm -f -- "$FAKE_INDEX_TARGET"
    ln -s "$FAKE_VICTIM" "$FAKE_INDEX_TARGET"
    printf '# candidate\n' > "$REFINE_COGNITIVE_PORTRAIT_OUTPUT"
    exit 7
    ;;
  candidate-symlink) ln -s "$FAKE_VICTIM" "$REFINE_COGNITIVE_PORTRAIT_OUTPUT" ;;
  tamper-validator)
    printf '#!/usr/bin/env bash\nprintf exploited > "%s"\n' "$FAKE_VALIDATOR_MARKER" > "$FAKE_VALIDATOR_TARGET"
    chmod 700 "$FAKE_VALIDATOR_TARGET"
    printf '# candidate\n' > "$REFINE_COGNITIVE_PORTRAIT_OUTPUT"
    ;;
  swap-directory)
    mv "$FAKE_PORTRAIT_TARGET" "${FAKE_PORTRAIT_TARGET}.moved"
    ln -s "$FAKE_VICTIM" "$FAKE_PORTRAIT_TARGET"
    printf '# candidate\n' > "$REFINE_COGNITIVE_PORTRAIT_OUTPUT"
    ;;
  sleep)
    printf '# candidate\n' > "$REFINE_COGNITIVE_PORTRAIT_OUTPUT"
    (sleep 10; printf survived > "$FAKE_DESCENDANT_MARKER") &
    wait
    ;;
  *) exit 90 ;;
esac
EOF
  cat > "$root/bin/fake-collector" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
output=""
while [[ $# -gt 0 ]]; do
  case "$1" in --output) output="$2"; shift 2 ;; *) shift ;; esac
done
printf '{"metric":1}\n' > "$output"
EOF
  cat > "$root/bin/fake-validator" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
output="" bundle=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    --bundle) bundle="$2"; shift 2 ;;
    *) shift ;;
  esac
done
[[ -z "${FAKE_VALIDATOR_MARKER:-}" ]] || printf called > "$FAKE_VALIDATOR_MARKER"
grep -q '"metric":1' "$bundle"
printf '{"passed":true}\n' > "$output"
exit "${FAKE_VALIDATOR_EXIT:-0}"
EOF
  chmod 700 "$root/bin/fake-agent" "$root/bin/fake-collector" "$root/bin/fake-validator"
}

run_case() {
  local name="$1" mode="${2:-normal}" validator_exit="${3:-0}"
  local root="${TEST_ROOT}/${name}"
  prepare_case "$name"
  env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" \
    REFINE_ROOT="$root/project" \
    REFINE_PORTRAIT_DIR="$root/portraits" \
    REFINE_PORTRAIT_STATE_ROOT="$root/state" \
    REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" \
    REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
    REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" \
    REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
    FAKE_AGENT_MODE="$mode" FAKE_VALIDATOR_EXIT="$validator_exit" \
    FAKE_ASSERT_ISOLATION=1 ANTHROPIC_API_KEY=ambient-secret BASE_URL=https://ambient.invalid \
    FAKE_INDEX_TARGET="$root/portraits/INDEX.md" \
    FAKE_HISTORY_TARGET="$root/portraits/cognitive-portrait-2026-01-01-v3.md" \
    FAKE_PORTRAIT_TARGET="$root/portraits" \
    FAKE_VALIDATOR_TARGET="$root/bin/fake-validator" \
    FAKE_VALIDATOR_MARKER="$root/validator.called" \
    FAKE_VICTIM="$root/victim" \
    bash "$SCRIPT_DIR/cognitive-portrait.sh"
}

run_case success
[[ -f "$TEST_ROOT/success/portraits/${REPORT_BASE}.md" ]] || fail 'report not published'
[[ -f "$TEST_ROOT/success/portraits/evidence/${REPORT_BASE}.bundle.json" ]] || fail 'bundle not published'
[[ -f "$TEST_ROOT/success/portraits/evidence/${REPORT_BASE}.quality.json" ]] || fail 'quality not published'
grep -q "$REPORT_BASE" "$TEST_ROOT/success/portraits/INDEX.md" || fail 'index not updated by host'

run_case nonzero exit && fail 'nonzero agent accepted'
[[ ! -e "$TEST_ROOT/nonzero/portraits/${REPORT_BASE}.md" ]] || fail 'failed candidate entered archive'

run_case validator-fail normal 9 && fail 'validator failure accepted'
[[ ! -e "$TEST_ROOT/validator-fail/portraits/${REPORT_BASE}.md" ]] || fail 'failed validation published report'
[[ "$(cat "$TEST_ROOT/validator-fail/portraits/INDEX.md")" == '# Index' ]] || fail 'failed validation changed index'

prepare_case evidence-file
rm -rf "$TEST_ROOT/evidence-file/portraits/evidence"
printf unsafe > "$TEST_ROOT/evidence-file/portraits/evidence"
run_root="$TEST_ROOT/evidence-file"
if env -i HOME="$run_root/home" PATH="$run_root/bin:/usr/bin:/bin" \
  REFINE_ROOT="$run_root/project" REFINE_PORTRAIT_DIR="$run_root/portraits" \
  REFINE_PORTRAIT_STATE_ROOT="$run_root/state" REFINE_PORTRAIT_AGENT="$run_root/bin/fake-agent" \
  REFINE_PORTRAIT_COLLECTOR="$run_root/bin/fake-collector" REFINE_PORTRAIT_VALIDATOR="$run_root/bin/fake-validator" \
  REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 FAKE_AGENT_MODE=normal FAKE_VALIDATOR_EXIT=0 \
  bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
  fail 'evidence path file accepted'
fi
[[ ! -e "$run_root/portraits/${REPORT_BASE}.md" ]] || fail 'transaction left report'

prepare_case failed-file
printf unsafe > "$TEST_ROOT/failed-file/portraits/.failed"
root="$TEST_ROOT/failed-file"
if env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project" \
  REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/state" \
  REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
  REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
  FAKE_AGENT_MODE=normal bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
  fail '.failed regular file accepted'
fi
[[ ! -e "$root/portraits/${REPORT_BASE}.md" ]] || fail '.failed preflight left report'

prepare_case trusted-script-links
root="$TEST_ROOT/trusted-script-links"
mv "$root/bin/fake-collector" "$root/bin/collector-real"
ln -s "$root/bin/collector-real" "$root/bin/fake-collector"
if env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project" \
  REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/state" \
  REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
  REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
  FAKE_AGENT_MODE=normal bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
  fail 'symlink collector accepted'
fi
rm "$root/bin/fake-collector"
ln "$root/bin/collector-real" "$root/bin/fake-collector"
if env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project" \
  REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/state" \
  REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
  REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
  FAKE_AGENT_MODE=normal bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
  fail 'hard-linked collector accepted'
fi

prepare_case state-parent-symlink
root="$TEST_ROOT/state-parent-symlink"
mkdir "$root/real-state"
ln -s "$root/real-state" "$root/linked-state"
if env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project" \
  REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/linked-state/runs" \
  REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
  REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
  FAKE_AGENT_MODE=normal bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
  fail 'symlink run-state parent accepted'
fi

run_case bundle-tamper tamper-bundle
grep -q '"metric":1' "$TEST_ROOT/bundle-tamper/portraits/evidence/${REPORT_BASE}.bundle.json" \
  || fail 'agent bundle copy replaced trusted archived bundle'

run_case history-tamper tamper-history && fail 'history mutation accepted'
[[ "$(cat "$TEST_ROOT/history-tamper/portraits/INDEX.md")" == '# Index' ]] || fail 'index was not restored'
[[ "$(cat "$TEST_ROOT/history-tamper/portraits/cognitive-portrait-2026-01-01-v3.md")" == '# prior' ]] \
  || fail 'history was not restored'

prepare_case index-symlink
printf 'victim-safe\n' > "$TEST_ROOT/index-symlink/victim"
root="$TEST_ROOT/index-symlink"
if env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project" \
  REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/state" \
  REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
  REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
  FAKE_AGENT_MODE=index-symlink FAKE_INDEX_TARGET="$root/portraits/INDEX.md" FAKE_VICTIM="$root/victim" \
  bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
  fail 'index symlink attack accepted'
fi
[[ "$(cat "$root/victim")" == 'victim-safe' ]] || fail 'rollback followed index symlink'
[[ ! -L "$root/portraits/INDEX.md" && "$(cat "$root/portraits/INDEX.md")" == '# Index' ]] \
  || fail 'index was not safely restored'

prepare_case candidate-symlink
printf 'victim-safe\n' > "$TEST_ROOT/candidate-symlink/victim"
root="$TEST_ROOT/candidate-symlink"
if env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project" \
  REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/state" \
  REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
  REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
  FAKE_AGENT_MODE=candidate-symlink FAKE_VICTIM="$root/victim" bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
  fail 'candidate symlink accepted'
fi
[[ "$(cat "$root/victim")" == 'victim-safe' ]] || fail 'candidate handling changed victim'

run_case validator-tamper tamper-validator && fail 'validator tamper accepted'
[[ ! -e "$TEST_ROOT/validator-tamper/validator.called" ]] || fail 'modified validator executed'

prepare_case crash-recovery
root="$TEST_ROOT/crash-recovery"
crash_env=(HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project"
  REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/state"
  REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector"
  REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0
  FAKE_INDEX_TARGET="$root/portraits/INDEX.md" FAKE_HISTORY_TARGET="$root/portraits/cognitive-portrait-2026-01-01-v3.md")
if env -i "${crash_env[@]}" FAKE_AGENT_MODE=normal REFINE_PORTRAIT_FAILPOINT=after-bundle \
  bash "$SCRIPT_DIR/cognitive-portrait.sh" >/dev/null 2>&1; then
  fail 'SIGKILL publication failpoint succeeded'
fi
[[ -f "$root/portraits/.portrait-publish.journal" ]] || fail 'crash journal was not durable'
if env -i "${crash_env[@]}" FAKE_AGENT_MODE=exit bash "$SCRIPT_DIR/cognitive-portrait.sh" >/dev/null 2>&1; then
  fail 'recovery probe unexpectedly succeeded'
fi
[[ ! -e "$root/portraits/.portrait-publish.journal" \
  && ! -e "$root/portraits/.portrait-publish.index-backup" ]] || fail 'recovery state remained'
[[ ! -e "$root/portraits/${REPORT_BASE}.md" \
  && ! -e "$root/portraits/evidence/${REPORT_BASE}.bundle.json" \
  && ! -e "$root/portraits/evidence/${REPORT_BASE}.quality.json" ]] || fail 'crash recovery left partial artifacts'
[[ "$(cat "$root/portraits/INDEX.md")" == '# Index' ]] || fail 'crash recovery did not restore index'

for path_kind in report evidence; do
  prepare_case "${path_kind}-symlink"
  root="$TEST_ROOT/${path_kind}-symlink"
  printf 'victim-safe\n' > "$root/victim"
  if [[ "$path_kind" == report ]]; then
    ln -s "$root/victim" "$root/portraits/${REPORT_BASE}.md"
  else
    ln -s "$root/victim" "$root/portraits/evidence/${REPORT_BASE}.bundle.json"
  fi
  if env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project" \
    REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/state" \
    REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
    REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
    FAKE_AGENT_MODE=normal bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
    fail "${path_kind} symlink accepted"
  fi
  [[ "$(cat "$root/victim")" == 'victim-safe' ]] || fail "${path_kind} symlink changed victim"
done

prepare_case directory-swap
root="$TEST_ROOT/directory-swap"
mkdir "$root/victim-dir"
printf 'victim-safe\n' > "$root/victim-dir/victim"
if env -i HOME="$root/home" PATH="$root/bin:/usr/bin:/bin" REFINE_ROOT="$root/project" \
  REFINE_PORTRAIT_DIR="$root/portraits" REFINE_PORTRAIT_STATE_ROOT="$root/state" \
  REFINE_PORTRAIT_AGENT="$root/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$root/bin/fake-collector" \
  REFINE_PORTRAIT_VALIDATOR="$root/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0 \
  FAKE_AGENT_MODE=swap-directory FAKE_PORTRAIT_TARGET="$root/portraits" FAKE_VICTIM="$root/victim-dir" \
  bash "$SCRIPT_DIR/cognitive-portrait.sh"; then
  fail 'portrait directory swap accepted'
fi
[[ "$(cat "$root/victim-dir/victim")" == 'victim-safe' ]] || fail 'directory swap changed victim'

concurrent="$TEST_ROOT/concurrent"
prepare_case concurrent
common=(HOME="$concurrent/home" PATH="$concurrent/bin:/usr/bin:/bin" REFINE_ROOT="$concurrent/project"
  REFINE_PORTRAIT_DIR="$concurrent/portraits" REFINE_PORTRAIT_STATE_ROOT="$concurrent/state"
  REFINE_PORTRAIT_AGENT="$concurrent/bin/fake-agent" REFINE_PORTRAIT_COLLECTOR="$concurrent/bin/fake-collector"
  REFINE_PORTRAIT_VALIDATOR="$concurrent/bin/fake-validator" REFINE_PORTRAIT_MIN_INTERVAL_DAYS=0
  REFINE_PORTRAIT_LOCK_WAIT_SECONDS=0 FAKE_AGENT_MODE=sleep FAKE_AGENT_LOG="$concurrent/agent.log"
  FAKE_DESCENDANT_MARKER="$concurrent/descendant")
env -i "${common[@]}" bash "$SCRIPT_DIR/cognitive-portrait.sh" > "$concurrent/first.log" 2>&1 &
first=$!
for _ in 1 2 3 4 5; do [[ -e "$concurrent/agent.log" ]] && break; sleep 1; done
if env -i "${common[@]}" FAKE_AGENT_MODE=normal bash "$SCRIPT_DIR/cognitive-portrait.sh" >/dev/null 2>&1; then
  fail 'concurrent run acquired lock'
fi
kill -TERM "$first"
wait "$first" && fail 'interrupted run succeeded'
sleep 1
[[ ! -e "$concurrent/descendant" ]] || fail 'agent descendant survived interruption'
[[ ! -e "$concurrent/portraits/${REPORT_BASE}.md" ]] || fail 'interrupted staging published report'

echo 'All cognitive portrait wrapper tests passed'
