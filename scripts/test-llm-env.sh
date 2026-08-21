#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
LOADER="${SCRIPT_DIR}/load-llm-env.sh"
CONFIGURE="${SCRIPT_DIR}/configure-llm-env.sh"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/refine-llm-env-test.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

assert_contains() {
  local text="$1"
  local needle="$2"
  local label="$3"
  case "$text" in
    *"$needle"*)
      ;;
    *)
      fail "$label"
      ;;
  esac
}

assert_not_contains() {
  local text="$1"
  local needle="$2"
  local label="$3"
  case "$text" in
    *"$needle"*)
      fail "$label"
      ;;
    *)
      ;;
  esac
}

assert_equal() {
  local expected="$1"
  local actual="$2"
  local label="$3"
  [[ "$expected" == "$actual" ]] || fail "$label"
}

new_home() {
  local name="$1"
  local path="${TEST_ROOT}/${name}"
  mkdir -p "${path}/.refine"
  chmod 700 "${path}/.refine"
  printf '%s\n' "$path"
}

write_text() {
  local path="$1"
  local text="$2"
  printf '%s\n' "$text" > "$path"
}

file_mode() {
  local path="$1"
  if [[ "$(uname -s)" == 'Darwin' ]]; then
    stat -f '%Lp' "$path"
  else
    stat -c '%a' "$path"
  fi
}

assert_incomplete_base_rejected() {
  local label="$1"
  shift
  local output=''
  if output="$(env -i HOME="$TEST_ROOT" PATH="$PATH" REFINE_LOADER="$LOADER" "$@" \
    bash -c 'source "$REFINE_LOADER"; load_refine_llm_env' 2>&1)"; then
    fail "incomplete BASE provider was accepted: ${label}"
  fi
  assert_contains "$output" 'BASE requires both BASE_API_KEY and BASE_URL' \
    "incomplete BASE error was not actionable: ${label}"
  assert_not_contains "$output" 'incomplete-secret' \
    "incomplete BASE error leaked its key: ${label}"
}

unset_supported_command='unset REFINE_ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN ANTHROPIC_API_KEY REFINE_ANTHROPIC_MODEL REFINE_ANTHROPIC_BASE_URL ANTHROPIC_BASE_URL REFINE_OPENAI_API_KEY OPENAI_API_KEY REFINE_OPENAI_MODEL REFINE_OPENAI_BASE_URL BASE_API_KEY BASE_MODEL BASE_URL'

run_config() {
  local home="$1"
  shift
  env HOME="$home" \
    REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
    CONFIGURE_SCRIPT="$CONFIGURE" \
    bash -c "${unset_supported_command}; bash \"\$CONFIGURE_SCRIPT\" \"\$@\"" test-config "$@"
}

printf 'Testing shared LLM env loader and migration helper\n'

# Process credentials remain authoritative while unset companion settings are
# loaded from lower-priority files, and no credential value is printed.
home="$(new_home process-priority)"
write_text "${home}/.refine/llm.env" "export BASE_API_KEY='secure-secret'
export BASE_URL='https://secure.example.invalid'"
chmod 600 "${home}/.refine/llm.env"
write_text "${home}/project.env" "export BASE_API_KEY='project-secret'
export BASE_MODEL='project-model'"
process_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  REFINE_LOADER="$LOADER" \
  REFINE_PROJECT="${home}/project.env" \
  bash -c "${unset_supported_command}; export BASE_API_KEY='process-secret'; source \"\$REFINE_LOADER\"; load_refine_llm_env \"\$REFINE_PROJECT\"; [[ \"\$BASE_API_KEY\" == 'process-secret' && \"\$BASE_URL\" == 'https://secure.example.invalid' && \"\$BASE_MODEL\" == 'project-model' && \"\$REFINE_LLM_ENV_SOURCE\" == 'process' ]]; printf 'process-priority-ok\\n'" 2>&1)" || fail 'process-priority probe failed'
