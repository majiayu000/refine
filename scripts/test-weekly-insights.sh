#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT

CALLS_FILE="${TEST_ROOT}/calls"
REMEM_PATH_FILE="${TEST_ROOT}/remem-path"
RUNTIME_PATH_FILE="${TEST_ROOT}/runtime-path"
FAKE_REFINE="${TEST_ROOT}/refine"
FAKE_REMEM="${TEST_ROOT}/.cargo/bin/remem"

mkdir -p "$(dirname "$FAKE_REMEM")"

cat > "$FAKE_REMEM" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$FAKE_REMEM"

cat > "$FAKE_REFINE" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$REFINE_TEST_CALLS"
if [[ "${1:-}" == "ingest-sessions" ]]; then
  command -v remem > "$REFINE_TEST_REMEM_PATH"
  printf '%s\n' "$PATH" > "$REFINE_TEST_RUNTIME_PATH"
fi
EOF
chmod +x "$FAKE_REFINE"

env \
  HOME="$TEST_ROOT" \
  PATH="/usr/bin:/bin" \
  REFINE_BIN="$FAKE_REFINE" \
  REFINE_TEST_CALLS="$CALLS_FILE" \
  REFINE_TEST_REMEM_PATH="$REMEM_PATH_FILE" \
  REFINE_TEST_RUNTIME_PATH="$RUNTIME_PATH_FILE" \
  REFINE_RUNTIME_LOCK_ACTIVE=1 \
  ANTHROPIC_API_KEY="test-only-key" \
  bash "${SCRIPT_DIR}/weekly-insights.sh" >/dev/null

resolved_remem="$(cat "$REMEM_PATH_FILE")"
case "$resolved_remem" in
  /opt/homebrew/bin/remem|/usr/local/bin/remem|"$FAKE_REMEM") ;;
  *)
    printf 'FAIL: weekly insights launchd PATH resolved Remem outside trusted Homebrew/Cargo locations\n' >&2
    exit 1
    ;;
esac

[[ "$(cat "$RUNTIME_PATH_FILE")" == "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${TEST_ROOT}/.cargo/bin:/usr/bin:/bin" ]] || {
  printf 'FAIL: weekly insights did not export the trusted Homebrew/Cargo PATH\n' >&2
  exit 1
}

grep -Fxq 'insights --period 7 --prescription' "$CALLS_FILE" || {
  printf 'FAIL: weekly insights did not use an explicit 7-day delta window\n' >&2
  exit 1
}

if grep -Fxq 'insights --prescription' "$CALLS_FILE"; then
  printf 'FAIL: weekly insights silently requested a full-history report\n' >&2
  exit 1
fi

printf 'PASS weekly insights uses an explicit 7-day delta window\n'
