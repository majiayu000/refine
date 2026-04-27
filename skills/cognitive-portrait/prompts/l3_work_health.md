# L3 Sub-agent: 工作方式健康度分析

## File Ownership (W-14)

**You own ONLY: `/tmp/cp_v{N}_l3.md`**
Do NOT create, read, or modify any other file except the 8 read-only data files listed below.

## Data Sources (read-only)

- `/tmp/cp_data_1.txt` — Recent observations overview (last 90 days)
- `/tmp/cp_data_2.txt` — Decision patterns
- `/tmp/cp_data_3.txt` — Bug fix patterns
- `/tmp/cp_data_4.txt` — Knowledge and patterns acquired
- `/tmp/cp_data_5.txt` — Friction and collaboration mode
- `/tmp/cp_data_6.txt` — Project distribution
- `/tmp/cp_data_7.txt` — Cognitive level distribution over time
- `/tmp/cp_data_8.txt` — Recent session insights documents

## Mandatory Constraints

1. **三分离强制**: Every claim must carry one of these labels:
   - `[事实]` — directly traceable to a data file line
   - `[推断，置信度：高/中/低]` — logical inference from facts
   - `[建议]` — action recommendation with prerequisite assumption stated
2. **精确数值**: All numbers in `N/M(百分比%)` format where applicable
3. **矩阵化优先**: Use tables over bullet lists when 3+ items can be compared
4. **行数硬下限**: This file MUST be ≥ 250 lines when complete
5. **与 03-21 对比**: Include at least one comparison table referencing 2026-03-21 baseline data
6. **范围隔离**: Write ONLY L3 content (工作方式健康度). Do NOT write L1 (认知演进), L2 (战略定位), or L4 (成长处方) content.

## Output Path

Write your complete analysis to: `/tmp/cp_v{N}_l3.md`

## Section Outline

Your output must follow this exact structure (H2 + H3 hierarchy):

```
## L3：工作方式健康度

### 3.1 摩擦密度与阻力分析

[From data_5.txt: friction field analysis]
[Table: friction_type × frequency × sessions_affected × severity]
[Comparison with 2026-03-21 friction baseline]
[Inferences about systemic vs accidental friction]

### 3.2 Bug/Decision 比率健康度

[From data_2.txt + data_3.txt: decision count vs bug count]
[Table: month × decision_count × bugfix_count × ratio × signal]
[Trend: is the ratio improving? What does it mean?]

### 3.3 工具使用模式与效率

[From data_1.txt + data_5.txt: tool field analysis]
[Table: tool × frequency × associated_friction × efficiency_signal]
[Are high-friction tools being overused?]

### 3.4 工作节奏与可持续性

[From data_6.txt + data_1.txt: session frequency, project switching]
[Table: week × session_count × projects_touched × focus_signal]
[Inferences about cognitive load and sustainability]
[Suggestions with prerequisite assumptions and risk/cost]
```

## Style Reference

Minimum density targets:
- Section 3.1: ≥ 65 lines (friction analysis + tables + comparison)
- Section 3.2: ≥ 55 lines (bug/decision ratio trend)
- Section 3.3: ≥ 60 lines (tool analysis + efficiency table)
- Section 3.4: ≥ 70 lines (rhythm analysis + sustainability suggestions)

Every paragraph must end with a labeled claim or labeled table row.
Avoid hedging without labels — if uncertain, use `[推断，置信度：低]` explicitly.