assert_contains "$process_output" 'process-priority-ok' 'process priority result missing'
assert_not_contains "$process_output" 'process-secret' 'process secret appeared in output'
assert_not_contains "$process_output" 'secure-secret' 'secure secret appeared in output'
assert_not_contains "$process_output" 'project-secret' 'project secret appeared in output'
printf 'PASS process environment priority\n'

# Whitespace-only process aliases are absent for precedence purposes and must
# not mask a usable value from the secure file.
home="$(new_home blank-process-alias)"
write_text "${home}/.refine/llm.env" "export ANTHROPIC_AUTH_TOKEN='secure-alias-secret'"
chmod 600 "${home}/.refine/llm.env"
blank_alias_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  REFINE_LOADER="$LOADER" \
  bash -c "${unset_supported_command}; export ANTHROPIC_AUTH_TOKEN=' '; source \"\$REFINE_LOADER\"; load_refine_llm_env; [[ \"\$ANTHROPIC_AUTH_TOKEN\" == 'secure-alias-secret' ]]; printf 'blank-alias-fallback-ok\\n'" 2>&1)" \
  || fail 'blank process alias masked the secure-file value'
assert_contains "$blank_alias_output" 'blank-alias-fallback-ok' 'blank alias fallback result missing'
assert_not_contains "$blank_alias_output" 'secure-alias-secret' 'blank alias fallback leaked its secret'
printf 'PASS blank process alias fallback\n'

# A whitespace-only key in the secure file is likewise absent and must not
# suppress a usable project fallback.
home="$(new_home blank-secure-alias)"
write_text "${home}/.refine/llm.env" "export ANTHROPIC_AUTH_TOKEN=' '"
chmod 600 "${home}/.refine/llm.env"
write_text "${home}/project.env" "export ANTHROPIC_API_KEY='project-alias-secret'"
blank_secure_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  REFINE_LOADER="$LOADER" \
  REFINE_PROJECT="${home}/project.env" \
  bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env \"\$REFINE_PROJECT\"; [[ \"\$ANTHROPIC_API_KEY\" == 'project-alias-secret' && \"\$REFINE_LLM_ENV_SOURCE\" == 'project-env' ]]; printf 'blank-secure-fallback-ok\\n'" 2>&1)" \
  || fail 'blank secure alias suppressed the project fallback'
assert_contains "$blank_secure_output" 'blank-secure-fallback-ok' 'blank secure fallback result missing'
assert_not_contains "$blank_secure_output" 'project-alias-secret' 'blank secure fallback leaked its secret'
printf 'PASS blank secure alias fallback\n'

# Lower-priority files are validated before their companion settings are used.
home="$(new_home process-insecure-lower)"
write_text "${home}/.refine/llm.env" "export BASE_URL='https://insecure.example.invalid'"
chmod 644 "${home}/.refine/llm.env"
if env HOME="$home" REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" REFINE_LOADER="$LOADER" \
  bash -c "${unset_supported_command}; export BASE_API_KEY='process-secret'; source \"\$REFINE_LOADER\"; load_refine_llm_env" >/dev/null 2>&1; then
  fail 'process key accepted companion settings from an insecure file'
fi
printf 'PASS process companion settings remain fail-closed\n'

process_only_output="$(env -i PATH="$PATH" REFINE_LOADER="$LOADER" REFINE_OPENAI_API_KEY='process-only-secret' \
  bash -c 'source "$REFINE_LOADER"; load_refine_llm_env; [[ "$REFINE_LLM_ENV_SOURCE" == process ]]; printf "process-only-ok\n"' 2>&1)" \
  || fail 'process-only credentials unexpectedly required HOME'
assert_contains "$process_only_output" 'process-only-ok' 'process-only result missing'
assert_not_contains "$process_only_output" 'process-only-secret' 'process-only secret appeared in output'
printf 'PASS process-only credentials without HOME\n'

assert_incomplete_base_rejected key-only "BASE_API_KEY=incomplete-secret"
assert_incomplete_base_rejected url-only "BASE_URL=https://incomplete.example.invalid"
assert_incomplete_base_rejected blank-key "BASE_API_KEY= " \
  "BASE_URL=https://incomplete.example.invalid"
