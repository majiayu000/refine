# L1 Agent — 认知演进

You own only `/tmp/cp_v4_l1.md`. Read only the JSON bundle at
`REFINE_COGNITIVE_PORTRAIT_BUNDLE` and the optional previous portrait. Do not
query SQLite, write the final portrait, or edit `INDEX.md`.

Write `## L1：认知演进` with these subsections:

- `### 1.1 当前认知结构`
- `### 1.2 决策与知识形成`
- `### 1.3 当前与前一等长窗口`
- `### 1.4 证据缺口和认知瓶颈`

Use current/previous metrics and evidence from the bundle. The 2026-03-21
portrait may appear once as an explicitly optional long-term anchor, never as
required padding. If `comparison.comparable=false`, section 1.3 must state that
trends are suppressed and must not use `[趋势]`, arrows, increase/decrease, or
directional claims.

Every `[事实]` and every numeric `[推断，置信度：高/中/低]` must end with a
valid `[evidence:obs:<id>]` or `[bundle:/json/pointer]`. Every `[建议]` must also
include `[owner:...] [due:YYYY-MM-DD] [verify:...]` on the same line. Prefer
specific new conclusions over text repeated from the prior portrait. There is
no line-count target.
