# Authoritative spec: docs/cognitive-portraits/SPEC.md

# Cognitive Portrait v3 — Dispatcher

Trigger keywords: 认知画像 / cognitive portrait / 认知分析 / 分析我的成长

---

## Overview

This is the **Dispatcher** skill. It orchestrates v3 multi-agent parallel architecture:

1. **Stage 1** — SQL data collection: run 8 queries against `refine.db`, write results to `/tmp/cp_data_*.txt`
2. **Stage 2** — Dispatch 4 Task agents in parallel, each writing one layer with explicit file ownership (W-14)
3. **Stage 3** — Load temp files, run acceptance checks, merge, write final report

---

## Stage 1: Data Collection

Determine the report version number N (count existing `docs/cognitive-portraits/cognitive-portrait-*-v3.md` files + 1, minimum 1).

Run the following 8 SQL queries against `refine.db` using `sqlite3`. Write each result to the corresponding `/tmp/cp_data_N.txt` file, where N matches the query number.

```bash
# Set DB path
DB=$(ls ~/.local/share/refine/refine.db ~/.refine/refine.db 2>/dev/null | head -1)
[ -z "$DB" ] && { echo "ERROR: refine.db not found"; exit 1; }

# Query 1: Recent observations overview (last 90 days)
sqlite3 "$DB" "
SELECT i.cognitive_level, i.collaboration_mode, i.tool,
       substr(i.created_at,1,10) as date, i.title
FROM items i
WHERE i.item_type='observation'
  AND i.created_at >= date('now','-90 days')
ORDER BY i.created_at DESC
LIMIT 200
" > /tmp/cp_data_1.txt

# Query 2: Decision patterns
sqlite3 "$DB" "
SELECT i.title, i.content, substr(i.created_at,1,10) as date
FROM items i
WHERE i.item_type='observation'
  AND i.decision IS NOT NULL AND i.decision != ''
  AND i.created_at >= date('now','-90 days')
ORDER BY i.created_at DESC
LIMIT 100
" > /tmp/cp_data_2.txt

# Query 3: Bug fix patterns
sqlite3 "$DB" "
SELECT i.title, i.content, substr(i.created_at,1,10) as date
FROM items i
WHERE i.item_type='observation'
  AND i.bugfix IS NOT NULL AND i.bugfix != ''
  AND i.created_at >= date('now','-90 days')
ORDER BY i.created_at DESC
LIMIT 100
" > /tmp/cp_data_3.txt

# Query 4: Knowledge and patterns acquired
sqlite3 "$DB" "
SELECT i.title, i.knowledge, i.pattern, i.architecture,
       substr(i.created_at,1,10) as date
FROM items i
WHERE i.item_type='observation'
  AND (i.knowledge IS NOT NULL OR i.pattern IS NOT NULL)
  AND i.created_at >= date('now','-90 days')
ORDER BY i.created_at DESC
LIMIT 100
" > /tmp/cp_data_4.txt

# Query 5: Friction and collaboration mode
sqlite3 "$DB" "
SELECT i.collaboration_mode, i.friction, i.tool,
       substr(i.created_at,1,10) as date, i.title
FROM items i
WHERE i.item_type='observation'
  AND i.created_at >= date('now','-90 days')
ORDER BY i.created_at DESC
LIMIT 150
" > /tmp/cp_data_5.txt

# Query 6: Project distribution
sqlite3 "$DB" "
SELECT d.url, count(*) as obs_count,
       min(substr(i.created_at,1,10)) as first_seen,
       max(substr(i.created_at,1,10)) as last_seen
FROM items i
JOIN documents d ON i.document_id = d.id
WHERE i.item_type='observation'
  AND i.created_at >= date('now','-90 days')
GROUP BY d.url
ORDER BY obs_count DESC
LIMIT 50
" > /tmp/cp_data_6.txt

# Query 7: Cognitive level distribution over time
sqlite3 "$DB" "
SELECT substr(i.created_at,1,7) as month,
       i.cognitive_level,
       count(*) as cnt
FROM items i
WHERE i.item_type='observation'
  AND i.created_at >= date('now','-180 days')
GROUP BY month, i.cognitive_level
ORDER BY month, i.cognitive_level
" > /tmp/cp_data_7.txt

# Query 8: Recent session insights documents
sqlite3 "$DB" "
SELECT d.title, d.raw_content, substr(d.created_at,1,10) as date
FROM documents d
WHERE d.source IN ('session-insights-v2','mirror-weekly','mirror-profile')
  AND d.created_at >= date('now','-30 days')
ORDER BY d.created_at DESC
LIMIT 5
" > /tmp/cp_data_8.txt
```

