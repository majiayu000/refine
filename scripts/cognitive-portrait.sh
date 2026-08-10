#!/usr/bin/env bash
# 定期触发 cognitive-portrait skill 生成认知画像。
# launchd 只支持"按周"，双周节流在本脚本内按最新产物日期判定。
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_DIR=$(cd "${SCRIPT_DIR}/.." && pwd)
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
ENV_FILE="${PROJECT_DIR}/.env"
LOG_PREFIX="[refine-portrait]"

log() {
  echo "${LOG_PREFIX} $(date '+%Y-%m-%d %H:%M:%S') $*"
}

notify() {
  osascript -e "display notification \"$1\" with title \"$2\"" 2>&1 || true
}

if [[ -f "$ENV_FILE" ]]; then
  log "Loading env from ${ENV_FILE}"
  set -a
  # shellcheck source=/dev/null
  source "$ENV_FILE"
  set +a
else
  log "WARNING: env file not found: ${ENV_FILE}"
fi

# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
load_refine_llm_env

log "=== Cognitive Portrait Run Start ==="
log "Preflight: PATH=$PATH"
log "Preflight: PROJECT_DIR=${PROJECT_DIR} PORTRAIT_DIR=${PORTRAIT_DIR} AGENT_BIN=${AGENT_BIN}"

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

start_marker=$(mktemp "${TMPDIR:-/tmp}/refine-portrait-start.XXXXXX")

log "执行 agent: ${AGENT_BIN} exec --sandbox ${AGENT_SANDBOX}"
rc=0
(cd "$PROJECT_DIR" && "$AGENT_BIN" exec --sandbox "$AGENT_SANDBOX" \
  "运行 cognitive-portrait 技能，生成本期认知画像") 2>&1 || rc=$?

# 产物校验：必须出现一份比本次启动更新的画像文件，否则视为静默阉割（U-29）。
new_portrait=$(find "$PORTRAIT_DIR" -maxdepth 1 -name 'cognitive-portrait-*.md' -newer "$start_marker" 2>/dev/null | head -1 || true)
rm -f "$start_marker"

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

log "画像已生成: ${new_portrait}"
notify "认知画像已生成: $(basename "$new_portrait")" "Refine Cognitive Portrait"
log "=== Cognitive Portrait Run End ==="
