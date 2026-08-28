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
 ('codex-item','observation','Shared title','summary','知识:\n- cohort contract\n阻力:\n- stale source','["zeta","decision","competent"]',NULL,'2026-08-27T00:00:00Z','2026-08-27T00:00:00Z','codex-doc','codex evidence'),
 ('remem-item','observation','Shared title','summary','模式:\n- preserve unknown provenance','["alpha","delegation"]',NULL,'2026-08-27T00:00:00Z','2026-08-27T00:00:00Z','remem-doc','remem evidence'),
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

jq -e '.schema_version == 2 and .collector_version == "cognitive-portrait-collector-v2"' "$bundle" >/dev/null \
  || fail 'bundle version contract is missing'
jq -e '.claim_catalog.schema_version == 2
  and ([.claim_catalog.claims[].claim_id] == ([.claim_catalog.claims[].claim_id] | sort | unique))
  and ([.claim_catalog.claims[].kind] | index("trend") == null)' "$bundle" >/dev/null \
  || fail 'DEGRADED bundle claim catalog is not stable or trend-free'
jq -e '.current.evidence | map(.item_id) | sort == ["codex-item", "remem-item"]' "$bundle" >/dev/null \
  || fail 'event-time/source cohort did not exclude stale or unsupported observations'
jq -e '.previous.evidence | map(.item_id) == ["claude-item"]' "$bundle" >/dev/null \
  || fail 'previous 90-day window is wrong'
jq -e '.manifest.current_window.source_counts | map(.source) | sort == ["codex", "platform_unknown"]' "$bundle" >/dev/null \
  || fail 'Codex/remem provenance reporting is wrong'
jq -e '.manifest.current_window.unsupported_sources.entries[0].source == "grok-knowledge"' "$bundle" >/dev/null \
  || fail 'Grok knowledge-only source was not disclosed'
jq -e '.current.metrics.total_sessions == 2 and .comparison.status == "DEGRADED" and (.comparison.comparable | not)' "$bundle" >/dev/null \
  || fail 'unsupported source did not suppress trend comparability'
jq -e '.current.metrics.project_ranking.entries | map([.value,.count]) == [["alpha",1],["zeta",1]]' "$bundle" >/dev/null \
  || fail 'equal-count project ranking is not name-stable'
jq -e '.current.evidence_selection.policy_version == "stratified-provenance-v1"
  and .current.evidence_selection.eligible_observations == 2
  and .current.evidence_selection.selected_observations == 2
  and .current.evidence_selection.omitted_observations == 0
  and (.current.evidence_selection.full_payload_digest | startswith("sha256:"))
  and (.current.evidence_selection.selection_digest | startswith("sha256:"))' "$bundle" >/dev/null \
  || fail 'bounded projection disclosure is incomplete'
jq -e '.current.evidence | map({key:.item_id,value:.project}) | from_entries == {"codex-item":"zeta","remem-item":"alpha"}' "$bundle" >/dev/null \
  || fail 'same-title observations were not assigned by direct item/project identity'
jq -e '.current.evidence | map(.item_id) | index("stale-item") == null' "$bundle" >/dev/null \
  || fail 'recent ingest time overrode old event time'

candidate="${TEST_ROOT}/candidate.md"
previous="${TEST_ROOT}/previous.md"
quality="${TEST_ROOT}/quality.json"
printf '%b\n' '# 认知画像 v4\n\n## L1：认知演进\n\n[事实] 本期胜过旧窗口。[bundle:/comparison/status]\n\n## L2：战略定位\n\n本层记录项目来源边界。\n\n## L3：工作方式健康度\n\n本层记录摩擦与协作边界。\n\n## L4：成长处方\n\n[建议] 继续。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:metric|/comparison/status|eq|"DEGRADED"]' > "$candidate"
if REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" \
  "${SCRIPT_DIR}/validate-cognitive-portrait.sh" \
  --bundle "$bundle" --portrait "$candidate" --output "$quality"; then
  fail 'degraded comparison accepted a trend claim'
fi
jq -e '(.passed | not) and (.errors | length > 0)' "$quality" >/dev/null \
  || fail 'failed quality report was not persisted'

sqlite3 "$db" "DELETE FROM items WHERE id = 'grok-item';"
comparable_bundle="${TEST_ROOT}/comparable-bundle.json"
REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" REFINE_DB_PATH="$db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" \
  --period 90 --cutoff "$cutoff" --output "$comparable_bundle"