assert_incomplete_base_rejected blank-url "BASE_API_KEY=incomplete-secret" "BASE_URL= "
printf 'PASS incomplete BASE provider rejection\n'

# Server startup may run without an LLM for query-only operation, while still
# rejecting malformed credential files.
home="$(new_home optional-empty)"
optional_output="$(env -i HOME="$home" PATH="$PATH" REFINE_LOADER="$LOADER" bash -c 'source "$REFINE_LOADER"; load_refine_llm_env_optional; [[ "$REFINE_LLM_ENV_SOURCE" == none ]]; printf "optional-empty-ok\n"' 2>&1)" \
  || fail 'optional loader rejected an absent credential file'
assert_contains "$optional_output" 'optional-empty-ok' 'optional loader result missing'
write_text "${home}/.refine/llm.env" 'export BASE_API_KEY=$(not-literal)'
chmod 600 "${home}/.refine/llm.env"
if env -i HOME="$home" PATH="$PATH" REFINE_LOADER="$LOADER" bash -c 'source "$REFINE_LOADER"; load_refine_llm_env_optional' >/dev/null 2>&1; then
  fail 'optional loader accepted malformed credentials'
fi
printf 'PASS optional server credential loading\n'

# Every supported alias is loaded as a literal assignment. This exercises the
# allowlist through the loader rather than only inspecting its implementation.
home="$(new_home allowlist)"
allowlist_file="${home}/.refine/llm.env"
write_text "$allowlist_file" "export REFINE_ANTHROPIC_API_KEY='alias-1'
export ANTHROPIC_AUTH_TOKEN='alias-2'
export ANTHROPIC_API_KEY='alias-3'
export REFINE_ANTHROPIC_MODEL='alias-4'
export REFINE_ANTHROPIC_BASE_URL='alias-5'
export ANTHROPIC_BASE_URL='alias-6'
export REFINE_OPENAI_API_KEY='alias-7'
export OPENAI_API_KEY='alias-8'
export REFINE_OPENAI_MODEL='alias-9'
export REFINE_OPENAI_BASE_URL='alias-10'
export BASE_API_KEY='alias-11'
export BASE_MODEL='alias-12'
export BASE_URL='alias-13'"
chmod 600 "$allowlist_file"
# shellcheck disable=SC2016
allowlist_output="$(env -i HOME="$home" PATH="$PATH" \
  REFINE_LLM_ENV_FILE="$allowlist_file" REFINE_LOADER="$LOADER" \
  bash -c '
    source "$REFINE_LOADER"
    load_refine_llm_env
    [[ "$REFINE_ANTHROPIC_API_KEY" == alias-1 ]]
    [[ "$ANTHROPIC_AUTH_TOKEN" == alias-2 ]]
    [[ "$ANTHROPIC_API_KEY" == alias-3 ]]
    [[ "$REFINE_ANTHROPIC_MODEL" == alias-4 ]]
    [[ "$REFINE_ANTHROPIC_BASE_URL" == alias-5 ]]
    [[ "$ANTHROPIC_BASE_URL" == alias-6 ]]
    [[ "$REFINE_OPENAI_API_KEY" == alias-7 ]]
    [[ "$OPENAI_API_KEY" == alias-8 ]]
    [[ "$REFINE_OPENAI_MODEL" == alias-9 ]]
    [[ "$REFINE_OPENAI_BASE_URL" == alias-10 ]]
    [[ "$BASE_API_KEY" == alias-11 ]]
    [[ "$BASE_MODEL" == alias-12 ]]
    [[ "$BASE_URL" == alias-13 ]]
    [[ "$REFINE_LLM_ENV_SOURCE" == secure-file ]]
    printf "allowlist-ok\\n"
  ' 2>&1)" || fail 'allowlist behavioral probe failed'
