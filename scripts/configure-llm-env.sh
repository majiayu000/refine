#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"

# Keep every temporary and backup private.  This script is the only workflow
# that intentionally copies an LLM credential during an explicit migration.
umask 077

usage() {
  cat <<'EOF'
Usage: scripts/configure-llm-env.sh [--check | --migrate | --from-env | --from-file PATH]

Modes:
  --check      Validate the secure file and report whether ~/.zshrc is ready
               to migrate. This is the default and never writes files.
  --migrate    Migrate literal export BASE_URL, BASE_API_KEY, and BASE_MODEL
               definitions from ~/.zshrc into ~/.refine/llm.env.
  --from-env   Write supported variables that are already exported in this
               process into ~/.refine/llm.env. This does not edit ~/.zshrc.
  --from-file  Safely parse supported literal variables from PATH and write
               them into ~/.refine/llm.env. Other variables are ignored.

The migration mode is intentionally limited to literal definitions. It does
not evaluate ~/.zshrc, run zsh startup code, or accept shell expressions.
EOF
}

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

home_dir="${HOME:-}"
[[ -n "$home_dir" ]] || die 'HOME is not set'

mode=''
source_file=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --check)
      [[ -z "$mode" ]] || die 'choose exactly one configuration mode'
      mode='check'
      ;;
    --migrate)
      [[ -z "$mode" ]] || die 'choose exactly one configuration mode'
      mode='migrate'
      ;;
    --from-env)
      [[ -z "$mode" ]] || die 'choose exactly one configuration mode'
      mode='from-env'
      ;;
    --from-file)
      [[ -z "$mode" ]] || die 'choose exactly one configuration mode'
      mode='from-file'
      shift
      [[ $# -gt 0 ]] || die '--from-file requires a path'
      source_file="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
  shift
done
[[ -n "$mode" ]] || mode='check'

secure_file="$(refine_llm_env_file_path)" || die 'cannot determine the secure LLM env file'
zshrc_file="${REFINE_ZSHRC_FILE:-${home_dir}/.zshrc}"
source_begin='# >>> Refine LLM env (managed) >>>'
source_end='# <<< Refine LLM env (managed) <<<'

tmp_env=''
tmp_zshrc=''
cleanup() {
  [[ -z "$tmp_env" || ! -e "$tmp_env" ]] || rm -f "$tmp_env"
  [[ -z "$tmp_zshrc" || ! -e "$tmp_zshrc" ]] || rm -f "$tmp_zshrc"
}
trap cleanup EXIT HUP INT TERM

write_quoted_assignment() {
  local key="$1"
  local value="$2"
  if [[ "$value" == *\'* ]]; then
    die "${key} contains an unsupported single quote; refusing to write it"
  fi
  printf "export %s='%s'\n" "$key" "$value"
}

zshrc_line_mentions_migration_key() {
  local line="$1" key

  # Use the same exported-assignment prefix accepted by the shared parser.
  # Extract only the left-hand side so names such as MIMO_BASE_URL and shell
  # references in unrelated commands never enter strict migration parsing.
  case "$line" in
    export[[:space:]]*)
      line="${line#export}"
      while [[ "$line" == [[:space:]]* ]]; do
        line="${line#?}"
      done
      ;;
    *)
      return 1
      ;;
  esac

  if [[ "$line" == *=* ]]; then
    key="${line%%=*}"
  else
    # Keep malformed exact exports in scope so the strict parser can fail
    # closed instead of silently ignoring them.
    key="$line"
  fi
  while [[ "$key" == *[[:space:]] ]]; do
    key="${key%?}"
  done

  case "$key" in
    BASE_URL|BASE_API_KEY|BASE_MODEL)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

MIGRATION_BASE_URL=''
MIGRATION_BASE_API_KEY=''
MIGRATION_BASE_MODEL=''
MIGRATION_REMOVE_LINES=' '
MIGRATION_DEFINITION_COUNT=0
MIGRATION_SOURCE_BLOCK_COUNT=0
MIGRATION_SOURCE_END_COUNT=0

collect_zsh_definitions() {
  local line line_number=0 parse_rc trimmed key value

  MIGRATION_BASE_URL=''
  MIGRATION_BASE_API_KEY=''
  MIGRATION_BASE_MODEL=''
  MIGRATION_REMOVE_LINES=' '
  MIGRATION_DEFINITION_COUNT=0
  MIGRATION_SOURCE_BLOCK_COUNT=0
  MIGRATION_SOURCE_END_COUNT=0

  if [[ -L "$zshrc_file" ]]; then
    die "zshrc is a symlink and was rejected: ${zshrc_file}"
  fi
  if [[ ! -f "$zshrc_file" ]]; then
    return 0
  fi

  while IFS= read -r line || [[ -n "$line" ]]; do
    line_number=$((line_number + 1))
    line="${line%$'\r'}"
    trimmed="$line"
    while [[ "$trimmed" == [[:space:]]* ]]; do
      trimmed="${trimmed#?}"
    done

    if [[ "$trimmed" == "$source_begin" ]]; then
      MIGRATION_SOURCE_BLOCK_COUNT=$((MIGRATION_SOURCE_BLOCK_COUNT + 1))
      continue
    fi
    if [[ "$trimmed" == "$source_end" ]]; then
      MIGRATION_SOURCE_END_COUNT=$((MIGRATION_SOURCE_END_COUNT + 1))
      continue
    fi
    [[ -z "$trimmed" || "$trimmed" == \#* ]] && continue
    zshrc_line_mentions_migration_key "$trimmed" || continue

    case "$trimmed" in
      export[[:space:]]*)
        ;;
      *)
        die "${zshrc_file}:${line_number}: BASE_* definitions must be literal exported assignments"
        ;;
    esac

    if refine_llm_env_parse_assignment "$trimmed" 1; then
      parse_rc=0
    else
      parse_rc=$?
    fi
    if [[ "$parse_rc" -ne 0 ]]; then
      die "${zshrc_file}:${line_number}: malformed BASE_* definition; refusing migration"
    fi
    key="$REFINE_LLM_ENV_PARSED_KEY"
    value="$REFINE_LLM_ENV_PARSED_VALUE"
    case "$key" in
      BASE_URL)
        [[ -n "$MIGRATION_BASE_URL" ]] && die "${zshrc_file}:${line_number}: duplicate BASE_URL definition"
        MIGRATION_BASE_URL="$value"
        ;;
      BASE_API_KEY)
        [[ -n "$MIGRATION_BASE_API_KEY" ]] && die "${zshrc_file}:${line_number}: duplicate BASE_API_KEY definition"
        MIGRATION_BASE_API_KEY="$value"
        ;;
      BASE_MODEL)
        [[ -n "$MIGRATION_BASE_MODEL" ]] && die "${zshrc_file}:${line_number}: duplicate BASE_MODEL definition"
        MIGRATION_BASE_MODEL="$value"
        ;;
      *)
        die "${zshrc_file}:${line_number}: only BASE_URL, BASE_API_KEY, and BASE_MODEL may be migrated"
        ;;
    esac
    [[ -n "$value" ]] || die "${zshrc_file}:${line_number}: ${key} has no value"
    MIGRATION_REMOVE_LINES="${MIGRATION_REMOVE_LINES}${line_number} "
    MIGRATION_DEFINITION_COUNT=$((MIGRATION_DEFINITION_COUNT + 1))
  done < "$zshrc_file"

  if [[ "$MIGRATION_SOURCE_BLOCK_COUNT" -gt 1 || "$MIGRATION_SOURCE_END_COUNT" -gt 1 || \
    "$MIGRATION_SOURCE_BLOCK_COUNT" -ne "$MIGRATION_SOURCE_END_COUNT" ]]; then
    die "${zshrc_file}: malformed or duplicate Refine LLM source block"
  fi
}

