#!/usr/bin/env bash
set -euo pipefail

REFINE_BIN="${REFINE_COGNITIVE_PORTRAIT_REFINE_BIN:-refine}"
bundle=""
portrait=""
previous=""
output=""

usage() {
  echo "usage: $0 --bundle PATH --portrait PATH --output PATH [--previous PATH]" >&2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle) bundle="${2:?missing --bundle value}"; shift 2 ;;
    --portrait) portrait="${2:?missing --portrait value}"; shift 2 ;;
    --previous) previous="${2:?missing --previous value}"; shift 2 ;;
    --output) output="${2:?missing --output value}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ -n "$bundle" && -n "$portrait" && -n "$output" ]] || {
  usage
  echo "--bundle, --portrait and --output are required" >&2
  exit 2
}
command -v "$REFINE_BIN" >/dev/null 2>&1 || {
  echo "refine binary not found: $REFINE_BIN" >&2
  exit 1
}

args=(cognitive-portrait validate --bundle "$bundle" --portrait "$portrait" --output "$output")
[[ -n "$previous" ]] && args+=(--previous "$previous")
exec "$REFINE_BIN" "${args[@]}"