assert_contains "$allowlist_output" 'allowlist-ok' 'allowlist result missing'
assert_not_contains "$allowlist_output" 'alias-' 'allowlist values appeared in output'
printf 'PASS exact supported alias allowlist\n'

# A private regular file is accepted and selected in a clean process.
home="$(new_home secure-success)"
write_text "${home}/.refine/llm.env" "export BASE_API_KEY='secure-success-secret'
export BASE_URL='https://secure-success.example.invalid'"
chmod 600 "${home}/.refine/llm.env"
secure_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  REFINE_LOADER="$LOADER" \
  bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env; [[ \"\$BASE_API_KEY\" == 'secure-success-secret' && \"\$REFINE_LLM_ENV_SOURCE\" == 'secure-file' ]]; printf 'secure-file-ok\\n'" 2>&1)" || fail 'secure file probe failed'
assert_contains "$secure_output" 'secure-file-ok' 'secure file result missing'
assert_not_contains "$secure_output" 'secure-success-secret' 'secure file secret appeared in output'
assert_equal '600' "$(file_mode "${home}/.refine/llm.env")" 'secure file mode is not 0600'
printf 'PASS secure 0600 file\n'

# Group/other permissions are rejected before parsing.
home="$(new_home insecure-mode)"
write_text "${home}/.refine/llm.env" "export BASE_API_KEY='insecure-mode-secret'"
chmod 644 "${home}/.refine/llm.env"
insecure_output=''
if insecure_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  REFINE_LOADER="$LOADER" \
  bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env" 2>&1)"; then
  fail '0644 secure file was accepted'
fi
assert_contains "$insecure_output" 'permission' '0644 rejection was not actionable'
assert_not_contains "$insecure_output" 'insecure-mode-secret' 'insecure-file secret appeared in output'
printf 'PASS insecure 0644 rejection\n'

# Symlinks are rejected even when their target has an acceptable mode.
home="$(new_home symlink-rejection)"
write_text "${home}/target.env" "export BASE_API_KEY='symlink-secret'"
chmod 600 "${home}/target.env"
ln -s "${home}/target.env" "${home}/.refine/llm.env"
symlink_output=''
if symlink_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  REFINE_LOADER="$LOADER" \
  bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env" 2>&1)"; then
  fail 'secure env symlink was accepted'
fi
assert_contains "$symlink_output" 'symlink' 'symlink rejection was not actionable'
assert_not_contains "$symlink_output" 'symlink-secret' 'symlink secret appeared in output'
printf 'PASS symlink rejection\n'

# An explicit project fallback is parsed without source/eval and does not
# override a higher-priority process value.
home="$(new_home project-fallback)"
write_text "${home}/project.env" "# development-only fallback
export BASE_API_KEY='project-fallback-secret'
export BASE_URL='https://project-fallback.example.invalid'
REFINE_DB_PATH=/tmp/refine-test.db"
project_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  REFINE_LOADER="$LOADER" \
  REFINE_PROJECT="${home}/project.env" \
  bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env \"\$REFINE_PROJECT\"; [[ \"\$BASE_API_KEY\" == 'project-fallback-secret' && \"\$REFINE_LLM_ENV_SOURCE\" == 'project-env' ]]; printf 'project-fallback-ok\\n'" 2>&1)" || fail 'project fallback probe failed'
assert_contains "$project_output" 'project-fallback-ok' 'project fallback result missing'
assert_not_contains "$project_output" 'project-fallback-secret' 'project fallback secret appeared in output'
printf 'PASS project .env fallback\n'

# No supported key is a hard failure.
home="$(new_home missing-key)"
missing_output=''
if missing_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  REFINE_LOADER="$LOADER" \
  bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env" 2>&1)"; then
  fail 'missing-key loader probe unexpectedly succeeded'
fi
assert_contains "$missing_output" 'no usable LLM provider configuration' 'missing-key error was not actionable'
printf 'PASS missing-key failure\n'

