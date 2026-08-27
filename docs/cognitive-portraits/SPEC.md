# Cognitive Portrait v4 — deterministic evidence contract

Status: implemented specification

Date: 2026-08-28

Issue: #191

## Problem

The v3 skill queried SQLite with columns that do not exist in the live schema,
used ingest time as recent activity, improvised missing queries at runtime, and
treated prose length as a quality signal. It also repeated the 2026-03-21 anchor
in every layer. Those properties made a large report possible but not
reproducible or trustworthy.

## v4 data flow

```text
one SQLite read snapshot at fixed cutoff
  -> current rolling 90d + previous 90d by Document.captured_at
  -> Session Insights source allowlist + strict eligible cohort
  -> versioned deterministic JSON evidence bundle
  -> four read-only analysis layers
  -> merged v4 candidate
  -> deterministic evidence/number/comparison/novelty/action gate
  -> archive report + bundle + quality report + INDEX row
```

No collector or validator step invokes an LLM. The four analysis layers are the
only generative step.

## Bundle contract

The collector is `cognitive-portrait-collector-v1`; bundle schema is `1`.
Every bundle records:

- fixed cutoff and current/previous event-time boundaries;
- the same `InsightsManifest` builder, cohort identity, source allowlist, source
  freshness, unknown-platform counts, binary identity, and source revision used
  by Session Insights;
- current/previous project, decision, bugfix, summary, cognitive-level,
  collaboration-mode, tool, knowledge, pattern, architecture, and friction data
  from the exact eligible cohort;
- stable `obs:<item-id>` evidence IDs with document event time and source;
- a comparison status and explicit reasons when trends are not valid.

`claude-code-session`, `codex-session`, and `remem-raw-session` are supported
session containers. Remem remains `platform_unknown` until upstream provenance
is trustworthy. Grok/Gemini knowledge-only or unknown sources are disclosed as
unsupported and never counted as sessions.

An unsupported source, a detached observation, or an empty previous eligible
window makes comparison `DEGRADED`. An empty current eligible cohort is
`NO_CORE_DATA`. Missing or incompatible tables/columns are `SCHEMA_INVALID`.

## Claim syntax

Every factual claim binds to one or more of:

```text
[evidence:obs:<item-id>]
[bundle:/valid/json/pointer]
[metric:/allowed/numeric/json/pointer=<canonical JSON number>]
```

Numeric facts and inferences use only the structured `metric` form. Numeric
tokens in free factual prose (including scientific notation, percent,
thousands separators, full-width digits, and Chinese numerals) fail closed.
Metadata/version pointers are not metric pointers. Every action carries:

```text
[owner:<person>] [due:YYYY-MM-DD] [verify:metric:/pointer<operator>target]
[verify:artifact:<name>==present|absent]
[verify:check:<name>==pass|fail]
```

When `comparison.comparable=false`, trend markers and current-to-previous
directional claims are forbidden and the report must include an explicit
`[事实][趋势抑制]` claim bound to `/comparison/status`. A comparable trend must
use `[趋势]` and bind both current and previous structured metric fields. A
pointer that merely exists does not support an unrelated number. A degraded
report may describe the current window and the evidence gap only.

## Quality gate

The gate replaces all raw line-count requirements. A candidate passes only when:

- factual traceability is 100%;
- unsupported-number rate is 0%;
- trends are absent when the cohort is not comparable;
- paragraph novelty is at least 60% relative to the previous portrait when one
  exists;
- every recommendation has allowlisted evidence, a meaningful owner, a deadline
  within 90 days of the bundle cutoff, and a structured verification target.

The four exact L1-L4 main headings are mandatory and ordered. Deadlines must be
valid bounded ISO dates. Fenced/indented code, frontmatter, HTML comments, link
destinations, HTML metadata, and machine fields are excluded from rendered
structure and novelty checks, so metadata-only edits do not count as insight.

A failed gate returns non-zero. One kernel-backed lock owns a run. The scheduled
wrapper gives the untrusted agent a unique writable staging directory and an
untrusted bundle copy; the trusted bundle, validator, skill, archive, history,
and index remain outside that writable root and are hash checked. The agent
writes only `candidate.md`. The host validates and atomically publishes the
fixed v4 report name, evidence, and index row; failed candidates never enter the
archive or throttle scan. Existing names, symlinks, and hard links fail closed.

```text
cognitive-portrait-YYYY-MM-DD-v4.md
evidence/cognitive-portrait-YYYY-MM-DD-v4.bundle.json
evidence/cognitive-portrait-YYYY-MM-DD-v4.quality.json
```

## Long-term anchor

The 2026-03-21 portrait is retained as an optional long-horizon reference. It is
not an equivalent-window comparator, not part of the bundle, and not mandatory
in any layer. Default comparison is always the immediately previous 90-day
window from the same cutoff snapshot.

## Verification fixtures

The collector fixture uses a temporary SQLite database initialized with the
real Refine schema. It covers:

- document event time taking precedence over recent item ingest time;
- current and previous 90-day boundaries;
- Claude, Codex, and remem/unknown source reporting;
- Grok/Gemini knowledge-only exclusion from session counts;
- unsupported Observation disclosure and trend suppression;
- empty and schema-invalid failure;
- validator failure preventing index eligibility.

Historical v0-v3 reports are immutable evidence of the former process. Their
line counts remain historical metadata, not a v4 acceptance signal.
