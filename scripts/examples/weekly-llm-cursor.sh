#!/usr/bin/env bash
# Weekly LLM command backed by the Cursor agent CLI.
#
# Install as ~/.refine/bin/weekly-llm (chmod +x) to route the weekly job's
# ingest and insights through Cursor instead of the API provider. Reads the
# prompt on stdin and prints the answer on stdout.
#
# The agent runs read-only (--mode ask) in an empty directory so it cannot
# touch any project files.
set -euo pipefail

CURSOR_BIN="${REFINE_CURSOR_BIN:-${HOME}/.local/bin/cr}"
WORKDIR="${REFINE_CURSOR_WORKDIR:-${HOME}/.refine/cursor-llm-workdir}"
mkdir -p "$WORKDIR"
cd "$WORKDIR"

prompt="$(cat)"
args=(-p --trust --mode ask --output-format text)
if [[ -n "${REFINE_CURSOR_MODEL:-}" ]]; then
  args+=(--model "$REFINE_CURSOR_MODEL")
fi
exec "$CURSOR_BIN" "${args[@]}" "$prompt" </dev/null
