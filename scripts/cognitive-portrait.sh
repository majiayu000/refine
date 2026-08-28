#!/usr/bin/env bash
# Generate a cognitive portrait through an untrusted-agent staging boundary.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/runtime-job-lock.sh
source "${SCRIPT_DIR}/runtime-job-lock.sh"
if [[ "${REFINE_RUNTIME_LOCK_ACTIVE:-}" != "1" ]]; then
  export REFINE_RUNTIME_JOB_LOCK_WAIT_SECONDS="${REFINE_PORTRAIT_LOCK_WAIT_SECONDS:-0}"
  supervisor=""
  forward_lock_signal() {
    local signal="$1" status="$2"
    [[ -z "$supervisor" ]] || kill -"$signal" -- "-$supervisor" 2>/dev/null \
      || kill -"$signal" "$supervisor" 2>/dev/null || true
    wait "$supervisor" 2>/dev/null || true
    exit "$status"
  }
  /usr/bin/perl -MPOSIX=setsid -e 'setsid() or die "setsid failed: $!"; exec @ARGV or die "exec failed: $!"' -- \
    bash -c 'source "$1"; shift; run_refine_runtime_job_locked "$@"' \
    refine-portrait-lock "${SCRIPT_DIR}/runtime-job-lock.sh" "${SCRIPT_DIR}/cognitive-portrait.sh" "$@" &
  supervisor=$!
  trap 'forward_lock_signal HUP 129' HUP
  trap 'forward_lock_signal INT 130' INT
  trap 'forward_lock_signal TERM 143' TERM
  status=0
  wait "$supervisor" || status=$?
  trap - HUP INT TERM
  exit "$status"
fi

PROJECT_DIR="${REFINE_ROOT:-$(cd "${SCRIPT_DIR}/.." && pwd)}"
PORTRAIT_DIR="${REFINE_PORTRAIT_DIR:-${PROJECT_DIR}/docs/cognitive-portraits}"
INDEX_FILE="${PORTRAIT_DIR}/INDEX.md"
PUBLICATION_JOURNAL="${PORTRAIT_DIR}/.portrait-publish.journal"
PUBLICATION_INDEX_BACKUP="${PORTRAIT_DIR}/.portrait-publish.index-backup"
AGENT_BIN="${REFINE_PORTRAIT_AGENT:-codex}"
AGENT_SANDBOX="${REFINE_PORTRAIT_SANDBOX:-workspace-write}"
MIN_INTERVAL_DAYS="${REFINE_PORTRAIT_MIN_INTERVAL_DAYS:-13}"
COLLECTOR_SCRIPT="${REFINE_PORTRAIT_COLLECTOR:-${SCRIPT_DIR}/collect-cognitive-portrait.sh}"
VALIDATOR_SCRIPT="${REFINE_PORTRAIT_VALIDATOR:-${SCRIPT_DIR}/validate-cognitive-portrait.sh}"
SKILL_FILE="${REFINE_PORTRAIT_SKILL:-${PROJECT_DIR}/skills/cognitive-portrait/SKILL.md}"
STATE_ROOT="${REFINE_PORTRAIT_STATE_ROOT:-${HOME}/.refine/cognitive-portrait-runs}"
STAGING_ROOT="${REFINE_PORTRAIT_STAGING_ROOT:-${TMPDIR:-/tmp}}"
LOG_PREFIX="[refine-portrait]"

log() { echo "${LOG_PREFIX} $(date '+%Y-%m-%d %H:%M:%S') $*"; }

notify() {
  command -v osascript >/dev/null 2>&1 || return 0
  osascript - "$1" "$2" <<'APPLESCRIPT' 2>&1 || true
on run argv
  display notification (item 1 of argv) with title (item 2 of argv)
end run
APPLESCRIPT
}

sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

file_identity() {
  if [[ "$(uname -s)" == "Darwin" ]]; then
    stat -f '%d:%i:%l' "$1"
  else
    stat -c '%d:%i:%h' "$1"
  fi
}

file_owner() {
  if [[ "$(uname -s)" == "Darwin" ]]; then
    stat -f '%u' "$1"
  else
    stat -c '%u' "$1"
  fi
}

