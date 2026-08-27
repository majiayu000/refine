#!/usr/bin/env bash
set -euo pipefail

REFINE_BIN="${REFINE_COGNITIVE_PORTRAIT_REFINE_BIN:-refine}"
period=90
cutoff=""
output=""
db="${REFINE_DB_PATH:-}"

usage() {
  echo "usage: $0 --output PATH [--db PATH] [--period DAYS] [--cutoff RFC3339]" >&2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --db) db="${2:?missing --db value}"; shift 2 ;;
    --period) period="${2:?missing --period value}"; shift 2 ;;
    --cutoff) cutoff="${2:?missing --cutoff value}"; shift 2 ;;
    --output) output="${2:?missing --output value}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ -n "$output" ]] || { usage; echo "--output is required" >&2; exit 2; }
[[ "$period" =~ ^[1-9][0-9]*$ ]] || { echo "--period must be a positive integer" >&2; exit 2; }
command -v "$REFINE_BIN" >/dev/null 2>&1 || {
  echo "refine binary not found: $REFINE_BIN" >&2
  exit 1
}

args=()
[[ -n "$db" ]] && args+=(--db "$db")
args+=(cognitive-portrait collect --period "$period" --output "$output")
[[ -n "$cutoff" ]] && args+=(--cutoff "$cutoff")
exec "$REFINE_BIN" "${args[@]}"
