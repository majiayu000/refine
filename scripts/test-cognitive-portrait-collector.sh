#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_DIR=$(cd "${SCRIPT_DIR}/.." && pwd)
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

if [[ -z "${REFINE_TEST_BIN:-}" ]]; then
  cargo build -q -p refine-cli --bin refine
  REFINE_TEST_BIN="${PROJECT_DIR}/target/debug/refine"
fi

db="${TEST_ROOT}/fixture.db"
bundle="${TEST_ROOT}/bundle.json"
bundle_again="${TEST_ROOT}/bundle-again.json"
cutoff="2026-08-28T00:00:00Z"
sqlite3 "$db" < "${PROJECT_DIR}/packages/core/src/infra/schema.sql"
sqlite3 "$db" <<'SQL'
INSERT INTO documents (id,title,raw_content,source,url,captured_at,created_at,updated_at) VALUES
 ('codex-doc','codex','raw','codex-session','codex://1','2026-08-20T00:00:00Z','2026-08-27T00:00:00Z','2026-08-27T00:00:00Z'),
 ('remem-doc','remem','raw','remem-raw-session','remem://1','2026-08-19T00:00:00Z','2026-08-27T00:00:00Z','2026-08-27T00:00:00Z'),
 ('grok-doc','grok','raw','grok-knowledge','grok://1','2026-08-18T00:00:00Z','2026-08-27T00:00:00Z','2026-08-27T00:00:00Z'),
 ('gemini-doc','gemini','raw','gemini-knowledge','gemini://1','2026-08-17T00:00:00Z','2026-08-27T00:00:00Z','2026-08-27T00:00:00Z'),
 ('claude-doc','claude','raw','claude-code-session','claude://1','2026-05-20T00:00:00Z','2026-08-27T00:00:00Z','2026-08-27T00:00:00Z'),
 ('stale-doc','stale','raw','codex-session','codex://stale','2026-01-01T00:00:00Z','2026-08-27T00:00:00Z','2026-08-27T00:00:00Z');

INSERT INTO items (id,item_type,title,summary,content,tags,source,created_at,updated_at,document_id,excerpt) VALUES
 ('codex-item','observation','Codex decision','summary','知识:\n- cohort contract\n阻力:\n- stale source','["refine","decision","competent"]',NULL,'2026-08-27T00:00:00Z','2026-08-27T00:00:00Z','codex-doc','codex evidence'),
 ('remem-item','observation','Remem summary','summary','模式:\n- preserve unknown provenance','["refine","delegation"]',NULL,'2026-08-27T00:00:00Z','2026-08-27T00:00:00Z','remem-doc','remem evidence'),
 ('grok-item','observation','Grok legacy note','summary','知识:\n- legacy note','["refine"]',NULL,'2026-08-27T00:00:00Z','2026-08-27T00:00:00Z','grok-doc','grok evidence'),
 ('claude-item','observation','Claude bugfix','summary','模式:\n- verify current head','["refine","bugfix","review"]',NULL,'2026-08-27T00:00:00Z','2026-08-27T00:00:00Z','claude-doc','claude evidence'),
 ('stale-item','observation','Recent ingest old event','summary','知识:\n- should be excluded','["refine"]',NULL,'2026-08-27T00:00:00Z','2026-08-27T00:00:00Z','stale-doc','stale evidence');
SQL

REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" \
  REFINE_DB_PATH="$db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" \
  --period 90 --cutoff "$cutoff" --output "$bundle"
REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" \
  REFINE_DB_PATH="$db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" \
  --period 90 --cutoff "$cutoff" --output "$bundle_again"
cmp -s "$bundle" "$bundle_again" || fail 'fixed-cutoff collector output is not deterministic'

jq -e '.schema_version == 1 and .collector_version == "cognitive-portrait-collector-v1"' "$bundle" >/dev/null \
  || fail 'bundle version contract is missing'
