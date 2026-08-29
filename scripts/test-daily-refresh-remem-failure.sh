#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TEST_HOME=$(mktemp -d)
trap 'rm -rf "$TEST_HOME"' EXIT

mkdir -p "$TEST_HOME/.cargo/bin" "$TEST_HOME/.refine"
cat > "$TEST_HOME/.cargo/bin/refine" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" > "$HOME/refine-args"
exit 42
SH
cat > "$TEST_HOME/.cargo/bin/mirror" <<'SH'
#!/usr/bin/env bash
touch "$HOME/mirror-was-called"
SH
chmod +x "$TEST_HOME/.cargo/bin/refine" "$TEST_HOME/.cargo/bin/mirror"

set +e
HOME="$TEST_HOME" \
  REFINE_RUNTIME_LOCK_ACTIVE=1 \
  REFINE_OPENAI_API_KEY=test-only-key \
  REFINE_INGEST_LATEST=37 \
  "$SCRIPT_DIR/daily-refresh.sh" > "$TEST_HOME/output.log" 2>&1
status=$?
set -e

[[ "$status" -ne 0 ]] || {
  echo "daily refresh unexpectedly succeeded" >&2
  exit 1
}
[[ "$(cat "$TEST_HOME/refine-args")" == "ingest-sessions --latest 37" ]] || {
  echo "daily refresh did not use the bounded Remem-only command" >&2
  exit 1
}
[[ ! -e "$TEST_HOME/mirror-was-called" ]] || {
  echo "mirror ran after Remem ingestion failed" >&2
  exit 1
}
[[ ! -e "$TEST_HOME/.refine/last-refresh-ok" ]] || {
  echo "success marker was written after Remem ingestion failed" >&2
  exit 1
}
grep -q "refusing to refresh derived reports" "$TEST_HOME/output.log"

echo "daily-refresh Remem failure contract: ok"
