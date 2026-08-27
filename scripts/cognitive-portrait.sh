#!/usr/bin/env bash
# 定期触发 cognitive-portrait skill 生成认知画像。
# launchd 只支持"按周"，双周节流在本脚本内按最新产物日期判定。
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_DIR="${REFINE_ROOT:-$(cd "${SCRIPT_DIR}/.." && pwd)}"
PORTRAIT_DIR="${REFINE_PORTRAIT_DIR:-${PROJECT_DIR}/docs/cognitive-portraits}"
# 历史产物（05-31 / 06-01 / 06-02）均由 Codex-native skill 生成，故默认 codex。
AGENT_BIN="${REFINE_PORTRAIT_AGENT:-codex}"
# 节流阈值（天）。低于此间隔直接跳过，避免每周重复 4 路 agent 的 LLM 成本。
MIN_INTERVAL_DAYS="${REFINE_PORTRAIT_MIN_INTERVAL_DAYS:-13}"
# 无人值守运行必须显式指定沙箱策略：workspace-write 允许 agent 把画像写进
# PORTRAIT_DIR，但不给工作区以外的写权限。不要默认用
# --dangerously-bypass-approvals-and-sandbox：那是交互式 shell 里的个人选择，
# 定时任务无人监督，权限必须取最小可用集。
AGENT_SANDBOX="${REFINE_PORTRAIT_SANDBOX:-workspace-write}"
MIRROR_DIR="${REFINE_PORTRAIT_MIRROR_DIR:-${HOME}/.mirror}"
COLLECTOR_SCRIPT="${REFINE_PORTRAIT_COLLECTOR:-${SCRIPT_DIR}/collect-cognitive-portrait.sh}"
VALIDATOR_SCRIPT="${REFINE_PORTRAIT_VALIDATOR:-${SCRIPT_DIR}/validate-cognitive-portrait.sh}"
LOG_PREFIX="[refine-portrait]"
INDEX_FILE="${PORTRAIT_DIR}/INDEX.md"

log() {
  echo "${LOG_PREFIX} $(date '+%Y-%m-%d %H:%M:%S') $*"
}

notify() {
  osascript -e "display notification \"$1\" with title \"$2\"" 2>&1 || true
}

# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
if ! load_refine_llm_env 2>/dev/null; then
  log "WARNING: no provider API key found; continuing with the agent's own authentication"
fi

log "=== Cognitive Portrait Run Start ==="
log "Preflight: PATH=$PATH"
log "Preflight: PROJECT_DIR=${PROJECT_DIR} PORTRAIT_DIR=${PORTRAIT_DIR} AGENT_BIN=${AGENT_BIN}"
log "Preflight: LLM source=${REFINE_LLM_ENV_SOURCE:-none} $(refine_llm_env_status)"

if [[ ! -d "$PORTRAIT_DIR" ]]; then
  log "ERROR: 画像归档目录不存在: ${PORTRAIT_DIR}"
  notify "画像归档目录不存在: ${PORTRAIT_DIR}" "Refine Cognitive Portrait 失败"
  exit 1
fi

# 双周节流：取最新 cognitive-portrait-YYYY-MM-DD-vN.md 的文件名日期。
# 依赖 skill 的命名约定（见 docs/cognitive-portraits/INDEX.md「命名约定」）；
# 若命名变化，节流失效并退化为每周执行（多跑不丢数据，可接受）。
latest=$(ls "${PORTRAIT_DIR}"/cognitive-portrait-*.md 2>/dev/null | sort | tail -1 || true)
if [[ -n "$latest" ]]; then
  base=$(basename "$latest")
  date_part=$(echo "$base" | sed -n 's/^cognitive-portrait-\([0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}\)-.*$/\1/p')
  if [[ -n "$date_part" ]]; then
    last_epoch=$(date -j -f '%Y-%m-%d' "$date_part" '+%s' 2>/dev/null || echo "")
    if [[ -n "$last_epoch" ]]; then
      age_days=$(( ($(date '+%s') - last_epoch) / 86400 ))
      if (( age_days < MIN_INTERVAL_DAYS )); then
        log "SKIP: 上一份画像 ${age_days} 天前生成（${base}），未达 ${MIN_INTERVAL_DAYS} 天节流间隔"
        log "=== Cognitive Portrait Run End ==="
        exit 0
      fi
      log "上一份画像 ${age_days} 天前生成（${base}），继续执行"
    else
      log "WARNING: 无法解析日期 ${date_part}，跳过节流判定"
    fi
  else
    log "WARNING: 文件名不符合命名约定: ${base}，跳过节流判定"
  fi