# Every duplicate mode and every pair of distinct modes must be rejected.
home="$(new_home mode-rejection)"
mode_pairs=(
  '--check --migrate'
  '--migrate --check'
  '--check --from-env'
  '--from-env --check'
  '--migrate --from-env'
  '--from-env --migrate'
  '--check --check'
  '--migrate --migrate'
  '--from-env --from-env'
)
for mode_pair in "${mode_pairs[@]}"; do
  read -r first_mode second_mode <<< "$mode_pair"
  mode_output=''
  if mode_output="$(run_config "$home" "$first_mode" "$second_mode" 2>&1)"; then
    fail "multi-mode invocation unexpectedly succeeded: ${mode_pair}"
  fi
  assert_contains "$mode_output" 'choose exactly one configuration mode' "mode rejection was not actionable: ${mode_pair}"
done
printf 'PASS duplicate and multi-mode rejection\n'

# Migrate the three literal BASE_* definitions, verify the private backup and
# one source block, then prove a repeat invocation is a no-op.
home="$(new_home migration)"
write_text "${home}/.zshrc" $'# unrelated interactive setup
# export BASE_URL=commented-url
MIMO_BASE_URL=https://mimo.example.invalid
export MIMO_BASE_URL=https://mimo.example.invalid
export ANTHROPIC_BASE_URL=https://anthropic.example.invalid
export SOME_BASE_MODEL_SUFFIX=not-a-migration-key
printf "%s" "$BASE_URL"
echo "$BASE_API_KEY" "$BASE_MODEL"
export PATH=/usr/bin:/bin
  export BASE_URL = https://example.invalid/compat
export BASE_API_KEY = migration-secret
	 export BASE_MODEL = compat-model'
migration_expected="${home}/zshrc.expected"
write_text "$migration_expected" $'# unrelated interactive setup
# export BASE_URL=commented-url
MIMO_BASE_URL=https://mimo.example.invalid
export MIMO_BASE_URL=https://mimo.example.invalid
export ANTHROPIC_BASE_URL=https://anthropic.example.invalid
export SOME_BASE_MODEL_SUFFIX=not-a-migration-key
printf "%s" "$BASE_URL"
echo "$BASE_API_KEY" "$BASE_MODEL"
export PATH=/usr/bin:/bin

# >>> Refine LLM env (managed) >>>
if [ -r "${REFINE_LLM_ENV_FILE:-$HOME/.refine/llm.env}" ]; then
  . "${REFINE_LLM_ENV_FILE:-$HOME/.refine/llm.env}"
fi
# <<< Refine LLM env (managed) <<<'
migration_before_check="${home}/zshrc.before-check"
cp "${home}/.zshrc" "$migration_before_check"
migration_check=''
migration_check="$(run_config "$home" --check 2>&1)" || fail 'migration check rejected valid definitions'
assert_contains "$migration_check" 'ready for --migrate' 'migration check did not report readiness'
assert_not_contains "$migration_check" 'migration-secret' 'migration check leaked a secret'
cmp -s "$migration_before_check" "${home}/.zshrc" || fail 'migration check changed zshrc'
migration_output=''
migration_output="$(run_config "$home" --migrate 2>&1)" || fail 'migration failed for valid definitions'
assert_contains "$migration_output" 'backup=' 'migration did not report a backup path'
assert_not_contains "$migration_output" 'migration-secret' 'migration output leaked a secret'
assert_equal '600' "$(file_mode "${home}/.refine/llm.env")" 'migrated env file mode is not 0600'
cmp -s "$migration_expected" "${home}/.zshrc" || fail 'migration changed unrelated zshrc content'
if grep -q 'export BASE_API_KEY' "${home}/.zshrc"; then
  fail 'migrated BASE_API_KEY definition remained in zshrc'
