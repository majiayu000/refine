#!/usr/bin/env bash
# ingest 结果判定 / 日志轮转 / WAL 收尾的共享纯函数库。
# 用法：source 本文件后直接调用函数，本文件自身不产生任何副作用。
#
# 阈值全部走环境变量，调用方不得 hardcode：
#   REFINE_INGEST_MAX_FAIL_RATE   失败率上限（百分比整数，默认 5）
#   REFINE_INGEST_MAX_FAIL_COUNT  失败条数绝对上限（默认 20）
#   REFINE_INGEST_TOLERATED_FAILS 小批次绝对容忍下限（默认 2）
#   REFINE_LOG_MAX_BYTES          日志轮转阈值（默认 5242880）

# 返回文件字节数；文件不存在返回 0。
refine_file_size() {
  local path="$1"
  if [[ -f "$path" ]]; then
    wc -c <"$path" | tr -d ' '
  else
    echo 0
  fi
}

# 把万分比整数格式化成 "x.yy%" 形式，避免引入浮点依赖。
refine_format_bp() {
  local bp="$1"
  printf '%d.%02d%%' "$((bp / 100))" "$((bp % 100))"
}

# 判定 ingest 结果：evaluate_ingest_result <ingest 输出文件> <原始退出码>
# 成功返回 0，失败返回 1。低于阈值的失败也会在 stderr 打出可见统计。
evaluate_ingest_result() {
  local out_file="$1"
  local raw_rc="$2"

  local max_rate="${REFINE_INGEST_MAX_FAIL_RATE:-5}"
  local max_count="${REFINE_INGEST_MAX_FAIL_COUNT:-20}"
  local tolerated="${REFINE_INGEST_TOLERATED_FAILS:-2}"

  local summary
  summary=$(grep -E '^完成: 处理 [0-9]+, 跳过重复 [0-9]+, 过滤 [0-9]+, 失败 [0-9]+,' "$out_file" 2>/dev/null | tail -n 1 || true)

  # U-29：解析不到汇总行说明进程崩溃/被杀/未跑完，必须 fail-closed，不能被阈值吞掉。
  if [[ -z "$summary" ]]; then
    echo "ERROR: 无法解析 ingest 汇总行（原始退出码=${raw_rc}），按失败处理" >&2
    return 1
  fi

  local processed failed
  processed=$(echo "$summary" | sed -E 's/^完成: 处理 ([0-9]+),.*/\1/')
  failed=$(echo "$summary" | sed -E 's/.*, 失败 ([0-9]+),.*/\1/')

  local attempted=$((processed + failed))
  if [[ "$failed" -eq 0 ]]; then
    echo "INFO: ingest 全部成功，处理 ${processed} 个会话" >&2
    return 0
  fi

  # 跳过重复/被过滤的会话不算尝试，分母只取 处理 + 失败。
  local rate_bp=$((failed * 10000 / attempted))
  local rate_str
  rate_str=$(refine_format_bp "$rate_bp")
  local limit_desc="rate<=${max_rate}% count<=${max_count}"

  if [[ "$failed" -le "$tolerated" ]] || { [[ "$rate_bp" -le $((max_rate * 100)) ]] && [[ "$failed" -le "$max_count" ]]; }; then
    echo "WARN: ingest 部分失败 ${failed}/${attempted} (${rate_str})，未超过阈值 ${limit_desc}，本次视为成功；失败会话下次运行续传" >&2
    return 0
  fi

  echo "ERROR: ingest 失败率超标 ${failed}/${attempted} (${rate_str})，阈值 ${limit_desc}；不更新 last-refresh-ok" >&2
  return 1
}

# 日志轮转：rotate_log_if_needed <日志路径> [阈值字节数]
# 必须用 copytruncate（cp 后清空原文件），因为 launchd 在启动脚本前
# 就已用 O_APPEND 打开该 inode，rename 会让本次输出写进旧文件。
rotate_log_if_needed() {
  local log_path="$1"
  local max_bytes="${2:-${REFINE_LOG_MAX_BYTES:-5242880}}"

  [[ -f "$log_path" ]] || return 0

  local size
  size=$(refine_file_size "$log_path")
  [[ "$size" -gt "$max_bytes" ]] || return 0

  if cp "$log_path" "${log_path}.1" 2>/dev/null; then
    : >"$log_path"
    echo "INFO: 日志已轮转 ${size} bytes > ${max_bytes}，历史保存到 ${log_path}.1" >&2
  else
    echo "WARN: 日志轮转失败，无法写入 ${log_path}.1" >&2
  fi
}

# 解析 refine 数据库路径，与 packages/core/src/infra/paths.rs 的默认值对齐。
resolve_refine_db_path() {
  if [[ -n "${REFINE_DB_PATH:-}" ]]; then
    echo "$REFINE_DB_PATH"
    return 0
  fi
  local data_dir
  if [[ "$(uname -s)" == "Darwin" ]]; then
    data_dir="${XDG_DATA_HOME:-$HOME/Library/Application Support}"
  else
    data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
  fi
  echo "${data_dir}/refine/refine.db"
}

# WAL 收尾：checkpoint_wal [db 路径]
# busy / sqlite3 缺失 / DB 不存在都只降级为 warning：WAL 未回收不会造成
# 用户可见的数据缺失或错误输出，只是磁盘占用，不符合 U-29 的 error 判定。
checkpoint_wal() {
  local db="${1:-$(resolve_refine_db_path)}"

  if ! command -v sqlite3 >/dev/null 2>&1; then
    echo "WARN: 未找到 sqlite3，跳过 WAL checkpoint" >&2
    return 0
  fi
  if [[ ! -f "$db" ]]; then
    echo "WARN: 数据库不存在，跳过 WAL checkpoint: ${db}" >&2
    return 0
  fi

  local wal="${db}-wal"
  local before after result
  before=$(refine_file_size "$wal")
  result=$(sqlite3 "$db" 'PRAGMA wal_checkpoint(TRUNCATE);' 2>/dev/null || echo "")
  after=$(refine_file_size "$wal")

  case "$result" in
    0\|*)
      echo "INFO: WAL checkpoint 完成 ${before} -> ${after} bytes" >&2
      ;;
    "")
      echo "WARN: WAL checkpoint 未返回结果，WAL 仍为 ${after} bytes" >&2
      ;;
    *)
      echo "WARN: WAL checkpoint 被占用未完成 (result=${result})，WAL 仍为 ${after} bytes；常见原因是 refine-server 常驻连接" >&2
      ;;
  esac
  return 0
}