directory_identity() {
  [[ -d "$1" && ! -L "$1" ]] || return 1
  if [[ "$(uname -s)" == "Darwin" ]]; then
    stat -f '%d:%i' "$1"
  else
    stat -c '%d:%i' "$1"
  fi
}

require_private_regular() {
  [[ -f "$1" && ! -L "$1" ]] || return 1
  [[ "$(file_identity "$1" | awk -F: '{print $3}')" == "1" \
    && "$(file_owner "$1")" == "$(id -u)" ]]
}

atomic_copy() {
  local source="$1" destination="$2" parent temporary
  parent=$(dirname "$destination")
  [[ -d "$parent" && ! -L "$parent" ]] || return 1
  temporary=$(mktemp "${parent}/.portrait-publish.XXXXXX") || return 1
  if ! cp -p -- "$source" "$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  chmod 600 "$temporary" 2>/dev/null || true
  if [[ -e "$destination" || -L "$destination" ]]; then
    rm -f -- "$temporary"
    return 1
  fi
  mv -- "$temporary" "$destination"
}

atomic_replace() {
  local source="$1" destination="$2" parent temporary
  parent=$(dirname "$destination")
  [[ "$parent" == "$PORTRAIT_DIR" && -d "$parent" && ! -L "$parent" ]] || return 1
  temporary=$(mktemp "${parent}/.portrait-replace.XXXXXX") || return 1
  cp -p -- "$source" "$temporary" || { rm -f -- "$temporary"; return 1; }
  chmod 600 "$temporary" 2>/dev/null || true
  rm -f -- "$destination" || { rm -f -- "$temporary"; return 1; }
  mv -f -- "$temporary" "$destination"
}

recover_incomplete_publication() {
  local base report bundle quality
  [[ -e "$PUBLICATION_JOURNAL" || -L "$PUBLICATION_JOURNAL" ]] || return 0
  require_private_regular "$PUBLICATION_JOURNAL" \
    && require_private_regular "$PUBLICATION_INDEX_BACKUP" || return 1
  IFS= read -r base < "$PUBLICATION_JOURNAL"
  [[ "$base" =~ ^cognitive-portrait-[0-9]{4}-[0-9]{2}-[0-9]{2}-v4$ ]] || return 1
  report="${PORTRAIT_DIR}/${base}.md"
  bundle="${PORTRAIT_DIR}/evidence/${base}.bundle.json"
  quality="${PORTRAIT_DIR}/evidence/${base}.quality.json"
  rm -f -- "$report" "$bundle" "$quality"
  atomic_replace "$PUBLICATION_INDEX_BACKUP" "$INDEX_FILE" || return 1
  rm -f -- "$PUBLICATION_JOURNAL" "$PUBLICATION_INDEX_BACKUP"
  sync
  log "recovered incomplete publication: ${base}"
}

tree_fingerprint() {
  local root="$1"
  find "$root" -type f -print | LC_ALL=C sort | while IFS= read -r file; do
    printf '%s  %s\n' "$(sha256_file "$file")" "${file#"${root}"/}"
  done
}

log "=== Cognitive Portrait Run Start ==="
if [[ ! -d "$PORTRAIT_DIR" || -L "$PORTRAIT_DIR" ]]; then
  log "ERROR: portrait archive must be a real directory: ${PORTRAIT_DIR}"
  exit 1
fi
PORTRAIT_DIR_ID=$(directory_identity "$PORTRAIT_DIR")
if [[ ! -f "$INDEX_FILE" || -L "$INDEX_FILE" ]] || ! require_private_regular "$INDEX_FILE"; then
  log "ERROR: INDEX.md must be a regular single-link file"
  exit 1
fi
if [[ ! -x "$COLLECTOR_SCRIPT" || ! -x "$VALIDATOR_SCRIPT" || ! -f "$SKILL_FILE" ]] \
  || ! require_private_regular "$COLLECTOR_SCRIPT" \
  || ! require_private_regular "$VALIDATOR_SCRIPT" \
  || ! require_private_regular "$SKILL_FILE"; then
  log "ERROR: trusted collector, validator, or skill is unavailable"
  exit 1
