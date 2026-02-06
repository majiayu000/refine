#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Selective importer for Claude Code history (.jsonl) -> Refine server.

Usage:
  scripts/import_claude_code.sh list [options]
  scripts/import_claude_code.sh import [options]

Commands:
  list
      List candidate Claude Code sessions under ~/.claude/projects.

  import
      Import selected sessions to Refine.

Important:
  - By default, import is selective. You must provide one of:
      --session <id-or-file>  (repeatable)
      --marker <text>
      --all

Options:
  --root <dir>         Claude projects root (default: current repo project dir under ~/.claude/projects if exists, otherwise ~/.claude/projects)
  --api-base <url>     Refine API base (default: http://localhost:8787)
  --token <token>      Bearer token when REFINE_API_TOKEN is enabled
  --source <name>      Source field in payload (default: claude_code)
  --title-prefix <t>   Title prefix (default: Claude Code Session)
  --session <value>    Session id or .jsonl file path (repeatable)
  --marker <text>      Only include sessions whose file contains this marker text
  --since-days <n>     Only include sessions updated within n days
  --limit <n>          Max sessions to list/import (default: 20)
  --all                Import all matched sessions (use with care)
  --dry-run            Print candidates only, do not upload
  -h, --help           Show this help

Examples:
  scripts/import_claude_code.sh list --limit 10
  scripts/import_claude_code.sh import --session 9bf08b81-21d4-4768-8c57-852e0bd17649
  scripts/import_claude_code.sh import --marker "#refine" --since-days 7
  scripts/import_claude_code.sh import --all --since-days 1 --dry-run
EOF
}

require_cmds() {
  local missing=0
  for cmd in jq curl shasum; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "Missing required command: $cmd" >&2
      missing=1
    fi
  done
  if [[ $missing -ne 0 ]]; then
    exit 1
  fi
}

mtime_epoch() {
  local file="$1"
  if stat -f %m "$file" >/dev/null 2>&1; then
    stat -f %m "$file"
  else
    stat -c %Y "$file"
  fi
}

fmt_local_time() {
  local epoch="$1"
  if date -r "$epoch" "+%Y-%m-%d %H:%M:%S" >/dev/null 2>&1; then
    date -r "$epoch" "+%Y-%m-%d %H:%M:%S"
  else
    date -d "@$epoch" "+%Y-%m-%d %H:%M:%S"
  fi
}

fmt_iso_utc() {
  local epoch="$1"
  if date -ur "$epoch" "+%Y-%m-%dT%H:%M:%SZ" >/dev/null 2>&1; then
    date -ur "$epoch" "+%Y-%m-%dT%H:%M:%SZ"
  else
    date -u -d "@$epoch" "+%Y-%m-%dT%H:%M:%SZ"
  fi
}

now_epoch() {
  date +%s
}

