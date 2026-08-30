# L1 section guide — 认知演进

In the single-agent run, use this guide for the L1 section of
`REFINE_COGNITIVE_PORTRAIT_OUTPUT`. Read only the JSON bundle at
`REFINE_COGNITIVE_PORTRAIT_BUNDLE` and the optional previous portrait. Do not
query SQLite, create an intermediate layer file, or edit `INDEX.md`.

Write `## L1：认知演进` with these subsections:

- `### 1.1 当前认知结构`
- `### 1.2 决策与知识形成`
- `### 1.3 当前与前一等长窗口`
- `### 1.4 证据缺口和认知瓶颈`

Use the bundle's observations and claim catalog. For every numeric fact or
trend, copy the exact `claim_catalog.claims[].rendered_line` that matches the
claim ID; never write, recalculate, round, or paraphrase a number or trend. The
2026-03-21 portrait may appear once as an explicitly optional long-term anchor,
never as required padding. A `DEGRADED` bundle is not analyzable: stop without
writing a layer. If `comparison.comparable=false`, no trend, arrow,
increase/decrease, or directional claim is allowed.

Every `[事实]`, including a non-numeric evidence fact, must be copied from the
catalog byte-for-byte. Write interpretations as `[推断，置信度：高/中/低]` and
end them with a valid `[evidence:obs:<id>]` or `[bundle:/json/pointer]`. Numeric
facts, evidence facts, and trends must be copied catalog lines only, including
`[claim:<claim_id>]`; unknown or duplicate IDs and edits to `rendered_line`
fail closed. A catalog line in code, a quote, or HTML is invisible. Every
`[建议]` must also include `[owner:...] [due:YYYY-MM-DD]` and one typed check,
such as `[verify:metric|/comparison/status|eq|"OK"]`,
`[verify:artifact|portrait-review|present]`, or
`[verify:check|weekly-reflection|pass]`. Prefer
specific new conclusions over text repeated from the prior portrait. There is
no line-count target.

Each catalog line must be copied byte-for-byte, with its catalog-defined label,
unit, window, pointers, and values unchanged. Comparable trends use the
catalog's canonical trend line and `[趋势]`; deadlines must be within 90 days
after cutoff and verification must be a machine-checkable typed metric,
artifact, or check condition.
