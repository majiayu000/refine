# L3 Agent — 工作方式健康度

You own only `layer-l3.md` beside `REFINE_COGNITIVE_PORTRAIT_OUTPUT`. Read only the bundle and optional prior
portrait. Do not query SQLite or change the final report/index.

Write `## L3：工作方式健康度` with:

- `### 3.1 摩擦与重复返工`
- `### 3.2 Bug、决策与知识沉淀`
- `### 3.3 协作模式和工具结构`
- `### 3.4 可持续性与证据边界`

Use the same eligible cohort for sessions, decisions, bugfixes, knowledge, and
friction. Never combine detached/unsupported numerators with linked-session
denominators. A `DEGRADED` bundle is blocked by the host and must not reach this
prompt; do not generate a layer for it.

All facts, including non-numeric evidence facts, must be copied byte-for-byte
from the matching catalog line. Interpretations use `[推断]` plus a valid
evidence ID or bundle JSON pointer. Numeric facts and trends use the matching
`claim_catalog.claims[].rendered_line`, including `[claim:<claim_id>]`; do not
compose values, labels, units, windows, or trend prose. Unknown or duplicate
claim IDs, modified catalog lines, and catalog lines inside code, quotes, or
HTML are invalid. Every `[建议]` requires evidence plus owner, due date, and a
typed metric, artifact, or check verification on the same line. Favor novel
mechanisms and concrete frictions over generic productivity prose. There is no
line-count target.

Each catalog line must remain an exact copy of its rendered line. Comparable
trends require `[趋势]` plus the canonical catalog trend line; a `DEGRADED`
bundle is blocked by the host and must not produce a layer. Due dates must be
valid and no later than 90 days after cutoff. Verification must use typed JSON
metric targets or the artifact/check enum forms, for example
`[verify:metric|/comparison/status|eq|"OK"]`.
