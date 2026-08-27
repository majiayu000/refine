# Authoritative spec: docs/cognitive-portraits/SPEC.md

# Cognitive Portrait v4 — evidence-bundle dispatcher

Trigger keywords: 认知画像 / cognitive portrait / 认知分析 / 分析我的成长

This skill turns one deterministic, versioned Refine bundle into four parallel
analysis layers. It never invents SQL, never queries `refine.db` directly, and
never treats output length as evidence quality.

## Stage 0 — preflight

Require all of the following or stop with a specific error:

- `REFINE_COGNITIVE_PORTRAIT_BUNDLE` points to a readable JSON bundle.
- The bundle has `schema_version=1` and
  `collector_version=cognitive-portrait-collector-v1`.
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

If collection reports `NO_CORE_DATA` or `SCHEMA_INVALID`, stop. If the bundle
reports `DEGRADED`, analysis may describe the evidence gap, but all trend,
direction, increase/decrease, and current-versus-previous claims are forbidden.
Include one `[事实][趋势抑制]` line bound to
`[bundle:/comparison/status]` so suppression is explicit.

## Stage 2 — four parallel layers

Dispatch four independent agents in parallel. Each receives:

- the same read-only bundle path;
- the optional previous portrait path;
- exactly one prompt under `skills/cognitive-portrait/prompts/`;
- exactly one owned layer file beside `REFINE_COGNITIVE_PORTRAIT_OUTPUT`, named
  `layer-l{1..4}.md`.

No layer may write another layer, the final report, `INDEX.md`, the bundle, or
the database. A layer must cite claims with one of these machine-readable forms:

- `[evidence:obs:<item-id>]` for an observation in the bundle;
- `[bundle:/json/pointer]` for a non-numeric aggregate or manifest field;
- `[metric:/allowed/numeric/pointer=<canonical JSON number>]` for a numeric
  claim. Keep numeric tokens out of the surrounding factual prose.

Every `[事实]` claim must have a valid reference. Numeric claims use only the
structured metric field; free-prose numbers fail closed. A comparable
cross-window statement must be tagged `[趋势]` and cite structured current and
previous metrics. Every `[建议]` must carry allowlisted evidence, a meaningful
owner, a due date no later than 90 days after the bundle cutoff, and one of
`[verify:metric:/pointer==target]`, `[verify:artifact:name==present]`, or
`[verify:check:name==pass]`. Do not cite knowledge-only Grok/Gemini sources as
sessions.

## Stage 3 — merge only; wrapper validates and archives

1. Confirm all four layer files exist and keep their L1-L4 boundaries.
2. Merge them only into `REFINE_COGNITIVE_PORTRAIT_OUTPUT` with a
   header that records bundle schema, collector version, cutoff, window, cohort
   status, source revision, and binary identity.
3. Do not use line count, table count, or prose volume as a pass condition.
4. Return zero only after the candidate is completely written. Do not run the
   validator, publish evidence, write a final archive name, or touch `INDEX.md`.

The scheduled wrapper owns the trusted bundle, validator, fixed report name,
kernel-backed lock, archive, and index. It hash-checks trusted inputs after the
agent exits, validates the staged candidate, rejects collisions/symlinks/hard
links, and publishes the report, evidence, and index transactionally. No agent
or layer may overwrite a prior artifact.

The validator requires complete factual traceability, zero unsupported numeric
claims, comparable cohorts for any trend claim, at least 60% paragraph novelty
when a previous portrait exists, and verifiable actions with owner, deadline,
and verification condition.

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
