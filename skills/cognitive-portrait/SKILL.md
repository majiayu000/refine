# Authoritative spec: docs/cognitive-portraits/SPEC.md

# Cognitive Portrait v4 — single-agent evidence analysis

Trigger keywords: 认知画像 / cognitive portrait / 认知分析 / 分析我的成长

This skill turns one deterministic, versioned Refine bundle into one four-part
portrait in a single agent context. It never invents SQL, never queries
`refine.db` directly, and never treats output length as evidence quality.

## Stage 0 — preflight

Require all of the following or stop with a specific error:

- `REFINE_COGNITIVE_PORTRAIT_BUNDLE` points to a readable JSON bundle.
- The bundle has `schema_version=2` and
  `collector_version=cognitive-portrait-collector-v2`.
- `claim_catalog.schema_version` is the supported catalog version. The catalog
  is the closed set of evidence, numeric, and trend facts for this run.
- `REFINE_COGNITIVE_PORTRAIT_OUTPUT` names the only final candidate file this
  invocation may write; its parent is the unique writable staging directory.
- All four prompt templates exist at the trusted skill path.
- `comparison.status` is read before any cross-period claim is planned.

`REFINE_COGNITIVE_PORTRAIT_PREVIOUS` is an optional read-only prior portrait
used only for novelty/repetition checks. The 2026-03-21 portrait is an optional
long-term anchor; do not pad every layer with it.

## Stage 1 — deterministic collection only

The scheduled wrapper collects the bundle before invoking this skill. For an
interactive run without that wrapper, run exactly:

```bash
bundle=$(mktemp "${TMPDIR:-/tmp}/refine-portrait-bundle.XXXXXX")
scripts/collect-cognitive-portrait.sh --period 90 --output "$bundle"
export REFINE_COGNITIVE_PORTRAIT_BUNDLE="$bundle"
```

Do not run ad-hoc `sqlite3`, do not use `items.created_at` as recent session
time, and do not relabel `platform_unknown` as Claude or Codex. The collector
already uses one SQLite read snapshot, event time, the Session Insights source
allowlist, and the strict eligible cohort for current rolling 90 days and the
previous 90 days.

`DEGRADED` is a host-level stop condition. The wrapper must not launch the
agent, create a candidate, publish a report, or update `INDEX.md` when the
collector reports `DEGRADED`; the collector output remains a diagnostic bundle,
while the scheduled archive stays unchanged.
If collection reports `NO_CORE_DATA` or `SCHEMA_INVALID`, stop with a specific
error as well.

## Stage 2 — one sequential analysis pass

Do not delegate, spawn subagents, or split the work into parallel tasks. In the
primary agent context, read the four trusted prompt templates in L1-L4 order and
use them as section guides. Each section uses:

- the same read-only bundle path;
- the optional previous portrait path;
- its matching prompt under `skills/cognitive-portrait/prompts/`;
- the same closed claim catalog and evidence boundaries.

Keep intermediate reasoning in the current context. Do not create layer files
or any other staging artifact. After all four sections are planned, write only
`REFINE_COGNITIVE_PORTRAIT_OUTPUT`. Do not write `INDEX.md`, the bundle, the
database, or any repository file.

### Closed claim catalog

The collector is the sole authority for numeric facts and trends. For every
numeric or trend statement, the model must copy the corresponding
`claim_catalog.claims[].rendered_line` byte-for-byte, including its
`[claim:<claim_id>]` marker. The model must not compose, paraphrase, round,
translate, or recalculate a catalog line. Its label, unit, window, pointers,
and values are already fixed by the catalog.

Unknown claim IDs, duplicate claim IDs, or a line that differs from its
catalog `rendered_line` fail closed. A claim ID cannot be used as multiple
facts. A canonical catalog line inside fenced/indented code, a block quote, or
HTML is not visible evidence and does not count. CommonMark paragraph text is
the rendered surface; soft-wrapped paragraphs are one paragraph.

Raw HTML, Markdown images, and `javascript:`, `data:`, `file:`, protocol-relative,
or other non-HTTP/non-relative link targets are forbidden in the candidate.
The candidate is capped at 1 MiB; a single Markdown line is capped at 64 KiB
and the rendered report at 4096 blocks. The trusted bundle is capped at 64 MiB
and the previous portrait at 4 MiB.

The bundle is a bounded projection of one exact full cohort. Scalar metrics,
source freshness, comparison status, and cohort identity remain full-cohort
facts. Qualitative dimensions and provenance anchors are selected by the
versioned deterministic policy recorded in each window. Read and disclose
`evidence_selection.eligible_observations`, `selected_observations`,
`omitted_observations`, and the dimension omission counts; never describe the
retained qualitative slice as exhaustive. Only retained `evidence[]` IDs and
their catalog claims may be cited. Do not infer facts about omitted rows from a
selection digest.

Each section may cite interpretations and recommendations with these
machine-readable forms:

- `[evidence:obs:<item-id>]` for an observation in the bundle;
- `[bundle:/json/pointer]` for a non-numeric aggregate or manifest field;

Every `[事实]` line, including a non-numeric evidence fact, must be an exact,
unique catalog `rendered_line`. The collector emits opaque evidence-record
claims for this purpose, so the model never invents a factual label. Free-prose
facts, numbers, and self-written trend lines fail closed. `[推断]` prose must
carry a valid evidence ID or bundle pointer. If `comparison.comparable=false`, no trend, direction,
increase/decrease, or current-versus-previous claim is allowed. Do not cite
knowledge-only Grok/Gemini sources as sessions.

Every `[建议]` must carry allowlisted evidence, a meaningful owner, a due date
no later than 90 days after the bundle cutoff, and one typed verification
condition. Valid examples are `[verify:metric|/comparison/status|eq|"OK"]`,
`[verify:artifact|portrait-review|present]`, and
`[verify:check|weekly-reflection|pass]`. Metric targets are typed JSON values;
artifact/check states are fixed enums. Free-form verification text is invalid.

## Stage 3 — write once; wrapper validates and archives

1. Confirm the planned report contains all four L1-L4 sections in order.
2. Write them once into `REFINE_COGNITIVE_PORTRAIT_OUTPUT` with a
   header that records bundle schema, collector version, cutoff, window, cohort
   status, source revision, and binary identity.
3. Do not use line count, table count, or prose volume as a pass condition.
4. Return zero only after the candidate is completely written. Do not run the
   validator, publish evidence, write a final archive name, or touch `INDEX.md`.

The scheduled wrapper owns the trusted bundle, validator, fixed report name,
kernel-backed lock, archive, and index. It hash-checks trusted inputs after the
agent exits, validates the staged candidate, rejects collisions/symlinks/hard
links, and publishes the report, evidence, and index transactionally. The agent
may not overwrite a prior artifact.

The validator requires complete factual traceability, zero unsupported numeric
claims, complete inference evidence traceability, canonical catalog usage,
trends absent when the cohort is not
comparable, at least 60% paragraph novelty when a previous portrait exists, and
verifiable actions with owner, deadline, and verification condition.

## Output contract

The final portrait contains exactly these main sections:

- `## L1：认知演进`
- `## L2：战略定位`
- `## L3：工作方式健康度`
- `## L4：成长处方`

Historical v0-v3 files remain immutable. New evidence artifacts are archived by
the wrapper as:

- `docs/cognitive-portraits/evidence/cognitive-portrait-{date}-v4.bundle.json`
- `docs/cognitive-portraits/evidence/cognitive-portrait-{date}-v4.quality.json`
