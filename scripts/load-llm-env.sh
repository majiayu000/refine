#!/usr/bin/env bash

load_refine_llm_env() {
  if [[ -n "${REFINE_ANTHROPIC_API_KEY:-}${ANTHROPIC_AUTH_TOKEN:-}${ANTHROPIC_API_KEY:-}${REFINE_OPENAI_API_KEY:-}${OPENAI_API_KEY:-}${BASE_API_KEY:-}" ]]; then
    return 0
  fi

  if ! command -v zsh >/dev/null 2>&1; then
    return 0
  fi

  local line key value
  while IFS= read -r line; do
    case "$line" in
      BASE_API_KEY=* | BASE_MODEL=* | BASE_URL=*)
        key="${line%%=*}"
        value="${line#*=}"
        if [[ -n "$value" ]]; then
          export "${key}=${value}"
        fi
        ;;
    esac
  done < <(probe_refine_llm_env_from_zsh)
}

probe_refine_llm_env_from_zsh() {
  local timeout_secs tmp pid watchdog status
  timeout_secs="${REFINE_LLM_ENV_LOAD_TIMEOUT_SECS:-10}"
  tmp=$(mktemp "${TMPDIR:-/tmp}/refine-llm-env.XXXXXX") || return 0

  zsh -fc '
      [[ -r "$HOME/.zshrc" ]] && source "$HOME/.zshrc" >/dev/null 2>&1
      for key in BASE_API_KEY BASE_MODEL BASE_URL; do
        value="${(P)key}"
        if [[ -n "$value" ]]; then
          print -r -- "$key=$value"
        fi
      done
    ' >"$tmp" 2>/dev/null &
  pid=$!

  (
    sleep "$timeout_secs"
    kill "$pid" 2>/dev/null || true
  ) &
  watchdog=$!

  wait "$pid" 2>/dev/null
  status=$?
  kill "$watchdog" 2>/dev/null || true
  wait "$watchdog" 2>/dev/null || true

  if [[ "$status" -eq 0 || -s "$tmp" ]]; then
    cat "$tmp"
  fi
  rm -f "$tmp"
}