extract_conversation() {
  local file="$1"
  jq -Rr '
    def msg_text:
      (.message?.content) as $c
      | if ($c|type) == "string" then $c
        elif ($c|type) == "array" then ([ $c[]? | select(.type=="text") | .text ] | join("\n"))
        elif ($c|type) == "object" then ($c.text // "")
        else (.message?.text // .text // .content // "")
        end;
    (fromjson? // empty) as $j
    | if ($j|type) != "object" then empty
      elif $j.type == "user" then
        ($j | msg_text | gsub("^\\s+|\\s+$"; "")) as $t
        | if ($t|length) > 0 then "Human: \($t)" else empty end
      elif $j.type == "assistant" then
        ($j | msg_text | gsub("^\\s+|\\s+$"; "")) as $t
        | if ($t|length) > 0 then "Assistant: \($t)" else empty end
      else empty end
  ' "$file"
}

first_user_preview() {
  local file="$1"
  jq -Rr '
    def msg_text:
      (.message?.content) as $c
      | if ($c|type) == "string" then $c
        elif ($c|type) == "array" then ([ $c[]? | select(.type=="text") | .text ] | join("\n"))
        elif ($c|type) == "object" then ($c.text // "")
        else (.message?.text // .text // .content // "")
        end;
    (fromjson? // empty) as $j
    | select(($j|type) == "object" and $j.type == "user")
    | ($j | msg_text | gsub("\\s+"; " ") | gsub("^\\s+|\\s+$"; ""))
    | select(length > 0)
  ' "$file" | sed -n '1p'
}

resolve_session_file() {
  local root="$1"
  local arg="$2"
  if [[ -f "$arg" ]]; then
    echo "$arg"
    return 0
  fi
  local found
  found="$(find "$root" -type f -name "${arg}.jsonl" 2>/dev/null | head -n 1 || true)"
  if [[ -n "$found" ]]; then
    echo "$found"
    return 0
  fi
  return 1
}

DEFAULT_ROOT="${HOME}/.claude/projects"
probe_dir="$(pwd)"
while [[ "$probe_dir" != "/" ]]; do
  project_key="-$(echo "$probe_dir" | sed 's#^/##; s#/#-#g')"
  if [[ -d "${DEFAULT_ROOT}/${project_key}" ]]; then
    DEFAULT_ROOT="${DEFAULT_ROOT}/${project_key}"
    break
  fi
  probe_dir="$(dirname "$probe_dir")"
done

ROOT="${CLAUDE_PROJECTS_ROOT:-$DEFAULT_ROOT}"
API_BASE="${REFINE_API_BASE:-http://localhost:8787}"
TOKEN="${REFINE_API_TOKEN:-}"
SOURCE="claude_code"
TITLE_PREFIX="Claude Code Session"
MARKER=""
SINCE_DAYS=""
LIMIT=20
DRY_RUN=0
ALL=0
declare -a SESSIONS=()

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

COMMAND="$1"
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) ROOT="$2"; shift 2 ;;
    --api-base) API_BASE="$2"; shift 2 ;;
    --token) TOKEN="$2"; shift 2 ;;
    --source) SOURCE="$2"; shift 2 ;;
    --title-prefix) TITLE_PREFIX="$2"; shift 2 ;;
    --session) SESSIONS+=("$2"); shift 2 ;;
    --marker) MARKER="$2"; shift 2 ;;
    --since-days) SINCE_DAYS="$2"; shift 2 ;;
    --limit) LIMIT="$2"; shift 2 ;;
    --all) ALL=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

require_cmds

if [[ ! -d "$ROOT" ]]; then
  echo "Claude projects root not found: $ROOT" >&2
  exit 1
fi

if ! [[ "$LIMIT" =~ ^[0-9]+$ ]] || [[ "$LIMIT" -lt 1 ]]; then
  echo "--limit must be a positive integer" >&2
  exit 1
fi

since_cutoff=0
if [[ -n "$SINCE_DAYS" ]]; then
  if ! [[ "$SINCE_DAYS" =~ ^[0-9]+$ ]]; then
    echo "--since-days must be a non-negative integer" >&2
    exit 1
  fi
  since_cutoff="$(( $(now_epoch) - SINCE_DAYS * 86400 ))"
fi

collect_files() {
  find "$ROOT" -type f -name '*.jsonl' 2>/dev/null
}

matches_filters() {
  local file="$1"
  local mt
  mt="$(mtime_epoch "$file")"
  if [[ "$since_cutoff" -gt 0 && "$mt" -lt "$since_cutoff" ]]; then
    return 1
  fi
  if [[ -n "$MARKER" ]] && ! grep -qF "$MARKER" "$file"; then
    return 1
  fi
  return 0
}

if [[ "$COMMAND" == "list" ]]; then
  tmp="$(mktemp)"
  trap 'rm -f "$tmp"' EXIT

  while IFS= read -r file; do
    [[ -f "$file" ]] || continue
    if ! matches_filters "$file"; then
      continue
    fi
    mt="$(mtime_epoch "$file")"
    sid="$(basename "$file" .jsonl)"
    size_kb="$(( ($(wc -c < "$file") + 1023) / 1024 ))"
    when="$(fmt_local_time "$mt")"
    printf "%s\t%s\t%s\t%s\t%s\n" "$mt" "$sid" "$size_kb" "$when" "$file" >> "$tmp"
  done < <(collect_files)

  if [[ ! -s "$tmp" ]]; then
    echo "No session candidates found."
    exit 0
  fi

  sorted_tmp="$(mktemp)"
  sort -nr "$tmp" > "$sorted_tmp"
  index=0
  while IFS=$'\t' read -r mt sid size_kb when file; do
    index=$((index + 1))
    if [[ "$index" -gt "$LIMIT" ]]; then
      break
    fi
    preview="$(first_user_preview "$file")"
    preview="$(echo "$preview" | tr '\t' ' ' | tr '\n' ' ' | sed 's/[[:space:]]\+/ /g')"
    preview="${preview:0:80}"
    printf "%3d) %s  %sKB  %s\n" "$index" "$sid" "$size_kb" "$when"
    if [[ -n "$preview" ]]; then
      printf "     %s\n" "$preview"
    fi
    printf "     %s\n" "$file"
  done < "$sorted_tmp"
  rm -f "$sorted_tmp"
  exit 0