fi
SKILL_DIR=$(dirname "$SKILL_FILE")
if find "$SKILL_DIR" \( -type l -o -type f -links +1 \) -print -quit | grep -q .; then
  log "ERROR: trusted skill tree contains a symlink or hard-linked file"
  exit 1
fi
if ! command -v "$AGENT_BIN" >/dev/null 2>&1; then
  log "ERROR: agent executable not found: ${AGENT_BIN}"
  notify "agent 未找到" "Refine Cognitive Portrait 失败"
  exit 1
fi
if [[ "$(basename "$AGENT_BIN")" == "codex" ]]; then
  agent_help=$("$AGENT_BIN" exec --help 2>&1) || {
    log "ERROR: cannot inspect Codex automation flags"
    exit 1
  }
  for required_flag in --ephemeral --ignore-user-config --ignore-rules --skip-git-repo-check; do
    grep -q -- "$required_flag" <<<"$agent_help" || {
      log "ERROR: Codex lacks required isolation flag ${required_flag}"
      exit 1
    }
  done
fi
if find "$PORTRAIT_DIR" \( -type l -o -type f -links +1 \) -print -quit | grep -q .; then
  log "ERROR: archive contains a symlink or hard-linked file"
  exit 1
fi
if ! recover_incomplete_publication; then
  log "ERROR: incomplete publication journal is unsafe or unrecoverable"
  exit 1
fi
if [[ -e "${PORTRAIT_DIR}/.failed" && ! -d "${PORTRAIT_DIR}/.failed" ]] \
  || [[ -L "${PORTRAIT_DIR}/.failed" ]]; then
  log "ERROR: .failed must be an ordinary directory when present"
  exit 1
fi

latest=$(find "$PORTRAIT_DIR" -maxdepth 1 -type f -name 'cognitive-portrait-*.md' -print \
  | LC_ALL=C sort | tail -1 || true)
if [[ -n "$latest" ]]; then
  base=$(basename "$latest")
  date_part=$(sed -n 's/^cognitive-portrait-\([0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}\)-.*$/\1/p' <<<"$base")
  if [[ -n "$date_part" ]]; then
    last_epoch=$(date -j -f '%Y-%m-%d' "$date_part" '+%s' 2>/dev/null \
      || date -d "$date_part" '+%s' 2>/dev/null || true)
    if [[ -n "$last_epoch" ]]; then
      age_days=$(( ($(date '+%s') - last_epoch) / 86400 ))
      if (( age_days < MIN_INTERVAL_DAYS )); then
        log "SKIP: previous portrait is ${age_days} days old"
        exit 0
      fi
    fi
  fi
fi