Also run `mirror score` and capture output to `/tmp/cp_mirror_score.txt`.

Verify all 8 data files are non-empty. If any are empty, log a warning but continue.

---

## Stage 2: Dispatch 4 Parallel Sub-agents

Use the Task tool to dispatch 4 sub-agents in parallel. Each agent receives:
- The path to its prompt template (in this repo under `skills/cognitive-portrait/prompts/`)
- Its explicit file ownership declaration
- The paths to all 8 data files (read-only)
- The version number N

**File ownership declarations (W-14 — no agent may write another agent's file):**

```
Agent L1 owns: /tmp/cp_v{N}_l1.md ONLY — do not create or modify any other file
Agent L2 owns: /tmp/cp_v{N}_l2.md ONLY — do not create or modify any other file
Agent L3 owns: /tmp/cp_v{N}_l3.md ONLY — do not create or modify any other file
Agent L4 owns: /tmp/cp_v{N}_l4.md ONLY — do not create or modify any other file
```

Load each prompt template from `skills/cognitive-portrait/prompts/l{N}_*.md` and pass it verbatim to the corresponding sub-agent, appending the data file paths and the file ownership declaration.

---

## Stage 3: Merge and Write Final Report

After all 4 sub-agents complete:

### 3.1 Acceptance Checks

```bash
for layer in 1 2 3 4; do
  file="/tmp/cp_v${N}_l${layer}.md"
  lines=$(wc -l < "$file" 2>/dev/null || echo 0)
  if [ "$layer" -le 3 ] && [ "$lines" -lt 250 ]; then
    echo "WARNING: L${layer} has ${lines} lines (minimum 250)"
  fi
  if [ "$layer" -eq 4 ] && [ "$lines" -lt 280 ]; then
    echo "WARNING: L4 has ${lines} lines (minimum 280)"
  fi
done
```

If any layer fails the line count check, log the warning but continue with the merge (do not abort).

### 3.2 Style Unification Pass

Before merging, scan all 4 layer files and ensure:
- All confidence labels use format: `[推断，置信度：高/中/低]`
- All fact labels use: `[事实]`
- All suggestion labels use: `[建议]`
- Tables use consistent `|---|` separator style

### 3.3 Build Header

```markdown
---
date: {YYYY-MM-DD}
version: v3
sessions_analyzed: {count from data file 1}
total_lines: {will fill after merge}
---

# 认知画像 v3 — {YYYY-MM-DD}

> 生成架构：Dispatcher + 4 并行 Sub-agent（L1/L2/L3/L4）
> 数据范围：最近 90 天观测 + 180 天认知等级时序
> 版本对比基准：2026-03-21 v0（978 行）

## 总体判断

[Dispatcher 根据 mirror score 和数据文件写 2-3 段总体判断，包含三分离标签]
```

### 3.4 Concatenate and Write

Concatenate: header + L1 content + L2 content + L3 content + L4 content

Write to: `docs/cognitive-portraits/cognitive-portrait-{date}-v3.md`

### 3.5 Update INDEX.md

Append a row to `docs/cognitive-portraits/INDEX.md`:

```
| [{date}](./cognitive-portrait-{date}-v3.md) | v3 | {sessions_count} | {total_lines} | {L1_lines} | {L2_lines} | {L3_lines} | {L4_lines} | {generation_mode} | {pass/fail} |
```

---

## Preflight Checks

Before Stage 1, verify:
1. `refine.db` exists and is readable
2. `mirror` binary is available (`which mirror`)
3. `docs/cognitive-portraits/` directory exists
4. If `~/.claude/skills/cognitive-portrait` is a symlink pointing to this repo's `skills/cognitive-portrait/` — log the path for traceability

If preflight fails, report the specific failure and stop.
