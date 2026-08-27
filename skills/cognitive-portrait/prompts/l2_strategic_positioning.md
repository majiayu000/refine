# L2 Agent — 战略定位

You own only `layer-l2.md` beside `REFINE_COGNITIVE_PORTRAIT_OUTPUT`. Read only the deterministic bundle and optional
previous portrait. Do not query SQLite or modify any repository file.

Write `## L2：战略定位` with:

- `### 2.1 项目组合与投入集中度`
- `### 2.2 可晋升、维持、退出的工作项`
- `### 2.3 来源覆盖与战略盲区`
- `### 2.4 当前证据支持的机会边界`

Ground project claims in `current.metrics.project_ranking` and observation IDs.
Disclose Claude, Codex, platform-unknown, and unsupported-source freshness from
the manifest. Grok/Gemini knowledge-only material is context coverage, never a
session count. Do not infer absent work from an absent source.

If `comparison.comparable=false`, suppress all cross-window trends. Every fact
and inference must cite `[evidence:obs:<id>]` or a valid
`[bundle:/json/pointer]`. Numeric claims use only a structured
`[metric:/allowed/numeric/pointer=<canonical JSON number>]` field. Every
recommendation must include evidence plus `[owner:...] [due:YYYY-MM-DD]` and a
structured `[verify:metric:/pointer==target]` or artifact/check equivalent.
Avoid paragraphs repeated from the
previous portrait. There is no line-count target.

Each metric field must equal its cited scalar. A comparable trend must use
`[趋势]` and cite current and previous metric fields. A degraded layer must
instead state `[事实][趋势抑制] ... [bundle:/comparison/status]`. Due dates must
be within 90 days after cutoff and verification must be machine-checkable.
