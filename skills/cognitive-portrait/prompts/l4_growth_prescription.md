# L4 Agent — 成长处方

You own only `layer-l4.md` beside `REFINE_COGNITIVE_PORTRAIT_OUTPUT`. Read only the deterministic bundle and optional
previous portrait. Do not query SQLite or edit the final archive.

Write `## L4：成长处方` with:

- `### 4.1 晋升、维持、退出清单`
- `### 4.2 决策条件`
- `### 4.3 未来 90 天执行窗口`
- `### 4.4 验证与复盘`

Each prescription is one `[建议]` line containing a valid observation or bundle
reference and all three machine fields:

`[owner:<person>] [due:YYYY-MM-DD] [verify:metric:/pointer==target]`

The due date must be within 90 days after the bundle cutoff date. Verification
must use a metric comparator, `artifact:name==present`, or `check:name==pass`.

Do not recommend expanding project breadth when the evidence supports
fragmentation or one-off work. If comparison is degraded, do not claim a trend;
frame the action as a current-window hypothesis with a verification condition.
The 2026-03-21 portrait is optional context, not a mandatory comparison.

All facts require valid evidence references. Numeric claims use only
`[metric:/allowed/numeric/pointer=<canonical JSON number>]`, with no free-prose
numbers. Avoid
repeating prior prescriptions without new evidence or a changed condition.
There is no line-count target.

Every metric field must equal its cited bundle scalar. Comparable trends use
`[趋势]` with current and previous metric fields. Degraded data instead
requires `[事实][趋势抑制] ... [bundle:/comparison/status]` and no trend claim.
