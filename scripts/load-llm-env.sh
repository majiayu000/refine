#!/usr/bin/env bash

# Shared, non-interactive LLM environment loader.
#
# This file intentionally does not source any shell startup file.  It parses a
# small allowlist itself so a scheduled job cannot execute arbitrary shell code
# from ~/.zshrc or from an env file.

# Used by configure-llm-env.sh when it imports already-exported variables.
# shellcheck disable=SC2034
REFINE_LLM_ENV_SUPPORTED_KEYS=(
  REFINE_ANTHROPIC_API_KEY
  ANTHROPIC_AUTH_TOKEN
  ANTHROPIC_API_KEY
  REFINE_ANTHROPIC_MODEL
  REFINE_ANTHROPIC_BASE_URL
  ANTHROPIC_BASE_URL
  REFINE_OPENAI_API_KEY
  OPENAI_API_KEY
  REFINE_OPENAI_MODEL
  REFINE_OPENAI_BASE_URL
  BASE_API_KEY
  BASE_MODEL
  BASE_URL
)

refine_llm_env_error() {
  printf 'ERROR: %s\n' "$*" >&2
}

refine_llm_env_file_path() {
  if [[ -n "${REFINE_LLM_ENV_FILE:-}" ]]; then
    printf '%s\n' "$REFINE_LLM_ENV_FILE"
  elif [[ -n "${HOME:-}" ]]; then
    printf '%s/.refine/llm.env\n' "$HOME"
  else
    return 1
  fi
}

refine_llm_env_is_supported_key() {
  case "$1" in
    REFINE_ANTHROPIC_API_KEY|ANTHROPIC_AUTH_TOKEN|ANTHROPIC_API_KEY|\
    REFINE_ANTHROPIC_MODEL|REFINE_ANTHROPIC_BASE_URL|ANTHROPIC_BASE_URL|\
    REFINE_OPENAI_API_KEY|OPENAI_API_KEY|REFINE_OPENAI_MODEL|\
    REFINE_OPENAI_BASE_URL|BASE_API_KEY|BASE_MODEL|BASE_URL)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

refine_llm_env_is_api_key() {
  case "$1" in
    REFINE_ANTHROPIC_API_KEY|ANTHROPIC_AUTH_TOKEN|ANTHROPIC_API_KEY|\
    REFINE_OPENAI_API_KEY|OPENAI_API_KEY|BASE_API_KEY)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

refine_llm_env_has_api_key() {
  [[ -n "${REFINE_ANTHROPIC_API_KEY:-}" || \
    -n "${ANTHROPIC_AUTH_TOKEN:-}" || \
    -n "${ANTHROPIC_API_KEY:-}" || \
    -n "${REFINE_OPENAI_API_KEY:-}" || \
    -n "${OPENAI_API_KEY:-}" || \
    -n "${BASE_API_KEY:-}" ]]
}

refine_llm_env_key_group_state() {
  case "$1" in
    anthropic)
      if [[ -n "${REFINE_ANTHROPIC_API_KEY:-}" || \
        -n "${ANTHROPIC_AUTH_TOKEN:-}" || \
        -n "${ANTHROPIC_API_KEY:-}" ]]; then
        printf '<set>'
      else
        printf '<unset>'
      fi
      ;;
    openai)
      if [[ -n "${REFINE_OPENAI_API_KEY:-}" || -n "${OPENAI_API_KEY:-}" ]]; then
        printf '<set>'
      else
        printf '<unset>'
      fi
      ;;
    base)
      if [[ -n "${BASE_API_KEY:-}" ]]; then
        printf '<set>'
      else
        printf '<unset>'
      fi
      ;;
    *)
      printf '<unset>'
      ;;
  esac
}

refine_llm_env_status() {
  printf 'anthropic_key=%s openai_key=%s base_key=%s\n' \
    "$(refine_llm_env_key_group_state anthropic)" \
    "$(refine_llm_env_key_group_state openai)" \
    "$(refine_llm_env_key_group_state base)"
}