jq -e '.comparison.status == "OK" and .comparison.comparable
  and ([.claim_catalog.claims[].kind] | index("trend") != null)' "$comparable_bundle" >/dev/null \
  || fail 'comparable fixture did not emit trend catalog claims'
claim_line=$(jq -r '.claim_catalog.claims[] | select(.claim_id == "fact.current.total_sessions") | .rendered_line' "$comparable_bundle")
[[ -n "$claim_line" ]] || fail 'canonical session claim is missing'
printf '%s\n\n%s\n\n%s\n' \
  '# 认知画像 v4

## L1：认知演进' \
  "$claim_line" \
  '[建议] 核对 cohort。[evidence:obs:codex-item] [owner:lifcc] [due:2026-09-01] [verify:metric|/comparison/status|eq|"OK"]

这是足够长且全新的可见分析段落，用来验证 canonical claim 与 typed action 合约。

## L2：战略定位

本层记录项目来源边界。

## L3：工作方式健康度

本层记录摩擦与协作边界。

## L4：成长处方

本层记录可验证的行动边界。' > "$candidate"
printf '%s\n\n%s\n' '# 旧画像' '这是与本期完全不同的旧画像段落，用于验证内容的新颖度门禁。' > "$previous"
REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" \
  "${SCRIPT_DIR}/validate-cognitive-portrait.sh" \
  --bundle "$comparable_bundle" --portrait "$candidate" --previous "$previous" --output "$quality"
jq -e '.passed and .factual_traceability_rate == 1 and .unsupported_number_rate == 0 and .action_verifiability_rate == 1' "$quality" >/dev/null \
  || fail 'canonical candidate failed evidence quality gate'

# Production-scale real-schema fixture: 100,000 eligible observations across
# both windows, 400 projects, three supported provenance classes, and unique
# high-cardinality dimension values. The projection must stay bounded without
# changing the full cohort metrics or outer 64 MiB contract.
large_db="${TEST_ROOT}/large-fixture.db"
large_bundle="${TEST_ROOT}/large-bundle.json"
large_bundle_again="${TEST_ROOT}/large-bundle-again.json"
sqlite3 "$large_db" < "${PROJECT_DIR}/packages/core/src/infra/schema.sql"
sqlite3 "$large_db" <<'SQL'
PRAGMA journal_mode=MEMORY;
PRAGMA synchronous=OFF;
BEGIN;
WITH RECURSIVE sequence(n) AS (
  SELECT 0
  UNION ALL
  SELECT n + 1 FROM sequence WHERE n < 399
)
INSERT INTO documents (id,title,raw_content,source,url,captured_at,created_at,updated_at)
SELECT
  printf('large-doc-%03d', n),
  printf('large document %03d', n),
  'bounded fixture transcript',
  CASE n % 3
    WHEN 0 THEN 'codex-session'
    WHEN 1 THEN 'claude-code-session'
    ELSE 'remem-raw-session'
  END,
  printf('fixture://large/%03d', n),
  CASE WHEN n < 200 THEN '2026-08-20T00:00:00Z' ELSE '2026-05-20T00:00:00Z' END,
  '2026-08-27T00:00:00Z',
  '2026-08-27T00:00:00Z'
FROM sequence;

WITH RECURSIVE sequence(n) AS (
  SELECT 0
  UNION ALL
  SELECT n + 1 FROM sequence WHERE n < 99999
)
INSERT INTO items (id,item_type,title,summary,content,tags,source,created_at,updated_at,document_id,excerpt)
SELECT
  printf('large-item-%06d', n),
  'observation',
  printf('unique title %06d with deterministic evidence payload', n),
  printf('unique summary %06d with deterministic evidence payload', n),
  printf('知识:') || char(10) || printf('- unique knowledge %06d', n)
    || char(10) || printf('模式:') || char(10) || printf('- unique pattern %06d', n)
    || char(10) || printf('架构:') || char(10) || printf('- unique architecture %06d', n)
    || char(10) || printf('阻力:') || char(10) || printf('- unique friction %06d', n)
    || char(10) || printf('工具:') || char(10) || printf('- tool-%04d', n % 1000),
  printf(
    '["project-%03d","%s","competent"]',
    n % 400,
    CASE n % 3 WHEN 0 THEN 'decision' WHEN 1 THEN 'bugfix' ELSE 'review' END
  ),
  NULL,
  '2026-08-27T00:00:00Z',
  '2026-08-27T00:00:00Z',
  printf('large-doc-%03d', n % 400),
  printf('unique excerpt %06d', n)
FROM sequence;
COMMIT;
SQL

REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" REFINE_DB_PATH="$large_db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" \
  --period 90 --cutoff "$cutoff" --output "$large_bundle"
REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" REFINE_DB_PATH="$large_db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" \
  --period 90 --cutoff "$cutoff" --output "$large_bundle_again"
large_sha=$(shasum -a 256 "$large_bundle" | awk '{print $1}')
large_sha_again=$(shasum -a 256 "$large_bundle_again" | awk '{print $1}')
[[ "$large_sha" == "$large_sha_again" ]] || fail '100k cross-process bundle SHA is not deterministic'
large_bytes=$(wc -c < "$large_bundle" | tr -d '[:space:]')
(( large_bytes <= 16 * 1024 * 1024 )) || fail '100k projected bundle exceeds the internal 16 MiB budget'
jq -e '
  .manifest.current_window.eligible_observations == 50000
  and .manifest.previous_window.eligible_observations == 50000
  and .current.evidence_selection.eligible_observations == 50000
  and .previous.evidence_selection.eligible_observations == 50000
  and .current.evidence_selection.selected_observations == 2048
  and .previous.evidence_selection.selected_observations == 2048
  and .current.evidence_selection.omitted_observations == 47952
  and .previous.evidence_selection.omitted_observations == 47952
  and .current.metrics.total_sessions == 200
  and .previous.metrics.total_sessions == 200
  and .current.metrics.project_ranking.total_entries == 200
  and .current.metrics.project_ranking.selected_entries == 128
  and .current.metrics.project_ranking.omitted_entries == 72
  and .current.dimensions.knowledge.total_occurrences == 50000
  and .current.dimensions.knowledge.selected_occurrences == 128
  and .current.dimensions.knowledge.omitted_occurrences == 49872
  and .current.dimensions.knowledge.selected_values == 128
  and ([.current.evidence_selection.strata[].eligible_observations] | add) == 50000
  and ([.current.evidence_selection.strata[].selected_observations] | add) == 2048
  and ([.current.evidence_selection.strata[].omitted_observations] | add) == 47952
  and .comparison.status == "OK"
  and .comparison.comparable' "$large_bundle" >/dev/null \
  || fail '100k projection counts, bounds, or comparability are inconsistent'

# Exact escaped-byte packing: the 512-byte control-character title serializes
# much larger than its UTF-8 length. The collector must shrink the retained set
# instead of exceeding the declared 4 MiB evidence component budget.
escaped_db="${TEST_ROOT}/escaped-fixture.db"
escaped_bundle="${TEST_ROOT}/escaped-bundle.json"
sqlite3 "$escaped_db" < "${PROJECT_DIR}/packages/core/src/infra/schema.sql"
sqlite3 "$escaped_db" <<'SQL'
BEGIN;
INSERT INTO documents (id,title,raw_content,source,url,captured_at,created_at,updated_at) VALUES
 ('escaped-current','current','raw','codex-session','fixture://escaped/current','2026-08-20T00:00:00Z','2026-08-20T00:00:00Z','2026-08-20T00:00:00Z'),
 ('escaped-previous','previous','raw','codex-session','fixture://escaped/previous','2026-05-20T00:00:00Z','2026-05-20T00:00:00Z','2026-05-20T00:00:00Z');
WITH RECURSIVE sequence(n) AS (
  SELECT 0 UNION ALL SELECT n + 1 FROM sequence WHERE n < 2199
)
INSERT INTO items (id,item_type,title,summary,content,tags,source,created_at,updated_at,document_id,excerpt)
SELECT printf('escaped-%04d', n), 'observation', replace(printf('%512s',''),' ',char(1)),
  '', '', '["project","decision"]', NULL, '2026-08-20T00:00:00Z',
  '2026-08-20T00:00:00Z', 'escaped-current', NULL FROM sequence;
INSERT INTO items (id,item_type,title,summary,content,tags,source,created_at,updated_at,document_id,excerpt)
VALUES ('escaped-previous-item','observation','previous','','','["project"]',NULL,
  '2026-05-20T00:00:00Z','2026-05-20T00:00:00Z','escaped-previous',NULL);
COMMIT;
SQL
REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" REFINE_DB_PATH="$escaped_db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" \
  --period 90 --cutoff "$cutoff" --output "$escaped_bundle"
jq -e '.current.evidence_selection.eligible_observations == 2200
  and .current.evidence_selection.selected_observations < 2048
  and .current.evidence_selection.selected_observations > 0
  and ([.current.evidence_selection.strata[].selected_observations] | add)
    == .current.evidence_selection.selected_observations' "$escaped_bundle" >/dev/null \
  || fail 'JSON-escaped evidence bytes were not packed into the declared budget'
