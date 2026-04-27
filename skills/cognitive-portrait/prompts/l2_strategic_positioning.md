# L2 Sub-agent: 战略定位分析

## File Ownership (W-14)

**You own ONLY: `/tmp/cp_v{N}_l2.md`**
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
6. **范围隔离**: Write ONLY L2 content (战略定位). Do NOT write L1 (认知演进), L3 (工作方式), or L4 (成长处方) content.

## Output Path

Write your complete analysis to: `/tmp/cp_v{N}_l2.md`

## Section Outline

Your output must follow this exact structure (H2 + H3 hierarchy):

```
## L2：战略定位

### 2.1 项目投入分布与战略对齐

[Table from data_6.txt: project × obs_count × first_seen × last_seen × strategic_signal]
[Analysis: which projects are getting disproportionate vs insufficient attention]
[Comparison with 2026-03-21 project distribution]

### 2.2 技术栈演化路径

[From data_1.txt + data_4.txt: what tools and technologies appear most frequently]
[Table: technology × frequency × trend (growing/stable/declining)]
[Inference about tech bet concentration risk]

### 2.3 AI 协作模式战略评估

[From data_5.txt: collaboration_mode distribution]
[Table: collaboration_mode × count × percentage × strategic_implication]
[Are you leveraging AI at the right level for strategic work?]

### 2.4 战略盲区与机会窗口

[What is missing from the data? Which domains are under-invested?]
[Table: opportunity × evidence_for × evidence_against × confidence × suggested_action]
[Suggestions with prerequisite assumptions and alternatives]
```

## Style Reference

Minimum density targets:
- Section 2.1: ≥ 65 lines (project analysis + tables + comparison)
- Section 2.2: ≥ 60 lines (tech stack analysis + trend table)
- Section 2.3: ≥ 55 lines (collaboration mode analysis)
- Section 2.4: ≥ 70 lines (opportunity/gap analysis + suggestions)

Every paragraph must end with a labeled claim or labeled table row.
Do not use vague phrases like "it appears that" — use explicit confidence labels instead.