jq -e '.current.evidence | map(.item_id) | sort == ["codex-item", "remem-item"]' "$bundle" >/dev/null \
  || fail 'event-time/source cohort did not exclude stale or unsupported observations'
jq -e '.previous.evidence | map(.item_id) == ["claude-item"]' "$bundle" >/dev/null \
  || fail 'previous 90-day window is wrong'
jq -e '.manifest.current_window.source_counts | map(.source) | sort == ["codex", "platform_unknown"]' "$bundle" >/dev/null \
  || fail 'Codex/remem provenance reporting is wrong'
jq -e '.manifest.current_window.unsupported_source_counts[0].source == "grok-knowledge"' "$bundle" >/dev/null \
  || fail 'Grok knowledge-only source was not disclosed'
jq -e '.current.metrics.total_sessions == 2 and .comparison.status == "DEGRADED" and (.comparison.comparable | not)' "$bundle" >/dev/null \
  || fail 'unsupported source did not suppress trend comparability'
jq -e '.current.evidence | map(.item_id) | index("stale-item") == null' "$bundle" >/dev/null \
  || fail 'recent ingest time overrode old event time'

candidate="${TEST_ROOT}/candidate.md"
previous="${TEST_ROOT}/previous.md"
quality="${TEST_ROOT}/quality.json"
printf '%s\n\n%s\n\n%s\n' \
  '# 认知画像 v4' \
  '[事实] 当前窗口 session 数为 2。[bundle:/current/metrics/total_sessions]' \
  '[建议] 修复 unsupported 来源后再比较。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:comparison.status 为 OK]' \
  > "$candidate"
printf '%s\n\n%s\n' '# 旧画像' '这是与本期完全不同的旧画像段落，用于验证内容的新颖度门禁。' > "$previous"
REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" \
  "${SCRIPT_DIR}/validate-cognitive-portrait.sh" \
  --bundle "$bundle" --portrait "$candidate" --previous "$previous" --output "$quality"
jq -e '.passed and .factual_traceability_rate == 1 and .unsupported_number_rate == 0 and .action_verifiability_rate == 1' "$quality" >/dev/null \
  || fail 'valid candidate failed evidence quality gate'

printf '%s\n\n%s\n' \
  '[事实][趋势] session 从 1→2。[bundle:/current/metrics/total_sessions]' \
  '[建议] 继续。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:重跑]' \
  > "$candidate"
if REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" \
  "${SCRIPT_DIR}/validate-cognitive-portrait.sh" \
  --bundle "$bundle" --portrait "$candidate" --output "$quality"; then
  fail 'degraded comparison accepted a trend claim'
fi
jq -e '(.passed | not) and (.errors | length > 0)' "$quality" >/dev/null \
  || fail 'failed quality report was not persisted'

empty_db="${TEST_ROOT}/empty.db"
sqlite3 "$empty_db" < "${PROJECT_DIR}/packages/core/src/infra/schema.sql"
if REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" REFINE_DB_PATH="$empty_db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" --cutoff "$cutoff" --output "${TEST_ROOT}/empty.json" \
  >"${TEST_ROOT}/empty.log" 2>&1; then
  fail 'empty core data was accepted'
fi
grep -q 'NO_CORE_DATA' "${TEST_ROOT}/empty.log" || fail 'empty data error is not explicit'

invalid_db="${TEST_ROOT}/invalid.db"
sqlite3 "$invalid_db" 'CREATE TABLE items (id TEXT PRIMARY KEY);'
if REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" REFINE_DB_PATH="$invalid_db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" --cutoff "$cutoff" --output "${TEST_ROOT}/invalid.json" \
  >"${TEST_ROOT}/invalid.log" 2>&1; then
  fail 'schema-invalid core data was accepted'
fi
grep -q 'SCHEMA_INVALID' "${TEST_ROOT}/invalid.log" || fail 'schema error is not explicit'

echo 'All cognitive portrait collector tests passed'