else
  log "归档目录暂无历史画像，直接执行"
fi

if ! command -v "$AGENT_BIN" >/dev/null 2>&1; then
  log "ERROR: agent 可执行文件未找到: ${AGENT_BIN}（launchd PATH 通常不含用户级 bin，请用 REFINE_PORTRAIT_AGENT 指定绝对路径）"
  notify "agent 未找到: ${AGENT_BIN}" "Refine Cognitive Portrait 失败"
  exit 1
fi
if [[ ! -x "$COLLECTOR_SCRIPT" || ! -x "$VALIDATOR_SCRIPT" ]]; then
  log "ERROR: collector/validator 不可执行: ${COLLECTOR_SCRIPT} / ${VALIDATOR_SCRIPT}"
  exit 1
fi
if [[ -L "$MIRROR_DIR" ]]; then
  log "ERROR: refusing symlink Mirror state directory: ${MIRROR_DIR}"
  exit 1
fi
mkdir -p "$MIRROR_DIR"
chmod 700 "$MIRROR_DIR" 2>/dev/null || true

start_marker=$(mktemp "${TMPDIR:-/tmp}/refine-portrait-start.XXXXXX")
index_snapshot=$(mktemp "${TMPDIR:-/tmp}/refine-portrait-index.XXXXXX")
bundle_file=$(mktemp "${TMPDIR:-/tmp}/refine-portrait-bundle.XXXXXX")
quality_file=$(mktemp "${TMPDIR:-/tmp}/refine-portrait-quality.XXXXXX")
index_existed=0
run_committed=0
agent_pid=""
if [[ -f "$INDEX_FILE" ]]; then
  cp "$INDEX_FILE" "$index_snapshot"
  index_existed=1
fi

quarantine_portrait() {
  local portrait="$1"
  local failed_dir="${PORTRAIT_DIR}/.failed"
  local destination
  mkdir -p "$failed_dir"
  chmod 700 "$failed_dir" 2>/dev/null || true
  destination="${failed_dir}/$(basename "$portrait").$(date -u +%Y%m%dT%H%M%SZ).failed"
  mv "$portrait" "$destination"
  log "隔离未完成画像: ${destination}"
}

restore_portrait_index() {
  if [[ "$index_existed" == "1" ]]; then
    cp "$index_snapshot" "$INDEX_FILE"
  else
    rm -f "$INDEX_FILE"
  fi
}

cleanup_incomplete_run() {
  local exit_status=$?
  local candidate
  trap - EXIT HUP INT TERM
  if [[ "$run_committed" != "1" ]]; then
    while IFS= read -r candidate; do
      [[ -n "$candidate" ]] && quarantine_portrait "$candidate"
    done < <(find "$PORTRAIT_DIR" -maxdepth 1 -name 'cognitive-portrait-*.md' -newer "$start_marker" 2>/dev/null || true)
    restore_portrait_index
  fi
  rm -f "$start_marker" "$index_snapshot" "$bundle_file" "$quality_file"
  exit "$exit_status"
}

forward_agent_signal() {
  local signal="$1"
  local exit_status="$2"
  if [[ -n "$agent_pid" ]]; then
    kill -"$signal" -- "-$agent_pid" 2>/dev/null || kill -"$signal" "$agent_pid" 2>/dev/null || true
  fi
  exit "$exit_status"
}

