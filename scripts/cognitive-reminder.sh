#!/usr/bin/env bash
# Cognitive refresh staleness check — run from SessionStart hook.
# Warns when daily-refresh.sh has not succeeded in the last 36 hours.
set -euo pipefail

TIMESTAMP_FILE="$HOME/.refine/last-refresh-ok"
THRESHOLD_HOURS=36

if [[ ! -f "$TIMESTAMP_FILE" ]]; then
  echo "⚠️  [refine] daily-refresh.sh has never run successfully. Run it manually or check your launchd plist." >&2
  exit 0
fi

if [[ "$(uname)" == "Darwin" ]]; then
  mtime=$(stat -f %m "$TIMESTAMP_FILE")
else
  mtime=$(stat -c %Y "$TIMESTAMP_FILE")
fi

now=$(date +%s)
age_seconds=$(( now - mtime ))

if [[ "$age_seconds" -gt $(( THRESHOLD_HOURS * 3600 )) ]]; then
  age_hours=$(( age_seconds / 3600 ))
  echo "⚠️  [refine] Last daily refresh was ${age_hours}h ago (threshold: ${THRESHOLD_HOURS}h). Check ~/.refine/hooks-error.log or run scripts/daily-refresh.sh manually." >&2
fi
