#!/usr/bin/env bash
# scripts/ingest-lib.sh 的可执行自证测试：零外部依赖（sqlite3 缺失时自动跳过 WAL 用例）。
# 运行: bash scripts/tests/test-ingest-lib.sh
set -uo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ingest-lib.sh
source "${SCRIPT_DIR}/../ingest-lib.sh"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/refine-ingest-test.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

passed=0
failed=0

# assert_gate <用例名> <期望退出码> <processed> <failed> <原始退出码> [env 赋值...]
assert_gate() {
  local name="$1" want="$2" processed="$3" nfail="$4" raw="$5"
  shift 5
  local out="${WORK}/out.txt"
  {
    echo "扫描到 N 个会话"
    if [[ "$processed" != "SKIP" ]]; then
      echo "完成: 处理 ${processed}, 跳过重复 0, 过滤 0, 失败 ${nfail}, 生成 0 条观测"
    fi
  } >"$out"

  local msg rc
  msg=$(env "$@" bash -c '
    source "$1"; evaluate_ingest_result "$2" "$3"
  ' _ "${SCRIPT_DIR}/../ingest-lib.sh" "$out" "$raw" 2>&1)
  rc=$?

  if [[ "$rc" -eq "$want" ]]; then
    echo "PASS  ${name}  (rc=${rc})  ${msg}"
    passed=$((passed + 1))
  else
    echo "FAIL  ${name}  期望 rc=${want} 实际 rc=${rc}  ${msg}"
    failed=$((failed + 1))
  fi
}

echo "--- 判定逻辑 ---"
assert_gate "真实线上样本 处理83 失败2 (2.35%) 视为成功" 0 83 2 1
assert_gate "零失败" 0 100 0 0
assert_gate "高失败率 50/100=50% 必须失败" 1 50 50 1
assert_gate "低失败率但绝对条数超限 25/1000" 1 975 25 1
assert_gate "边界 5/100=5.00% 恰好等于阈值 视为成功" 0 95 5 1
assert_gate "边界 6/100=6.00% 超过阈值 失败" 1 94 6 1
assert_gate "无汇总行(进程崩溃) 必须失败" 1 SKIP 0 101
assert_gate "无待处理会话" 0 0 0 0
assert_gate "env 放宽后 50% 视为成功" 0 50 50 1 \
  REFINE_INGEST_MAX_FAIL_RATE=60 REFINE_INGEST_MAX_FAIL_COUNT=100
assert_gate "env 收紧后 2 条失败也算失败" 1 83 2 1 \
  REFINE_INGEST_MAX_FAIL_RATE=0 REFINE_INGEST_TOLERATED_FAILS=0

echo "--- 日志轮转（用常驻 fd 模拟 launchd 的 O_APPEND 句柄）---"
LOG="${WORK}/x.log"
head -c 5000 /dev/zero | tr '\0' 'a' >"$LOG"
exec 3>>"$LOG"
ino_before=$(ls -i "$LOG" | awk '{print $1}')
rotate_log_if_needed "$LOG" 1024
echo "本次输出" >&3
ino_after=$(ls -i "$LOG" | awk '{print $1}')
exec 3>&-

check() {
  if [[ "$2" == "$3" ]]; then
    echo "PASS  $1 ($2)"
    passed=$((passed + 1))
  else
    echo "FAIL  $1 期望 $3 实际 $2"
    failed=$((failed + 1))
  fi
}
check "轮转后 inode 不变" "$ino_before" "$ino_after"
check "历史完整保留在 .1" "$(refine_file_size "${LOG}.1")" "5000"
check "本次输出落在当前日志" "$(grep -c '本次输出' "$LOG")" "1"

SMALL="${WORK}/small.log"
echo "tiny" >"$SMALL"
rotate_log_if_needed "$SMALL" 1048576
check "小文件不轮转" "$([[ -f "${SMALL}.1" ]] && echo yes || echo no)" "no"

echo "--- WAL checkpoint ---"
msg=$(checkpoint_wal "${WORK}/missing.db" 2>&1)
check "数据库不存在时只 warn 并跳过" "$(echo "$msg" | grep -c '跳过 WAL checkpoint')" "1"
if command -v sqlite3 >/dev/null 2>&1; then
  DB="${WORK}/t.db"
  sqlite3 "$DB" "PRAGMA journal_mode=WAL; CREATE TABLE t(a); INSERT INTO t VALUES('x');" >/dev/null
  msg=$(checkpoint_wal "$DB" 2>&1)
  check "空闲库 checkpoint 成功" "$(echo "$msg" | grep -c 'WAL checkpoint 完成')" "1"
else
  echo "SKIP  sqlite3 不可用，跳过真实 checkpoint 用例"
fi

echo "passed=${passed} failed=${failed}"
[[ "$failed" -eq 0 ]]
