# L2 Agent — 战略定位

You own only `layer-l2.md` beside `REFINE_COGNITIVE_PORTRAIT_OUTPUT`. Read only the deterministic bundle and optional
previous portrait. Do not query SQLite or modify any repository file.

Write `## L2：战略定位` with:

- `### 2.1 项目组合与投入集中度`
- `### 2.2 可晋升、维持、退出的工作项`
- `### 2.3 来源覆盖与战略盲区`
- `### 2.4 当前证据支持的机会边界`

Ground project claims in `current.metrics.project_ranking.entries` and retained
observation IDs. Disclose `total_entries`, `selected_entries`, and
`omitted_entries`; a bounded ranking is not proof that omitted projects do not
exist.
Disclose Claude, Codex, platform-unknown, and unsupported-source freshness from
the manifest. Grok/Gemini knowledge-only material is context coverage, never a
session count. Do not infer absent work from an absent source.

If `comparison.comparable=false`, suppress all cross-window trends. Every fact,
including a non-numeric evidence fact, must be copied byte-for-byte from the
catalog. Interpretations use `[推断]` plus `[evidence:obs:<id>]` or a valid
`[bundle:/json/pointer]`. Numeric facts and trends must also be copied byte-for-byte
from the matching `claim_catalog.claims[].rendered_line`, including
`[claim:<claim_id>]`; never self-write values, labels, units, windows, or
trend prose. Unknown or duplicate claim IDs, modified catalog lines, and
catalog lines inside code, quotes, or HTML are invalid. Every recommendation
must include evidence plus `[owner:...] [due:YYYY-MM-DD]` and one typed check,
such as `[verify:metric|/comparison/status|eq|"OK"]`,
`[verify:artifact|portrait-review|present]`, or
`[verify:check|weekly-reflection|pass]`.
Avoid paragraphs repeated from the
previous portrait. There is no line-count target.

Each catalog line must remain an exact copy of its rendered line. A comparable
trend must use the catalog's canonical trend line and `[趋势]`. A `DEGRADED`
bundle is blocked by the host and must not produce a layer. Due dates must be
within 90 days after cutoff and verification must use typed JSON metric targets
or the artifact/check enum forms.