escaped_evidence_bytes=$(jq -c '.current.evidence' "$escaped_bundle" | wc -c | tr -d '[:space:]')
(( escaped_evidence_bytes <= 4 * 1024 * 1024 + 1 )) \
  || fail 'escaped evidence exceeded the 4 MiB compact JSON budget'

# High-cardinality unsupported sources also exercise the chunked metadata
# lookup beyond SQLite's variable limit. The diagnostic remains DEGRADED and
# bounded while preserving exact observation/session totals and full digest.
unsupported_db="${TEST_ROOT}/unsupported-fixture.db"
unsupported_bundle="${TEST_ROOT}/unsupported-bundle.json"
sqlite3 "$unsupported_db" < "${PROJECT_DIR}/packages/core/src/infra/schema.sql"
sqlite3 "$unsupported_db" <<'SQL'
PRAGMA journal_mode=MEMORY;
PRAGMA synchronous=OFF;
BEGIN;
WITH RECURSIVE sequence(n) AS (
  SELECT 0 UNION ALL SELECT n + 1 FROM sequence WHERE n < 34999
)
INSERT INTO documents (id,title,raw_content,source,url,captured_at,created_at,updated_at)
SELECT printf('unsupported-doc-%05d',n), 'unsupported', 'raw',
  printf('unsupported-%05d-',n) || replace(printf('%1000s',''),' ','x'),
  printf('fixture://unsupported/%05d',n), '2026-08-20T00:00:00Z',
  '2026-08-20T00:00:00Z','2026-08-20T00:00:00Z' FROM sequence;
WITH RECURSIVE sequence(n) AS (
  SELECT 0 UNION ALL SELECT n + 1 FROM sequence WHERE n < 34999
)
INSERT INTO items (id,item_type,title,summary,content,tags,source,created_at,updated_at,document_id,excerpt)
SELECT printf('unsupported-item-%05d',n),'observation','unsupported','','','[]',NULL,
  '2026-08-20T00:00:00Z','2026-08-20T00:00:00Z',printf('unsupported-doc-%05d',n),NULL
FROM sequence;
INSERT INTO documents (id,title,raw_content,source,url,captured_at,created_at,updated_at) VALUES
 ('valid-current','valid','raw','codex-session','fixture://valid/current','2026-08-20T00:00:00Z','2026-08-20T00:00:00Z','2026-08-20T00:00:00Z'),
 ('valid-previous','valid','raw','codex-session','fixture://valid/previous','2026-05-20T00:00:00Z','2026-05-20T00:00:00Z','2026-05-20T00:00:00Z');
INSERT INTO items (id,item_type,title,summary,content,tags,source,created_at,updated_at,document_id,excerpt) VALUES
 ('valid-current-item','observation','valid','','','["project"]',NULL,'2026-08-20T00:00:00Z','2026-08-20T00:00:00Z','valid-current',NULL),
 ('valid-previous-item','observation','valid','','','["project"]',NULL,'2026-05-20T00:00:00Z','2026-05-20T00:00:00Z','valid-previous',NULL);
COMMIT;
SQL
REFINE_COGNITIVE_PORTRAIT_REFINE_BIN="$REFINE_TEST_BIN" REFINE_DB_PATH="$unsupported_db" \
  "${SCRIPT_DIR}/collect-cognitive-portrait.sh" \
  --period 90 --cutoff "$cutoff" --output "$unsupported_bundle"
jq -e '.comparison.status == "DEGRADED"
  and .manifest.current_window.unsupported_sources.total_observations == 35000
  and .manifest.current_window.unsupported_sources.total_sessions == 35000
  and (.manifest.current_window.unsupported_sources.entries | length) == 128
  and .manifest.current_window.unsupported_sources.selected_observations == 128
  and .manifest.current_window.unsupported_sources.omitted_observations == 34872
  and (.manifest.current_window.unsupported_sources.full_digest | startswith("sha256:"))' \
  "$unsupported_bundle" >/dev/null \
  || fail 'high-cardinality unsupported source disclosure is not bounded and exact'
unsupported_bytes=$(wc -c < "$unsupported_bundle" | tr -d '[:space:]')
(( unsupported_bytes <= 16 * 1024 * 1024 )) \
  || fail 'high-cardinality DEGRADED diagnostic exceeds the 16 MiB budget'

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
