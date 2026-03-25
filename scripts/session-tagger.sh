#!/usr/bin/env bash
# Claude Code Stop hook — async incremental ingest
#
# Usage (add to ~/.claude/settings.json hooks.Stop):
#   bash /path/to/refine/scripts/session-tagger.sh
#
# What it does:
#   Runs `refine ingest-sessions --latest 1` in the background so the most
#   recently modified session is ingested without blocking the Claude Code UI.
#   Output is appended to ~/.refine/incremental.log.
#
# Requirements:
#   - `refine` binary in PATH (or ~/.cargo/bin)
#   - LLM API key exported in the environment (e.g. ANTHROPIC_API_KEY)
#   - (optional) ~/.config/refine/.env or ~/.refine/.env for key injection

export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${HOME}/.cargo/bin:${PATH}"

LOG_DIR="${HOME}/.refine"
LOG_FILE="${LOG_DIR}/incremental.log"
ENV_FILE="${HOME}/.refine/.env"

mkdir -p "${LOG_DIR}"

# Load API keys from optional env file
if [[ -f "${ENV_FILE}" ]]; then
  set -a
  # shellcheck source=/dev/null
  source "${ENV_FILE}"
  set +a
fi

# Fire-and-forget: ingest the latest 1 session, do not block the hook
nohup refine ingest-sessions --latest 1 >> "${LOG_FILE}" 2>&1 &