fi
assert_equal '1' "$(grep -c '# >>> Refine LLM env (managed) >>>' "${home}/.zshrc" || true)" 'source block count is not one'
assert_equal '1' "$(find "${home}/.refine/backups" -type f -name '*.bak' -print | wc -l | tr -d ' ')" 'zshrc backup count is not one'
migrated_copy="${home}/zshrc.after-first"
cp "${home}/.zshrc" "$migrated_copy"
repeat_output=''
repeat_output="$(run_config "$home" --migrate 2>&1)" || fail 'repeat migration was not safe'
assert_not_contains "$repeat_output" 'migration-secret' 'repeat migration leaked a secret'
cmp -s "$migrated_copy" "${home}/.zshrc" || fail 'repeat migration changed zshrc'
assert_equal '1' "$(grep -c '# >>> Refine LLM env (managed) >>>' "${home}/.zshrc" || true)" 'repeat migration duplicated source block'
assert_equal '1' "$(find "${home}/.refine/backups" -type f -name '*.bak' -print | wc -l | tr -d ' ')" 'repeat migration created an unexpected backup'
migration_load=''
migration_load="$(env HOME="$home" REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" REFINE_LOADER="$LOADER" bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env; [[ \"\$BASE_API_KEY\" == 'migration-secret' ]]; printf 'migration-load-ok\\n'" 2>&1)" || fail 'migrated env file did not load'
assert_contains "$migration_load" 'migration-load-ok' 'migrated env load result missing'
assert_not_contains "$migration_load" 'migration-secret' 'migrated secret appeared in output'
printf 'PASS migration idempotency, backup, and source block\n'

# A valid secure file with no remaining literals still gets one managed source
# block and a private backup when the block is missing; repeating it is a no-op.
home="$(new_home source-block-repair)"
write_text "${home}/.refine/llm.env" "export BASE_API_KEY='source-block-secret'
export BASE_URL='https://source-block.example.invalid'"
chmod 600 "${home}/.refine/llm.env"
write_text "${home}/.zshrc" '# unrelated interactive setup
export PATH=/usr/bin:/bin
'
secure_copy="${home}/secure-before"
cp "${home}/.refine/llm.env" "$secure_copy"
source_block_output=''
source_block_output="$(run_config "$home" --migrate 2>&1)" || fail 'source block repair migration failed'
assert_contains "$source_block_output" 'added one managed source block' 'source block was not added'
assert_contains "$source_block_output" 'backup=' 'source block repair did not report a backup'
assert_not_contains "$source_block_output" 'source-block-secret' 'source block repair leaked a secret'
cmp -s "$secure_copy" "${home}/.refine/llm.env" || fail 'source block repair changed the secure file'
assert_equal '1' "$(grep -c '# >>> Refine LLM env (managed) >>>' "${home}/.zshrc" || true)" 'source block repair count is not one'
assert_equal '1' "$(find "${home}/.refine/backups" -type f -name '*.bak' -print | wc -l | tr -d ' ')" 'source block repair backup count is not one'
repaired_copy="${home}/zshrc.after-repair"
cp "${home}/.zshrc" "$repaired_copy"
repeat_output=''
repeat_output="$(run_config "$home" --migrate 2>&1)" || fail 'source block repeat was not safe'
assert_not_contains "$repeat_output" 'source-block-secret' 'source block repeat leaked a secret'
cmp -s "$repaired_copy" "${home}/.zshrc" || fail 'source block repeat changed zshrc'
assert_equal '1' "$(grep -c '# >>> Refine LLM env (managed) >>>' "${home}/.zshrc" || true)" 'source block repeat duplicated the block'
assert_equal '1' "$(find "${home}/.refine/backups" -type f -name '*.bak' -print | wc -l | tr -d ' ')" 'source block repeat created an unexpected backup'
printf 'PASS secure-file source block repair and no-op\n'

# Every configuration mode rejects an existing secure file that is syntactically
# valid but cannot construct a provider.
home="$(new_home incomplete-existing-secure)"
write_text "${home}/.refine/llm.env" "export BASE_API_KEY='incomplete-existing-secret'"
chmod 600 "${home}/.refine/llm.env"
write_text "${home}/.zshrc" "export BASE_URL='https://pending.example.invalid'
export BASE_API_KEY='pending-migration-secret'
export BASE_MODEL='pending-model'"
write_text "${home}/source.env" "export REFINE_OPENAI_API_KEY='replacement-secret'"
cp "${home}/.refine/llm.env" "${home}/secure.before"
for mode in --check --migrate --from-env; do
  incomplete_existing_output=''
  if incomplete_existing_output="$(run_config "$home" "$mode" 2>&1)"; then
    fail "configuration mode accepted an incomplete existing secure file: ${mode}"
  fi
  assert_not_contains "$incomplete_existing_output" 'incomplete-existing-secret' \
    "configuration mode leaked an incomplete existing key: ${mode}"
  cmp -s "${home}/secure.before" "${home}/.refine/llm.env" \
    || fail "configuration mode changed an incomplete existing secure file: ${mode}"
done
incomplete_existing_output=''
if incomplete_existing_output="$(run_config "$home" --from-file "${home}/source.env" 2>&1)"; then
  fail 'from-file accepted an incomplete existing secure file'
fi
assert_contains "$incomplete_existing_output" 'no usable provider' \
  'from-file incomplete existing secure-file error was not actionable'
assert_not_contains "$incomplete_existing_output" 'replacement-secret' \
  'from-file incomplete existing secure-file error leaked the replacement key'
cmp -s "${home}/secure.before" "${home}/.refine/llm.env" \
  || fail 'from-file changed an incomplete existing secure file'
printf 'PASS incomplete existing secure-file rejection\n'

# Malformed and duplicate definitions fail closed before either destination is
# replaced, and diagnostics remain redacted.
home="$(new_home malformed-migration)"
write_text "${home}/.zshrc" "export BASE_URL=https://example.invalid/compat
export BASE_API_KEY=\$(printf malformed-secret)
export BASE_MODEL=compat-model"
malformed_output=''
if malformed_output="$(run_config "$home" --migrate 2>&1)"; then
  fail 'malformed migration unexpectedly succeeded'
fi
assert_contains "$malformed_output" 'malformed' 'malformed migration error was not actionable'
assert_not_contains "$malformed_output" 'malformed-secret' 'malformed migration leaked a secret'
[[ ! -e "${home}/.refine/llm.env" ]] || fail 'malformed migration wrote secure env file'
grep -q 'malformed-secret' "${home}/.zshrc" || fail 'malformed zshrc definition was unexpectedly removed'

home="$(new_home duplicate-migration)"
write_text "${home}/.zshrc" 'export BASE_URL=https://example.invalid/compat
export BASE_API_KEY=duplicate-secret
export BASE_MODEL=compat-model
export BASE_MODEL=second-model'
duplicate_output=''
if duplicate_output="$(run_config "$home" --migrate 2>&1)"; then
  fail 'duplicate migration unexpectedly succeeded'
fi
assert_contains "$duplicate_output" 'duplicate' 'duplicate migration error was not actionable'
assert_not_contains "$duplicate_output" 'duplicate-secret' 'duplicate migration leaked a secret'
[[ ! -e "${home}/.refine/llm.env" ]] || fail 'duplicate migration wrote secure env file'
grep -q 'duplicate-secret' "${home}/.zshrc" || fail 'duplicate zshrc definition was unexpectedly removed'
printf 'PASS malformed and duplicate fail-closed behavior\n'

# Already-exported variables can be configured without touching ~/.zshrc.
home="$(new_home from-env)"
from_env_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  CONFIGURE_SCRIPT="$CONFIGURE" \
  bash -c "${unset_supported_command}; export REFINE_OPENAI_API_KEY='from-env-secret' REFINE_OPENAI_MODEL='from-env-model'; bash \"\$CONFIGURE_SCRIPT\" --from-env" 2>&1)" || fail 'from-env configuration failed'
