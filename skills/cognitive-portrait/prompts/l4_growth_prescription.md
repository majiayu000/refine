# L4 section guide — 成长处方

In the single-agent run, use this guide for the L4 section of
`REFINE_COGNITIVE_PORTRAIT_OUTPUT`. Read only the deterministic bundle and
optional previous portrait. Do not query SQLite, create an intermediate layer
file, or edit the final archive.

Write `## L4：成长处方` with:

- `### 4.1 晋升、维持、退出清单`
- `### 4.2 决策条件`
- `### 4.3 未来 90 天执行窗口`
- `### 4.4 验证与复盘`

Each prescription is one `[建议]` line containing a valid observation or bundle
reference and all three machine fields:

`[owner:<person>] [due:YYYY-MM-DD] [verify:metric|/pointer|eq|<typed-JSON-target>]`

The due date must be within 90 days after the bundle cutoff date. Verification
must use a typed metric comparator, `[verify:artifact|<name>|present]` (or
`absent`), or `[verify:check|<name>|pass]` (or `fail`). For example,
`[verify:metric|/comparison/status|eq|"OK"]`.

Do not recommend expanding project breadth when the evidence supports
fragmentation or one-off work. A `DEGRADED` bundle is blocked by the host and
must not reach this prompt.
The 2026-03-21 portrait is optional context, not a mandatory comparison.

All facts, including non-numeric evidence facts, must be copied byte-for-byte
from the matching catalog line. Interpretations use `[推断]` plus valid evidence
references. Numeric facts and trends use the matching
`claim_catalog.claims[].rendered_line`, including `[claim:<claim_id>]`; never
self-write values, labels, units, windows, or trend prose. Unknown or duplicate
claim IDs, modified catalog lines, and catalog lines inside code, quotes, or
HTML are invalid. Avoid
repeating prior prescriptions without new evidence or a changed condition.
There is no line-count target.

Every catalog line must remain an exact copy of its rendered line. Comparable
trends use `[趋势]` with the canonical catalog trend line. A `DEGRADED` bundle
is blocked by the host and must not produce a layer or prescription. Typed
verification targets must use the allowed metric pointer or artifact/check
enum forms.