fi

if [[ "$COMMAND" != "import" ]]; then
  echo "Unknown command: $COMMAND" >&2
  usage
  exit 1
fi

declare -a candidates=()

if [[ "${#SESSIONS[@]}" -gt 0 ]]; then
  for s in "${SESSIONS[@]}"; do
    if file="$(resolve_session_file "$ROOT" "$s")"; then
      candidates+=("$file")
    else
      echo "Session not found: $s" >&2
    fi
  done
else
  if [[ "$ALL" -ne 1 && -z "$MARKER" ]]; then
    echo "Selective import required. Use --session, --marker, or --all." >&2
    exit 1
  fi
  while IFS= read -r file; do
    [[ -f "$file" ]] || continue
    candidates+=("$file")
  done < <(collect_files)
fi

if [[ "${#candidates[@]}" -eq 0 ]]; then
  echo "No candidate sessions."
  exit 1
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

for file in "${candidates[@]}"; do
  if ! matches_filters "$file"; then
    continue
  fi
  mt="$(mtime_epoch "$file")"
  printf "%s\t%s\n" "$mt" "$file" >> "$tmp"
done

sorted_tmp="$(mktemp)"
sort -nr "$tmp" > "$sorted_tmp"
mapfile -t ordered < <(awk -F '\t' -v limit="$LIMIT" 'NR <= limit {print $2}' "$sorted_tmp")
rm -f "$sorted_tmp"

if [[ "${#ordered[@]}" -eq 0 ]]; then
  echo "No sessions matched filters."
  exit 1
fi

imported=0
skipped=0
failed=0

for file in "${ordered[@]}"; do
  sid="$(basename "$file" .jsonl)"
  mt="$(mtime_epoch "$file")"
  captured_at="$(fmt_iso_utc "$mt")"
  idempotency_key="$(printf "%s|%s|%s" "$file" "$mt" "$SOURCE" | shasum -a 256 | awk '{print $1}')"
  title="${TITLE_PREFIX} ${sid}"
  url="claude-code://session/${sid}"

  convo_tmp="$(mktemp)"
  extract_conversation "$file" > "$convo_tmp"
  if [[ ! -s "$convo_tmp" ]]; then
    echo "Skip (empty conversation): $sid"
    rm -f "$convo_tmp"
    skipped=$((skipped + 1))
    continue
  fi

  payload="$(jq -Rs \
    --arg content "$(cat "$convo_tmp")" \
    --arg url "$url" \
    --arg source "$SOURCE" \
    --arg title "$title" \
    --arg captured_at "$captured_at" \
    --arg idempotency_key "$idempotency_key" \
    '{content:$content,url:$url,source:$source,title:$title,captured_at:$captured_at,idempotency_key:$idempotency_key}' < /dev/null)"
  rm -f "$convo_tmp"

  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "DRY-RUN import: $sid"
    echo "  file: $file"
    echo "  title: $title"
    echo "  captured_at: $captured_at"
    imported=$((imported + 1))
    continue
  fi

  headers=(-H "Content-Type: application/json")
  if [[ -n "$TOKEN" ]]; then
    headers+=(-H "Authorization: Bearer $TOKEN")
  fi

  response="$(printf "%s" "$payload" | curl -sS -X POST "$API_BASE/v1/conversations" "${headers[@]}" --data-binary @-)"
  if echo "$response" | jq -e '.success == true' >/dev/null 2>&1; then
    cid="$(echo "$response" | jq -r '.conversation_id // ""')"
    jid="$(echo "$response" | jq -r '.job_id // ""')"
    dedup="$(echo "$response" | jq -r '.deduplicated // false')"
    echo "Imported: $sid  conversation_id=$cid job_id=$jid deduplicated=$dedup"
    imported=$((imported + 1))
  else
    msg="$(echo "$response" | jq -r '.message // "unknown error"')"
    echo "Failed: $sid  $msg" >&2
    failed=$((failed + 1))
  fi
done

echo
echo "Done. imported=$imported skipped=$skipped failed=$failed"