# Parse one dotenv-like assignment without eval or command substitution.
# Return 0 for a supported assignment, 1 for an ignored line, and 2 for a
# malformed/unsupported line in strict mode. Parsed values are returned through
# REFINE_LLM_ENV_PARSED_KEY and REFINE_LLM_ENV_PARSED_VALUE.
refine_llm_env_parse_assignment() {
  local line="$1"
  local strict="${2:-1}"
  local key value inner

  REFINE_LLM_ENV_PARSED_KEY=''
  REFINE_LLM_ENV_PARSED_VALUE=''
  REFINE_LLM_ENV_PARSE_ERROR=''

  line="${line%$'\r'}"
  while [[ "$line" == [[:space:]]* ]]; do
    line="${line#?}"
  done
  if [[ -z "$line" || "$line" == \#* ]]; then
    return 1
  fi

  case "$line" in
    export[[:space:]]*)
      line="${line#export}"
      while [[ "$line" == [[:space:]]* ]]; do
        line="${line#?}"
      done
      ;;
  esac

  if [[ "$line" != *=* ]]; then
    if [[ "$strict" == '1' ]]; then
      REFINE_LLM_ENV_PARSE_ERROR='malformed assignment'
      return 2
    fi
    return 1
  fi

  key="${line%%=*}"
  while [[ "$key" == *[[:space:]] ]]; do
    key="${key%?}"
  done
  if [[ -z "$key" || "$key" == *[[:space:]]* ]]; then
    REFINE_LLM_ENV_PARSE_ERROR='malformed variable name'
    return 2
  fi

  if ! refine_llm_env_is_supported_key "$key"; then
    if [[ "$strict" == '1' ]]; then
      REFINE_LLM_ENV_PARSE_ERROR='unsupported variable'
      return 2
    fi
    return 1
  fi

  value="${line#*=}"
  while [[ "$value" == [[:space:]]* ]]; do
    value="${value#?}"
  done

  case "$value" in
    \'*)
      case "$value" in
        *\')
          inner="${value:1:${#value}-2}"
          if [[ "$inner" == *\'* ]]; then
            REFINE_LLM_ENV_PARSE_ERROR='unsupported quoted value'
            return 2
          fi
          ;;
        *)
          REFINE_LLM_ENV_PARSE_ERROR='unterminated quoted value'
          return 2
          ;;
      esac
      ;;
    \"*)
      case "$value" in
        *\")
          inner="${value:1:${#value}-2}"
          case "$inner" in
            *\\*|*\$*|*\`*|*\"*)
              REFINE_LLM_ENV_PARSE_ERROR='unsupported quoted value'
              return 2
              ;;
          esac
          ;;
        *)
          REFINE_LLM_ENV_PARSE_ERROR='unterminated quoted value'
          return 2
          ;;
      esac
      ;;
    *[[:space:]]*)
      REFINE_LLM_ENV_PARSE_ERROR='whitespace in unquoted value'
      return 2
      ;;
    *\'*|*\"*)
      REFINE_LLM_ENV_PARSE_ERROR='malformed quoted value'
      return 2
      ;;
    *\$\(*|*\`*|*\;*|*\&*|*\|*|*\<*|*\>*|*\{*|*\}*)
      REFINE_LLM_ENV_PARSE_ERROR='unsupported shell syntax'
      return 2
      ;;
    *)
      inner="$value"
      ;;
  esac

  REFINE_LLM_ENV_PARSED_KEY="$key"
  REFINE_LLM_ENV_PARSED_VALUE="$inner"
  return 0
}

refine_llm_env_validate_content() {
  local path="$1"
  local strict="${2:-1}"
  local line line_number=0 parse_rc key seen_keys=' '

  REFINE_LLM_ENV_FILE_HAS_API_KEY=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line_number=$((line_number + 1))
    if refine_llm_env_parse_assignment "$line" "$strict"; then
      parse_rc=0
    else
      parse_rc=$?
    fi
    if [[ "$parse_rc" -eq 1 ]]; then
      continue
    fi
    if [[ "$parse_rc" -ne 0 ]]; then
      refine_llm_env_error "${path}:${line_number}: ${REFINE_LLM_ENV_PARSE_ERROR}; only supported literal LLM variables are allowed"
      return 1
    fi

    key="$REFINE_LLM_ENV_PARSED_KEY"
    case "$seen_keys" in
      *" $key "*)
        refine_llm_env_error "${path}:${line_number}: duplicate definition for ${key}"
        return 1
        ;;
    esac
    seen_keys="${seen_keys}${key} "
    if refine_llm_env_is_api_key "$key" && [[ -n "$REFINE_LLM_ENV_PARSED_VALUE" ]]; then
      REFINE_LLM_ENV_FILE_HAS_API_KEY=1
    fi
  done < "$path"
}

refine_llm_env_stat_owner_uid() {
  local path="$1"
  if [[ "$(uname -s)" == 'Darwin' ]]; then
    stat -f '%u' "$path"
  else
    stat -c '%u' "$path"
  fi
}

refine_llm_env_stat_mode() {
  local path="$1"
  if [[ "$(uname -s)" == 'Darwin' ]]; then
    stat -f '%Lp' "$path"
  else
    stat -c '%a' "$path"
  fi
}

refine_llm_env_validate_secure_file() {
  local path="$1"
  local owner_uid current_uid mode

  if [[ -L "$path" ]]; then
    refine_llm_env_error "secure LLM env file is a symlink and was rejected: ${path}"
    return 1
  fi
  if [[ ! -f "$path" ]]; then
    refine_llm_env_error "secure LLM env file is not a regular file: ${path}"
    return 1
  fi

  owner_uid="$(refine_llm_env_stat_owner_uid "$path" 2>/dev/null || true)"
  current_uid="$(id -u)"
  if [[ -z "$owner_uid" || "$owner_uid" != "$current_uid" ]]; then
    refine_llm_env_error "secure LLM env file has the wrong owner (expected current user): ${path}"
    return 1
  fi

  mode="$(refine_llm_env_stat_mode "$path" 2>/dev/null || true)"
  case "$mode" in
    ''|*[!0-7]*)
      refine_llm_env_error "cannot inspect secure LLM env file permissions: ${path}"
      return 1
      ;;
  esac
  if [[ "${mode: -2}" != '00' ]]; then
    refine_llm_env_error "secure LLM env file must have no group/other permission bits (use chmod 600): ${path}"
    return 1
  fi

  refine_llm_env_validate_content "$path" 1
}

refine_llm_env_load_file() {
  local path="$1"
  local strict="${2:-1}"
  local skip_api_keys="${3:-0}"
  local line parse_rc key value seen_keys=' '

  while IFS= read -r line || [[ -n "$line" ]]; do
    if refine_llm_env_parse_assignment "$line" "$strict"; then
      parse_rc=0
    else
      parse_rc=$?
    fi
    if [[ "$parse_rc" -eq 1 ]]; then
      continue
    fi
    if [[ "$parse_rc" -ne 0 ]]; then
      refine_llm_env_error "${path}: ${REFINE_LLM_ENV_PARSE_ERROR}; only supported literal LLM variables are allowed"
      return 1
    fi

    key="$REFINE_LLM_ENV_PARSED_KEY"
    case "$seen_keys" in
      *" $key "*)
        refine_llm_env_error "${path}: duplicate definition for ${key}"
        return 1
        ;;
    esac
    seen_keys="${seen_keys}${key} "
    if [[ "$skip_api_keys" == '1' ]] && refine_llm_env_is_api_key "$key"; then
      continue
    fi
    value="$REFINE_LLM_ENV_PARSED_VALUE"
    if [[ -z "${!key:-}" ]]; then
      export "$key=$value"
    fi
  done < "$path"
}

load_refine_llm_env_impl() {
  local project_file="${1:-}"
  local require_key="${2:-1}"
  local secure_file secure_has_key project_has_key
  local secure_exists=0 project_exists=0

  if refine_llm_env_has_api_key; then
    REFINE_LLM_ENV_SOURCE='process'
    export REFINE_LLM_ENV_SOURCE
    return 0
  fi

  if ! secure_file="$(refine_llm_env_file_path)"; then
    refine_llm_env_error 'HOME is not set; cannot determine the secure LLM env file'
    return 1
  fi

  secure_has_key=0
  project_has_key=0
  REFINE_LLM_ENV_SOURCE='none'

  if [[ -L "$secure_file" || -e "$secure_file" ]]; then
    secure_exists=1
    if ! refine_llm_env_validate_secure_file "$secure_file"; then
      return 1
    fi
    secure_has_key="$REFINE_LLM_ENV_FILE_HAS_API_KEY"
    if ! refine_llm_env_load_file "$secure_file" 1 0; then
      return 1
    fi
  fi

  if [[ -n "$project_file" && -f "$project_file" ]]; then
    project_exists=1
    if ! refine_llm_env_validate_content "$project_file" 0; then
      return 1
    fi
    project_has_key="$REFINE_LLM_ENV_FILE_HAS_API_KEY"
    if [[ "$secure_has_key" == '1' ]]; then
      if ! refine_llm_env_load_file "$project_file" 0 1; then
        return 1
      fi
    elif ! refine_llm_env_load_file "$project_file" 0 0; then
      return 1
    fi
  fi

  if [[ "$secure_has_key" == '1' ]]; then
    REFINE_LLM_ENV_SOURCE='secure-file'
  elif [[ "$project_has_key" == '1' ]]; then
    REFINE_LLM_ENV_SOURCE='project-env'
  fi
  export REFINE_LLM_ENV_SOURCE

  if ! refine_llm_env_has_api_key; then
    if [[ "$require_key" != '1' ]]; then
      return 0
    fi
    local project_description='disabled'
    if [[ "$project_exists" == '1' ]]; then
      project_description="$project_file"
    fi
    if [[ "$secure_exists" == '1' ]]; then
      refine_llm_env_error "no supported LLM API key loaded; secure file was checked at ${secure_file} and project fallback was ${project_description}; run scripts/configure-llm-env.sh --check"
    else
      refine_llm_env_error "no supported LLM API key loaded; secure file is absent at ${secure_file} and project fallback was ${project_description}; run scripts/configure-llm-env.sh --check"
    fi
    return 1
  fi
  return 0
}

load_refine_llm_env() {
  load_refine_llm_env_impl "${1:-}" 1
}

load_refine_llm_env_optional() {
  load_refine_llm_env_impl "${1:-}" 0
}
