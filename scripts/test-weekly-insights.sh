#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT

CALLS_FILE="${TEST_ROOT}/calls"
FAKE_REFINE="${TEST_ROOT}/refine"

cat > "$FAKE_REFINE" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$REFINE_TEST_CALLS"
EOF
chmod +x "$FAKE_REFINE"

env \
  HOME="$TEST_ROOT" \
  PATH="/usr/bin:/bin" \
  REFINE_BIN="$FAKE_REFINE" \
  REFINE_TEST_CALLS="$CALLS_FILE" \
  REFINE_RUNTIME_LOCK_ACTIVE=1 \
  ANTHROPIC_API_KEY="test-only-key" \
  bash "${SCRIPT_DIR}/weekly-insights.sh" >/dev/null

grep -Fxq 'insights --period 7 --prescription' "$CALLS_FILE" || {
  printf 'FAIL: weekly insights did not use an explicit 7-day delta window\n' >&2
  exit 1
}

if grep -Fxq 'insights --prescription' "$CALLS_FILE"; then
  printf 'FAIL: weekly insights silently requested a full-history report\n' >&2
  exit 1
fi

printf 'PASS weekly insights uses an explicit 7-day delta window\n'
