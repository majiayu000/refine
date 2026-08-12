#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SERVER_BIN="${1:-${HOME}/.cargo/bin/refine-server}"
PROJECT_ENV="${2:-${SCRIPT_DIR}/../.env}"

# Mark every process boundary in both append-only launchd logs so an error from
# an earlier server cannot be mistaken for one from the current process.
startup="[refine-server] $(date '+%Y-%m-%d %H:%M:%S') starting pid=$$"
printf '%s\n' "$startup"
printf '%s\n' "$startup" >&2

# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
if ! load_refine_llm_env_optional "$PROJECT_ENV"; then
  printf '[refine-server] invalid LLM credential configuration; refusing to start\n' >&2
  exit 1
fi

if refine_llm_env_has_api_key; then
  printf '[refine-server] LLM source=%s\n' "${REFINE_LLM_ENV_SOURCE:-process}"
else
  printf '[refine-server] LLM credentials unavailable; extraction is disabled\n' >&2
fi

if [[ -n "${REFINE_API_TOKEN_FILE:-}" ]]; then
  token_file="$REFINE_API_TOKEN_FILE"
  if [[ -L "$token_file" || ! -f "$token_file" ]]; then
    printf '[refine-server] invalid token file: %s\n' "$token_file" >&2
    exit 1
  fi
  if [[ "$(uname -s)" == 'Darwin' ]]; then
    token_mode=$(stat -f '%Lp' "$token_file" 2>/dev/null || true)
  else
    token_mode=$(stat -c '%a' "$token_file" 2>/dev/null || true)
  fi
  if [[ -z "$token_mode" || $((8#$token_mode & 077)) -ne 0 ]]; then
    printf '[refine-server] token file must have no group/other permission bits: %s\n' "$token_file" >&2
    exit 1
  fi
  IFS= read -r token < "$token_file" || true
  if [[ -z "$token" ]]; then
    printf '[refine-server] token file is empty: %s\n' "$token_file" >&2
    exit 1
  fi
  export REFINE_API_TOKEN="$token"
fi

exec "$SERVER_BIN"