trap cleanup_incomplete_run EXIT
trap 'forward_agent_signal HUP 129' HUP
trap 'forward_agent_signal INT 130' INT
trap 'forward_agent_signal TERM 143' TERM

cutoff=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
log "采集 deterministic evidence bundle: cutoff=${cutoff} period=90d"
if ! "$COLLECTOR_SCRIPT" --period 90 --cutoff "$cutoff" --output "$bundle_file"; then
  log "ERROR: cognitive portrait collector 失败；不启动 agent"
  notify "画像证据采集失败，详见 refine-portrait.log" "Refine Cognitive Portrait 失败"
  exit 1
fi
export REFINE_COGNITIVE_PORTRAIT_BUNDLE="$bundle_file"
export REFINE_COGNITIVE_PORTRAIT_PREVIOUS="${latest:-}"

log "执行 agent: ${AGENT_BIN} exec --sandbox ${AGENT_SANDBOX} --add-dir ${MIRROR_DIR}"
rc=0
(cd "$PROJECT_DIR" && exec /usr/bin/perl -MPOSIX=setsid -e 'setsid() or die "setsid failed: $!"; exec @ARGV or die "exec failed: $!"' -- \
  "$AGENT_BIN" exec --sandbox "$AGENT_SANDBOX" --add-dir "$MIRROR_DIR" \
  "运行 cognitive-portrait 技能，生成本期认知画像") 2>&1 &
agent_pid=$!
wait "$agent_pid" || rc=$?
agent_pid=""

# 产物校验：必须出现一份比本次启动更新的画像文件，否则视为静默阉割（U-29）。
new_portrait=$(find "$PORTRAIT_DIR" -maxdepth 1 -name 'cognitive-portrait-*.md' -newer "$start_marker" 2>/dev/null | head -1 || true)

if [[ -z "$new_portrait" ]]; then
  log "ERROR: agent 退出但未产出新画像（agent exit code ${rc}）"
  notify "agent 退出但未产出新画像，详见 refine-portrait.log" "Refine Cognitive Portrait 失败"
  exit 1
fi

if [[ "$rc" -ne 0 ]]; then
  log "ERROR: agent 以非零状态退出: ${rc}（已产出 ${new_portrait}，但结果可能不完整）"
  notify "agent 非零退出 (${rc})，详见 refine-portrait.log" "Refine Cognitive Portrait 失败"
  exit 1
fi

validator_args=(--bundle "$bundle_file" --portrait "$new_portrait" --output "$quality_file")
[[ -n "${latest:-}" ]] && validator_args+=(--previous "$latest")
if ! "$VALIDATOR_SCRIPT" "${validator_args[@]}"; then
  log "ERROR: 新画像未通过 evidence quality gate: ${new_portrait}"
  notify "新画像质量门禁失败，已隔离" "Refine Cognitive Portrait 失败"
  exit 1
fi

new_base=$(basename "$new_portrait")
new_date=$(sed -n 's/^cognitive-portrait-\([0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}\)-.*$/\1/p' <<<"$new_base")
if [[ ! -f "$INDEX_FILE" ]] || ! grep -Eq "^\|[[:space:]]*(\\[)?${new_date}([[:space:]]|\\]|\\()" "$INDEX_FILE"; then
  log "ERROR: 新画像未写入归档索引: ${new_base}"
  notify "新画像未写入 INDEX.md，已隔离" "Refine Cognitive Portrait 失败"
  exit 1
fi

run_committed=1
evidence_dir="${PORTRAIT_DIR}/evidence"
mkdir -p "$evidence_dir"
chmod 700 "$evidence_dir" 2>/dev/null || true
mv "$bundle_file" "${evidence_dir}/${new_base%.md}.bundle.json"
mv "$quality_file" "${evidence_dir}/${new_base%.md}.quality.json"
rm -f "$start_marker" "$index_snapshot"
trap - EXIT HUP INT TERM
log "画像已生成: ${new_portrait}"
notify "认知画像已生成: $(basename "$new_portrait")" "Refine Cognitive Portrait"
log "=== Cognitive Portrait Run End ==="