require_complete_migration_definitions() {
  [[ "$MIGRATION_BASE_URL" != '' ]] || die "missing literal export BASE_URL=... in ${zshrc_file}"
  [[ "$MIGRATION_BASE_API_KEY" != '' ]] || die "missing literal export BASE_API_KEY=... in ${zshrc_file}"
  [[ "$MIGRATION_BASE_MODEL" != '' ]] || die "missing literal export BASE_MODEL=... in ${zshrc_file}"
}

line_is_removed() {
  local line_number="$1"
  case "$MIGRATION_REMOVE_LINES" in
    *" ${line_number} "*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

validate_transformed_zshrc() {
  local path="$1"
  local line trimmed begin_count=0 end_count=0

  while IFS= read -r line || [[ -n "$line" ]]; do
    trimmed="$line"
    while [[ "$trimmed" == [[:space:]]* ]]; do
      trimmed="${trimmed#?}"
    done
    if [[ "$trimmed" == "$source_begin" ]]; then
      begin_count=$((begin_count + 1))
    elif [[ "$trimmed" == "$source_end" ]]; then
      end_count=$((end_count + 1))
    elif [[ -n "$trimmed" && "$trimmed" != \#* ]] && zshrc_line_mentions_migration_key "$trimmed"; then
      die "${path}: migrated BASE_* definition remains; refusing replacement"
    fi
  done < "$path"

  [[ "$begin_count" -eq 1 && "$end_count" -eq 1 ]] || die "${path}: generated source block validation failed"
  if command -v zsh >/dev/null 2>&1 && ! zsh -n "$path" >/dev/null 2>&1; then
    die "${path}: generated zsh syntax validation failed"
  fi
}

secure_file_is_present() {
  [[ -L "$secure_file" || -e "$secure_file" ]]
}

secure_file_has_usable_provider() (
  local key
  for key in "${REFINE_LLM_ENV_SUPPORTED_KEYS[@]}"; do
    unset "$key"
  done
  refine_llm_env_load_file "$secure_file" 1 0 \
    && refine_llm_env_has_usable_provider
)

write_managed_source_block() {
  printf '\n%s\n' "$source_begin"
  printf '%s\n' "if [ -r \"\${REFINE_LLM_ENV_FILE:-\$HOME/.refine/llm.env}\" ]; then"
  printf '%s\n' "  . \"\${REFINE_LLM_ENV_FILE:-\$HOME/.refine/llm.env}\""
  printf '%s\n' 'fi'
  printf '%s\n' "$source_end"
}

backup_zshrc() {
  local backup_dir="${home_dir}/.refine/backups" backup_index=0

  mkdir -p "$backup_dir"
  chmod 700 "$backup_dir" || die "cannot secure backup directory: ${backup_dir}"
  MIGRATION_BACKUP_FILE="${backup_dir}/zshrc.$(date +%Y%m%d%H%M%S).bak"
  while [[ -e "$MIGRATION_BACKUP_FILE" ]]; do
    backup_index=$((backup_index + 1))
    MIGRATION_BACKUP_FILE="${backup_dir}/zshrc.$(date +%Y%m%d%H%M%S).${backup_index}.bak"
  done
  cp "$zshrc_file" "$MIGRATION_BACKUP_FILE" || die 'cannot create zshrc backup'
  chmod 600 "$MIGRATION_BACKUP_FILE"
}

check_mode() {
  local secure_present=0 secure_ready=0

  if secure_file_is_present; then
    secure_present=1
    refine_llm_env_validate_secure_file "$secure_file" || exit 1
    if secure_file_has_usable_provider; then
      secure_ready=1
      printf 'CHECK: secure LLM env file has a usable provider (values withheld)\n'
    else
      printf 'CHECK: secure LLM env file is valid but has no usable provider\n'
    fi
  else
    printf 'CHECK: secure LLM env file is not present yet: %s\n' "$secure_file"
  fi

  if [[ -f "$zshrc_file" ]]; then
    collect_zsh_definitions
    if [[ "$MIGRATION_DEFINITION_COUNT" -gt 0 ]]; then
      require_complete_migration_definitions
      [[ "$MIGRATION_DEFINITION_COUNT" -eq 3 ]] || die "${zshrc_file}: expected exactly one BASE_URL, BASE_API_KEY, and BASE_MODEL definition"
      [[ "$secure_present" -eq 0 ]] \
        || die "secure LLM env file already exists; --migrate would refuse to overwrite it while BASE_* definitions remain in ${zshrc_file}"
      printf 'CHECK: ~/.zshrc has 3 literal BASE_* definitions ready for --migrate (no changes made)\n'
    elif [[ "$secure_ready" == '1' ]]; then
      printf 'CHECK: no pending BASE_* migration; secure credentials are available\n'
    else
      die "no complete literal BASE_* definitions found and unattended credentials are not configured"
    fi
  elif [[ "$secure_ready" == '1' ]]; then
    printf 'CHECK: ~/.zshrc is absent; secure credentials are available\n'
  else
      die "${home_dir}/.zshrc is absent and unattended credentials are not configured"
  fi
}

migrate_mode() {
  local env_dir line line_number=0

  if secure_file_is_present; then
    refine_llm_env_validate_secure_file "$secure_file" || exit 1
  fi
  collect_zsh_definitions

  if [[ "$MIGRATION_DEFINITION_COUNT" -eq 0 ]]; then
    if secure_file_is_present && secure_file_has_usable_provider; then
      if [[ "$MIGRATION_SOURCE_BLOCK_COUNT" -eq 1 ]]; then
        printf 'MIGRATE: no pending literal BASE_* definitions; secure file and source block were left unchanged\n'
        return 0
      fi
      [[ -f "$zshrc_file" ]] || die "cannot add the managed source block because ${zshrc_file} does not exist"
      tmp_zshrc="$(mktemp "${zshrc_file}.tmp.XXXXXX")" || die 'cannot create zshrc temporary file'
      while IFS= read -r line || [[ -n "$line" ]]; do
        printf '%s\n' "$line" >> "$tmp_zshrc"
      done < "$zshrc_file"
      write_managed_source_block >> "$tmp_zshrc"
      chmod 600 "$tmp_zshrc"
      validate_transformed_zshrc "$tmp_zshrc"
      backup_zshrc
      mv "$tmp_zshrc" "$zshrc_file"
      tmp_zshrc=''
      printf 'MIGRATE: secure LLM env file was valid; added one managed source block; backup=%s\n' "$MIGRATION_BACKUP_FILE"
      return 0
    fi
    require_complete_migration_definitions
  fi
  require_complete_migration_definitions
  [[ "$MIGRATION_DEFINITION_COUNT" -eq 3 ]] || die "${zshrc_file}: expected exactly one BASE_URL, BASE_API_KEY, and BASE_MODEL definition"
  [[ "$MIGRATION_SOURCE_BLOCK_COUNT" -le 1 ]] || die "${zshrc_file}: duplicate Refine LLM source block"

  if secure_file_is_present; then
    die "secure LLM env file already exists; refusing to overwrite it while BASE_* definitions remain in ${zshrc_file}"
  fi
  [[ -f "$zshrc_file" ]] || die "cannot migrate because ${zshrc_file} does not exist"

  env_dir="${secure_file%/*}"
  [[ "$env_dir" != "$secure_file" ]] || env_dir='.'
  mkdir -p "$env_dir"
  chmod 700 "$env_dir" || die "cannot secure the LLM env directory: ${env_dir}"

  tmp_env="$(mktemp "${secure_file}.tmp.XXXXXX")" || die 'cannot create secure env temporary file'
  tmp_zshrc="$(mktemp "${zshrc_file}.tmp.XXXXXX")" || die 'cannot create zshrc temporary file'

  {
    write_quoted_assignment BASE_URL "$MIGRATION_BASE_URL"
    write_quoted_assignment BASE_API_KEY "$MIGRATION_BASE_API_KEY"
    write_quoted_assignment BASE_MODEL "$MIGRATION_BASE_MODEL"
  } > "$tmp_env"
  chmod 600 "$tmp_env"
  refine_llm_env_validate_secure_file "$tmp_env" || die 'generated secure env file failed validation'

  while IFS= read -r line || [[ -n "$line" ]]; do
    line_number=$((line_number + 1))
    if ! line_is_removed "$line_number"; then
      printf '%s\n' "$line" >> "$tmp_zshrc"
    fi
  done < "$zshrc_file"
  if [[ "$MIGRATION_SOURCE_BLOCK_COUNT" -eq 0 ]]; then
    write_managed_source_block >> "$tmp_zshrc"
  fi
  chmod 600 "$tmp_zshrc"
  validate_transformed_zshrc "$tmp_zshrc"

  backup_zshrc

  mv "$tmp_env" "$secure_file"
  tmp_env=''
  mv "$tmp_zshrc" "$zshrc_file"
  tmp_zshrc=''
  printf 'MIGRATE: wrote secure LLM env file and removed 3 literal BASE_* definitions; backup=%s\n' "$MIGRATION_BACKUP_FILE"
}

from_env_mode() {
  local env_dir key value found_api=0

  if secure_file_is_present; then
    refine_llm_env_validate_secure_file "$secure_file" || exit 1
    secure_file_has_usable_provider \
      || die 'secure LLM env file has no usable provider; BASE requires both BASE_API_KEY and BASE_URL'
    printf 'FROM-ENV: secure LLM env file already exists; no files changed\n'
    return 0
  fi
  refine_llm_env_has_usable_provider \
    || die 'no usable LLM provider configuration found; BASE requires both BASE_API_KEY and BASE_URL'

  env_dir="${secure_file%/*}"
  [[ "$env_dir" != "$secure_file" ]] || env_dir='.'
  mkdir -p "$env_dir"
  chmod 700 "$env_dir" || die "cannot secure the LLM env directory: ${env_dir}"
  tmp_env="$(mktemp "${secure_file}.tmp.XXXXXX")" || die 'cannot create secure env temporary file'

  for key in "${REFINE_LLM_ENV_SUPPORTED_KEYS[@]}"; do
    value="${!key:-}"
    if refine_llm_env_value_is_nonblank "$value"; then
      write_quoted_assignment "$key" "$value" >> "$tmp_env"
      if refine_llm_env_is_api_key "$key"; then
        found_api=1
      fi
    fi
  done
  [[ "$found_api" -eq 1 ]] || die 'no supported exported API key found; nothing was written'
  chmod 600 "$tmp_env"
  refine_llm_env_validate_secure_file "$tmp_env" || die 'generated secure env file failed validation'
  mv "$tmp_env" "$secure_file"
  tmp_env=''
  printf 'FROM-ENV: wrote supported exported variables to the secure LLM env file (values withheld)\n'
}

from_file_mode() {
  local key

  [[ -n "$source_file" ]] || die '--from-file requires a path'
  [[ ! -L "$source_file" ]] || die "source LLM env file is a symlink and was rejected: ${source_file}"
  [[ -f "$source_file" ]] || die "source LLM env file is not a regular file: ${source_file}"

  if secure_file_is_present; then
    refine_llm_env_validate_secure_file "$secure_file" || exit 1
    secure_file_has_usable_provider \
      || die 'secure LLM env file has no usable provider; BASE requires both BASE_API_KEY and BASE_URL'
    printf 'FROM-FILE: secure LLM env file already exists; no files changed\n'
    return 0
  fi

  refine_llm_env_validate_content "$source_file" 0 \
    || die "source LLM env file contains an invalid supported assignment: ${source_file}"
  [[ "$REFINE_LLM_ENV_FILE_HAS_API_KEY" == '1' ]] \
    || die "source LLM env file has no supported API key: ${source_file}"

  # Load only values parsed from the explicit file. Current process values must
  # not override or supplement a migration source chosen by the user.
  for key in "${REFINE_LLM_ENV_SUPPORTED_KEYS[@]}"; do
    unset "$key"
  done
  refine_llm_env_load_file "$source_file" 0 0 \
    || die "cannot parse source LLM env file: ${source_file}"
  from_env_mode
  printf 'FROM-FILE: imported supported literal variables (values withheld)\n'
}

case "$mode" in
  check)
    check_mode
    ;;
  migrate)
    migrate_mode
    ;;
  from-env)
    from_env_mode
    ;;
  from-file)
    from_file_mode
    ;;
esac