assert_contains "$from_env_output" 'FROM-ENV' 'from-env result missing'
assert_not_contains "$from_env_output" 'from-env-secret' 'from-env leaked a secret'
from_env_load=''
from_env_load="$(env HOME="$home" REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" REFINE_LOADER="$LOADER" bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env; [[ -n \"\${REFINE_OPENAI_API_KEY:-}\" ]]; printf 'from-env-load-ok\\n'" 2>&1)" || fail 'from-env file did not load'
assert_contains "$from_env_load" 'from-env-load-ok' 'from-env load result missing'
assert_not_contains "$from_env_load" 'from-env-secret' 'from-env loaded secret appeared in output'
[[ ! -e "${home}/.zshrc" ]] || fail 'from-env unexpectedly created or modified zshrc'
printf 'PASS configuring already-exported variables\n'

home="$(new_home incomplete-from-env)"
incomplete_from_env_output=''
if incomplete_from_env_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  CONFIGURE_SCRIPT="$CONFIGURE" \
  bash -c "${unset_supported_command}; export BASE_API_KEY='incomplete-secret'; bash \"\$CONFIGURE_SCRIPT\" --from-env" 2>&1)"; then
  fail 'from-env accepted an incomplete BASE provider'
fi
assert_contains "$incomplete_from_env_output" 'BASE requires both BASE_API_KEY and BASE_URL' \
  'from-env incomplete BASE error was not actionable'
