#!/usr/bin/env bash
# Daily refresh: ingest new sessions → update mirror score + advice
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${HOME}/.cargo/bin:$PATH"
cd "${SCRIPT_DIR}/.."

# Load .env for LLM API keys
if [ -f .env ]; then
  set -a
  # shellcheck source=/dev/null
  source .env
  set +a
fi

# launchd does not inherit interactive shell variables; fall back to BASE_* from zsh.
# shellcheck source=scripts/load-llm-env.sh
source "${SCRIPT_DIR}/load-llm-env.sh"
load_refine_llm_env

# ingest 判定 / 日志轮转 / WAL 收尾的共享函数
# shellcheck source=scripts/ingest-lib.sh
source "${SCRIPT_DIR}/ingest-lib.sh"

# launchd 的 StandardOutPath 只会 append，没有任何上限，先做一次轮转再输出。
rotate_log_if_needed "${REFINE_DAILY_LOG:-$HOME/Library/Logs/refine-daily-ingest.log}"

QUOTA_FILE="$HOME/.refine/quota_exhausted_until"
if [ -f "$QUOTA_FILE" ]; then
  UNTIL=$(cat "$QUOTA_FILE")
  NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  if [[ "$UNTIL" > "$NOW" ]]; then
    echo "LLM quota exhausted until $UNTIL — skipping refresh"
    exit 0
  fi
fi

echo "=== $(date) ==="

# 失败步骤记账：与 weekly-insights.sh 同范式。任何导致用户可见产出缺失的步骤
# 都必须进这个数组，末尾统一转成非零退出码（U-29：不能只打 echo 就当没事）。
FAILED_STEPS=()

# Preflight: environment diagnostics for troubleshooting
echo "Preflight: PATH=$PATH"
echo "Preflight: refine=$(command -v refine) mirror=$(command -v mirror)"
echo "Preflight: cwd=$(pwd)"
echo "Preflight: env REFINE_DB_PATH=${REFINE_DB_PATH:-<unset>} REFINE_ANTHROPIC_MODEL=${REFINE_ANTHROPIC_MODEL:-<unset>} REFINE_OPENAI_MODEL=${REFINE_OPENAI_MODEL:-<unset>} BASE_MODEL=${BASE_MODEL:-<unset>}"
echo "Preflight: keys REFINE_ANTHROPIC_API_KEY=$([ -n "${REFINE_ANTHROPIC_API_KEY:-}" ] && echo '<set>' || echo '<unset>') REFINE_OPENAI_API_KEY=$([ -n "${REFINE_OPENAI_API_KEY:-}" ] && echo '<set>' || echo '<unset>') BASE_API_KEY=$([ -n "${BASE_API_KEY:-}" ] && echo '<set>' || echo '<unset>')"

# 1. Ingest new sessions
# CLI 只要有 1 个会话失败就返回 Err，所以退出码不能直接当健康信号；
# 这里捕获输出后按失败率阈值判定（阈值见 scripts/ingest-lib.sh）。
echo "Step 1: ingest-sessions"
ingest_out=$(mktemp "${TMPDIR:-/tmp}/refine-ingest.XXXXXX")
trap 'rm -f "$ingest_out"' EXIT
ingest_raw_rc=0
refine ingest-sessions >"$ingest_out" 2>&1 || ingest_raw_rc=$?
cat "$ingest_out"
if evaluate_ingest_result "$ingest_out" "$ingest_raw_rc"; then
  ingest_ok=1
else
  ingest_ok=0
fi

# 2. Refresh mirror score + LLM advice (run regardless of ingest result)
echo "Step 2: mirror score"
mirror score 2>&1

# 3. Weekly report on Sundays — generates ~/.mirror/last-weekly.md for Monday MOTD reminder.
# weekly 失败 = 周报缺失 + 周一 MOTD 提醒消失，属用户可见产出缺失，必须记账。
# 这里不直接中断，是为了让 Step 4 的 WAL 收尾仍然执行；退出码在末尾统一处理。
DOW=$(date +%u)  # 1=Monday … 7=Sunday
if [ "$DOW" = "7" ]; then
  echo "Step 3: mirror weekly (Sunday)"
  weekly_rc=0
  mirror weekly 2>&1 || weekly_rc=$?
  if [ "$weekly_rc" -ne 0 ]; then
    echo "ERROR: Step 3 mirror weekly failed with exit code ${weekly_rc}" >&2
    FAILED_STEPS+=("mirror weekly")
  fi
fi

# 4. WAL 收尾：长连接 + 从不 checkpoint 会让 refine.db-wal 单调增长。
echo "Step 4: wal checkpoint"
checkpoint_wal

echo "Done."

# last-refresh-ok 的语义是"摄入健康"，只由 ingest 决定：
# 周报依赖 LLM/网络，它的故障不应该污染数据摄入的健康信号。
if [ "$ingest_ok" -eq 1 ]; then
  mkdir -p ~/.refine
  date -u +%Y-%m-%dT%H:%M:%SZ > ~/.refine/last-refresh-ok
else
  FAILED_STEPS+=("ingest-sessions")
fi

# 退出码反映整体健康，供 launchd/cron 监控
if [ ${#FAILED_STEPS[@]} -gt 0 ]; then
  echo "ERROR: run finished with failures: ${FAILED_STEPS[*]}" >&2
  exit 1
fi