if [[ "$STATE_ROOT" != /* ]]; then
  log "ERROR: run-state root must be absolute"
  exit 1
fi
if [[ ! -d "$STAGING_ROOT" ]]; then
  log "ERROR: agent staging root must be a real directory"
  exit 1
fi
STAGING_ROOT=$(cd "$STAGING_ROOT" && pwd -P)
state_parent=$(dirname "$STATE_ROOT")
if [[ -L "$state_parent" ]]; then
  log "ERROR: refusing symlink run-state parent"
  exit 1
fi
mkdir -p "$state_parent"
state_parent_physical=$(cd "$state_parent" && pwd -P)
if [[ "$state_parent_physical" != "$state_parent" || "$(file_owner "$state_parent")" != "$(id -u)" ]]; then
  log "ERROR: run-state parent must be canonical and owned by the current user"
  exit 1
fi
chmod 700 "$state_parent" 2>/dev/null || true
if [[ -L "$STATE_ROOT" ]]; then
  log "ERROR: refusing symlink run-state root"
  exit 1
fi
mkdir -p "$STATE_ROOT"
chmod 700 "$STATE_ROOT" 2>/dev/null || true
if [[ "$(file_owner "$STATE_ROOT")" != "$(id -u)" ]]; then
  log "ERROR: run-state root is not owned by the current user"
  exit 1
fi
trusted_dir=$(mktemp -d "${STATE_ROOT}/trusted.XXXXXX")
staging_dir=$(mktemp -d "${STAGING_ROOT%/}/refine-portrait-agent.XXXXXX")
chmod 700 "$trusted_dir" "$staging_dir"
bundle_file="${trusted_dir}/bundle.json"
quality_file="${trusted_dir}/quality.json"
candidate_trusted="${trusted_dir}/candidate.md"
agent_bundle="${staging_dir}/bundle.json"
agent_previous="${staging_dir}/previous.md"
agent_candidate="${staging_dir}/candidate.md"
archive_snapshot="${trusted_dir}/archive.before"
mkdir "$archive_snapshot"
cp -R "${PORTRAIT_DIR}/." "$archive_snapshot/"
archive_before=$(tree_fingerprint "$PORTRAIT_DIR")
collector_hash=$(sha256_file "$COLLECTOR_SCRIPT")
validator_hash=$(sha256_file "$VALIDATOR_SCRIPT")
skill_hash=$(tree_fingerprint "$SKILL_DIR")
run_committed=0
agent_pid=""
published_report=""
published_bundle=""
published_quality=""

restore_archive_file() {
  local snapshot="$1" destination="$2" parent temporary
  parent=$(dirname "$destination")
  [[ "$(directory_identity "$PORTRAIT_DIR" 2>/dev/null || true)" == "$PORTRAIT_DIR_ID" ]] || return 1
  [[ "$parent" == "$PORTRAIT_DIR" || "$parent" == "${PORTRAIT_DIR}/evidence" ]] || return 1
  [[ -d "$parent" && ! -L "$parent" ]] || return 1
  rm -f -- "$destination" || return 1
  temporary=$(mktemp "${parent}/.portrait-restore.XXXXXX") || return 1
  cp -p -- "$snapshot" "$temporary" || { rm -f -- "$temporary"; return 1; }
  mv -f -- "$temporary" "$destination"
}

restore_archive() {
  local current relative snapshot destination
  [[ "$(directory_identity "$PORTRAIT_DIR" 2>/dev/null || true)" == "$PORTRAIT_DIR_ID" ]] || return 1
  while IFS= read -r current; do
    relative=${current#"${PORTRAIT_DIR}"/}
    [[ -f "${archive_snapshot}/${relative}" ]] || rm -f -- "$current" || true
  done < <(find "$PORTRAIT_DIR" -type f -print)
  while IFS= read -r current; do
    [[ -L "$current" ]] && rm -f -- "$current" || true
  done < <(find "$PORTRAIT_DIR" -type l -print 2>/dev/null || true)
  while IFS= read -r snapshot; do
    relative=${snapshot#"${archive_snapshot}"/}
    destination="${PORTRAIT_DIR}/${relative}"
    mkdir -p "$(dirname "$destination")"
    restore_archive_file "$snapshot" "$destination" || return 1
  done < <(find "$archive_snapshot" -type f -print)
}

cleanup() {
  local status=$?
  trap - EXIT HUP INT TERM
  if [[ "$run_committed" != "1" ]]; then
    [[ -z "$published_report" ]] || rm -f -- "$published_report" || true
    [[ -z "$published_bundle" ]] || rm -f -- "$published_bundle" || true
    [[ -z "$published_quality" ]] || rm -f -- "$published_quality" || true
    restore_archive || log "ERROR: archive rollback could not be completed safely"
  fi
  chmod 700 "$staging_dir" 2>/dev/null || true
  rm -rf -- "$staging_dir" "$trusted_dir"
  exit "$status"
}

forward_agent_signal() {
  local signal="$1" status="$2"
  [[ -z "$agent_pid" ]] || kill -"$signal" -- "-$agent_pid" 2>/dev/null \
    || kill -"$signal" "$agent_pid" 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT
trap 'forward_agent_signal HUP 129' HUP
trap 'forward_agent_signal INT 130' INT
trap 'forward_agent_signal TERM 143' TERM

cutoff=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
if ! require_private_regular "$COLLECTOR_SCRIPT" \
  || [[ "$(sha256_file "$COLLECTOR_SCRIPT")" != "$collector_hash" ]]; then
  log "ERROR: trusted collector identity changed before execution"
  exit 1
fi
if ! "$COLLECTOR_SCRIPT" --period 90 --cutoff "$cutoff" --output "$bundle_file"; then
  log "ERROR: deterministic collector failed"
  exit 1
fi
require_private_regular "$bundle_file" || { log "ERROR: collector output is unsafe"; exit 1; }
bundle_hash=$(sha256_file "$bundle_file")
cp -p "$bundle_file" "$agent_bundle"
chmod 600 "$agent_bundle" 2>/dev/null || true
if [[ -n "$latest" ]]; then
  cp -p "$latest" "$agent_previous"
  chmod 400 "$agent_previous" 2>/dev/null || true
fi

export REFINE_COGNITIVE_PORTRAIT_BUNDLE="$agent_bundle"
export REFINE_COGNITIVE_PORTRAIT_PREVIOUS="${agent_previous:-}"
export REFINE_COGNITIVE_PORTRAIT_OUTPUT="$agent_candidate"
prompt="Read ${SKILL_FILE} and generate one cognitive portrait from the supplied bundle. Write only ${agent_candidate}; do not edit the repository, archive, evidence, input bundle, validator, or history."
agent_env=(
  "HOME=${HOME}"
  "PATH=${PATH}"
  "REFINE_COGNITIVE_PORTRAIT_BUNDLE=${agent_bundle}"
  "REFINE_COGNITIVE_PORTRAIT_PREVIOUS=${agent_previous:-}"
  "REFINE_COGNITIVE_PORTRAIT_OUTPUT=${agent_candidate}"
)
for allowed_env in TMPDIR CODEX_HOME OPENAI_API_KEY OPENAI_ORG_ID OPENAI_PROJECT SSL_CERT_FILE SSL_CERT_DIR \
  FAKE_AGENT_MODE FAKE_AGENT_LOG FAKE_ASSERT_ISOLATION FAKE_INDEX_TARGET FAKE_HISTORY_TARGET \
  FAKE_PORTRAIT_TARGET FAKE_VALIDATOR_TARGET FAKE_VALIDATOR_MARKER FAKE_VICTIM \
  FAKE_DESCENDANT_MARKER; do
  [[ -z "${!allowed_env:-}" ]] || agent_env+=("${allowed_env}=${!allowed_env}")
done
log "running untrusted agent in isolated staging directory"
rc=0
(cd "$staging_dir" && exec /usr/bin/perl -MPOSIX=setsid -e 'setsid() or die "setsid failed: $!"; exec @ARGV or die "exec failed: $!"' -- \
  env -i "${agent_env[@]}" \
    "$AGENT_BIN" exec --ephemeral --ignore-user-config --ignore-rules \
      --skip-git-repo-check --sandbox "$AGENT_SANDBOX" "$prompt") 2>&1 &
agent_pid=$!
wait "$agent_pid" || rc=$?
agent_pid=""

if [[ "$rc" -ne 0 ]]; then
  log "ERROR: agent exited ${rc}"
  exit 1
fi
if [[ "$(sha256_file "$COLLECTOR_SCRIPT")" != "$collector_hash" \
  || "$(sha256_file "$VALIDATOR_SCRIPT")" != "$validator_hash" \
  || "$(tree_fingerprint "$SKILL_DIR")" != "$skill_hash" ]]; then
  log "ERROR: trusted collector, validator, or skill changed during agent run"
  exit 1
fi
if [[ "$(sha256_file "$bundle_file")" != "$bundle_hash" ]]; then
  log "ERROR: trusted evidence bundle changed during agent run"
  exit 1
fi
archive_after=$(tree_fingerprint "$PORTRAIT_DIR")
if [[ "$archive_after" != "$archive_before" ]]; then
  log "ERROR: agent attempted to mutate portrait history"
  exit 1
fi
if [[ $(find "$staging_dir" -maxdepth 1 -type f ! -name bundle.json ! -name previous.md \
  ! -name candidate.md ! -name 'layer-l[1-4].md' | wc -l | tr -d ' ') != 0 ]]; then
  log "ERROR: agent produced unexpected staging artifacts"
  exit 1
fi
if ! require_private_regular "$agent_candidate"; then
  log "ERROR: candidate must be a regular single-link file"
  exit 1
fi
chmod 500 "$staging_dir" 2>/dev/null || true
candidate_hash=$(sha256_file "$agent_candidate")
cp -p -- "$agent_candidate" "$candidate_trusted"
require_private_regular "$candidate_trusted" || { log "ERROR: trusted candidate copy is unsafe"; exit 1; }
if [[ "$(sha256_file "$candidate_trusted")" != "$candidate_hash" ]]; then
  log "ERROR: candidate changed while crossing the trust boundary"
  exit 1
fi

validator_args=(--bundle "$bundle_file" --portrait "$candidate_trusted" --output "$quality_file")
[[ -n "$latest" ]] && validator_args+=(--previous "$latest")
if ! require_private_regular "$VALIDATOR_SCRIPT" \
  || [[ "$(sha256_file "$VALIDATOR_SCRIPT")" != "$validator_hash" ]]; then
  log "ERROR: trusted validator identity changed before execution"
  exit 1
fi
if ! "$VALIDATOR_SCRIPT" "${validator_args[@]}"; then
  log "ERROR: candidate failed evidence quality gates"
  exit 1
fi
require_private_regular "$quality_file" || { log "ERROR: validator output is unsafe"; exit 1; }

report_date=$(date '+%Y-%m-%d')
new_base="cognitive-portrait-${report_date}-v4.md"
report_destination="${PORTRAIT_DIR}/${new_base}"
evidence_dir="${PORTRAIT_DIR}/evidence"
if [[ -e "$evidence_dir" && ! -d "$evidence_dir" ]] || [[ -L "$evidence_dir" ]]; then
  log "ERROR: evidence path is unsafe"
  exit 1
fi
mkdir -p "$evidence_dir"
bundle_destination="${evidence_dir}/${new_base%.md}.bundle.json"
quality_destination="${evidence_dir}/${new_base%.md}.quality.json"
for destination in "$report_destination" "$bundle_destination" "$quality_destination"; do
  if [[ -e "$destination" || -L "$destination" ]]; then
    log "ERROR: refusing to overwrite ${destination}"
    exit 1
  fi
done

if [[ -e "$PUBLICATION_JOURNAL" || -L "$PUBLICATION_JOURNAL" \
  || -e "$PUBLICATION_INDEX_BACKUP" || -L "$PUBLICATION_INDEX_BACKUP" ]]; then
  log "ERROR: refusing to overwrite publication recovery state"
  exit 1
fi
atomic_copy "$INDEX_FILE" "$PUBLICATION_INDEX_BACKUP"
journal_stage=$(mktemp "${PORTRAIT_DIR}/.portrait-journal.XXXXXX")
printf '%s\n' "${new_base%.md}" > "$journal_stage"
chmod 600 "$journal_stage" 2>/dev/null || true
mv -- "$journal_stage" "$PUBLICATION_JOURNAL"
sync

atomic_copy "$bundle_file" "$bundle_destination"
published_bundle="$bundle_destination"
sync
if [[ "${REFINE_PORTRAIT_FAILPOINT:-}" == "after-bundle" ]]; then
  kill -KILL "$$"
fi
atomic_copy "$quality_file" "$quality_destination"
published_quality="$quality_destination"
sync
atomic_copy "$candidate_trusted" "$report_destination"
published_report="$report_destination"
sync
index_stage="${trusted_dir}/INDEX.next.md"
index_row="| [${report_date}](./${new_base}) | v4 | bundle | evidence | ✅ | ✅ | ✅ | ✅ | deterministic collector + gated agent | ✅ |"
awk -v row="$index_row" '
  BEGIN { in_reports = 0; inserted = 0; previous_was_table = 0 }
  $0 == "## 报告清单" { in_reports = 1 }
  in_reports && !inserted && previous_was_table && $0 !~ /^\|/ {
    print row
    inserted = 1
  }
  { print; previous_was_table = in_reports && $0 ~ /^\|/ }
  END { if (!inserted) print row }
' "$INDEX_FILE" > "$index_stage"
restore_archive_file "$index_stage" "$INDEX_FILE"
sync
rm -f -- "$PUBLICATION_JOURNAL" "$PUBLICATION_INDEX_BACKUP"
sync
run_committed=1
trap - EXIT HUP INT TERM
chmod 700 "$staging_dir" 2>/dev/null || true
rm -rf -- "$staging_dir" "$trusted_dir"
log "portrait published: ${report_destination}"
notify "认知画像已生成: ${new_base}" "Refine Cognitive Portrait"
log "=== Cognitive Portrait Run End ==="