assert_not_contains "$incomplete_from_env_output" 'incomplete-secret' \
  'from-env incomplete BASE error leaked its key'
[[ ! -e "${home}/.refine/llm.env" ]] || fail 'from-env wrote an incomplete BASE provider'
printf 'PASS from-env incomplete BASE rejection\n'

# A legacy repository file can be imported without evaluating unrelated lines
# or allowing the caller's process credentials to override the explicit source.
home="$(new_home from-file)"
legacy_file="${home}/legacy.env"
write_text "$legacy_file" "UNRELATED_VALUE=ignored
export BASE_URL='https://legacy.example.invalid'
export BASE_API_KEY='from-file-secret'
export BASE_MODEL='legacy-model'"
from_file_output="$(env HOME="$home" \
  REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" \
  BASE_API_KEY='process-must-not-win' \
  bash "$CONFIGURE" --from-file "$legacy_file" 2>&1)" || fail 'from-file configuration failed'
assert_contains "$from_file_output" 'FROM-FILE' 'from-file result missing'
assert_not_contains "$from_file_output" 'from-file-secret' 'from-file leaked a source secret'
assert_not_contains "$from_file_output" 'process-must-not-win' 'from-file leaked a process secret'
from_file_load="$(env HOME="$home" REFINE_LLM_ENV_FILE="${home}/.refine/llm.env" REFINE_LOADER="$LOADER" \
  bash -c "${unset_supported_command}; source \"\$REFINE_LOADER\"; load_refine_llm_env; [[ \"\$BASE_API_KEY\" == 'from-file-secret' && \"\$BASE_MODEL\" == 'legacy-model' ]]; printf 'from-file-load-ok\\n'" 2>&1)" \
  || fail 'from-file output did not load'
assert_contains "$from_file_load" 'from-file-load-ok' 'from-file load result missing'
assert_equal '600' "$(file_mode "${home}/.refine/llm.env")" 'from-file output mode is not 0600'

home="$(new_home malformed-from-file)"
malformed_from_file="${home}/legacy.env"
write_text "$malformed_from_file" 'export BASE_API_KEY=$(printf from-file-malicious-secret)'
malformed_from_file_output=''
if malformed_from_file_output="$(run_config "$home" --from-file "$malformed_from_file" 2>&1)"; then
  fail 'malformed from-file input unexpectedly succeeded'
fi
assert_contains "$malformed_from_file_output" 'invalid' 'malformed from-file error was not actionable'
assert_not_contains "$malformed_from_file_output" 'from-file-malicious-secret' 'malformed from-file leaked a secret'
[[ ! -e "${home}/.refine/llm.env" ]] || fail 'malformed from-file input wrote secure env file'
printf 'PASS explicit file import and fail-closed parsing\n'

printf 'All LLM env tests passed\n'
