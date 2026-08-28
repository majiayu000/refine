# Cognitive Portrait v4 — deterministic bounded evidence contract

Status: implemented specification

Date: 2026-08-28

Issues: #191, #199

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

The collector is `cognitive-portrait-collector-v2`; bundle schema is `2`.
Every bundle records:

- fixed cutoff and current/previous event-time boundaries;
- the same `InsightsManifest` builder, cohort identity, source allowlist, source
  freshness, unknown-platform counts, binary identity, and source revision used
  by Session Insights;
- exact full-cohort scalar metrics plus bounded project/tool breakdowns with
  total/selected/omitted counts and full-breakdown digests;
- current/previous project, decision, bugfix, knowledge, pattern, architecture,
  and friction projections selected deterministically from the exact eligible
  cohort;
- stable `obs:<item-id>` provenance anchors with document event time, source,
  direct item-to-project assignment, categories, bounded display text, and
  original field byte lengths/digests;
- a comparison status and explicit reasons when trends are not valid.
- a closed `claim_catalog` with a schema version, stable claim IDs, typed
  metric/pointer metadata, and canonical `rendered_line` text for every
  numeric fact and comparable trend. Claims are sorted by `claim_id`; escaping
  and rendered text are deterministic. Stable opaque claims cover only retained
  provenance anchors; omitted observations never manufacture a usable claim.

### Bounded projection

Every window includes `evidence_selection` using policy
`stratified-provenance-v1`. Full-cohort metrics, source counts/freshness,
comparison status, and cohort identity are computed before projection and are
never sampled. The selection manifest records:

- eligible, retained, and omitted observation counts;
- a fixed per-window evidence component budget and the global 16 MiB internal
  bundle target;
- the full eligible payload digest and retained selection digest;
- deterministic strata counts across source, primary category, and the top 32
  full-cohort projects plus an explicit other-project bucket.

Within each sorted stratum, observations rank by event time descending and
evidence ID ascending. Selection round-robins across sorted strata up to 2048
anchors per window. It is therefore neither SQL first-N nor recency-only.

Each qualitative dimension discloses exact total/selected/omitted occurrences,
the retained value and evidence-reference counts, and an order-independent
digest of all full-cohort occurrences and references. At most 128 deterministic
min-hash values per dimension and four retained evidence IDs per value are
emitted. Display values and anchor display text are UTF-8 safely bounded to 512
bytes while their original byte length and SHA-256 remain available.

Unsupported sources use the same bounded disclosure: exact observation and
session totals, freshest event time, selected and omitted observation counts,
an order-independent full digest, and at most 128 min-hash source entries.
Their raw high-cardinality names are never expanded without a bound.

Project ranking and tool frequency retain exact full-entry counts, full digests,
and reproducible retained-selection digests but emit at most 128 ranked entries.
Fixed scalar totals, cognitive levels, and collaboration modes remain exact.

The builder counts evidence, dimensions, claim-catalog, and final JSON bytes
through streaming writers before allocating the bounded final buffer. Any
implementation invariant exceeding its component
budget is `INTERNAL_BUDGET_VIOLATION`; it is never handled by silently dropping
additional data at write time. The final pretty JSON must be at most 16 MiB.
The independent 64 MiB reader/wrapper safety limit remains unchanged.

The projection is internally closed: every dimension reference resolves to one
retained anchor; every retained anchor has a unique claim and pointer; selection
and component digests reproduce; eligible equals retained plus omitted; and all
arrays use canonical stable ordering. Unknown, duplicate, or omitted references
fail bundle validation.

`claude-code-session`, `codex-session`, and `remem-raw-session` are supported
session containers. Remem remains `platform_unknown` until upstream provenance
is trustworthy. Grok/Gemini knowledge-only or unknown sources are disclosed as
unsupported and never counted as sessions.

An unsupported source, a detached observation, or an empty previous eligible
window makes comparison `DEGRADED`. An empty current eligible cohort is
`NO_CORE_DATA`. Missing or incompatible tables/columns are `SCHEMA_INVALID`.

## Claim syntax

Every factual line, including a non-numeric evidence fact, is an exact unique
`claim_catalog.claims[].rendered_line`. The collector emits stable evidence
record claims as well as metric and trend claims. Interpretations and actions
bind to one or more of:

```text
[evidence:obs:<item-id>]
[bundle:/valid/json/pointer]
```

Numeric facts and trends do not have a free-form numeric syntax. The model must
copy the matching `claim_catalog.claims[].rendered_line` byte-for-byte,
including `[claim:<claim_id>]`. The catalog's label, unit, window, pointers,
and values are authoritative. It is forbidden to calculate, round, translate,
or paraphrase a catalog line. Unknown or duplicate claim IDs, a modified
rendered line, and reusing one claim ID as multiple facts fail closed.

Catalog lines inside fenced or indented code, block quotes, or HTML are not
visible claims. The validator parses the rendered CommonMark surface, so a
soft-wrapped paragraph is one paragraph.

Every `[推断]` requires at least one existing, allowlisted observation ID or
bundle pointer. Raw HTML and Markdown images are rejected. Link destinations
must be `http`, `https`, or a safe relative path; active schemes such as
`javascript`, `data`, and `file` fail closed.

Numeric tokens in free factual prose (including scientific notation, percent,
thousands separators, full-width digits, and Chinese numerals) fail closed.
Metadata/version pointers are not catalog claims. Every action carries:

```text
[owner:<person>] [due:YYYY-MM-DD] [verify:metric|/pointer|<comparator>|<typed-JSON-target>]
[verify:artifact|<name>|present]
[verify:check|<name>|pass]
```

Metric comparators are `eq`, `gt`, `gte`, `lt`, and `lte`; numeric targets
must be JSON numbers, while string and boolean targets support `eq` only.
Artifact states are `present` or `absent`; check states are `pass` or `fail`.
Pointers and names are allowlisted by the validator. Free-form verification
text is invalid.

When `comparison.comparable=false`, the host wrapper records the diagnostic
bundle and does not launch the agent, create a candidate, publish a report, or
update `INDEX.md`. Therefore no degraded portrait or trend-suppression prose
is generated. A comparable trend must use the catalog's canonical trend line
and `[趋势]`. A pointer that merely exists does not support an unrelated
number.

## Quality gate

The gate replaces all raw line-count requirements. A candidate passes only when:

- factual traceability is 100%;
- inference evidence traceability is 100% whenever inferences are present;
- unsupported-number rate is 0%;
- trends are absent when the cohort is not comparable;
- every numeric/trend line is an exact line from the closed claim catalog;
- paragraph novelty is at least 60% relative to the previous portrait when one
  exists;
- every recommendation has allowlisted evidence, a meaningful owner, a deadline
  within 90 days of the bundle cutoff, and a structured verification target.

The four exact L1-L4 main headings are mandatory and ordered. Deadlines must be
valid bounded ISO dates. Fenced/indented code, block quotes, frontmatter, HTML
comments, link destinations, HTML metadata, and machine fields are excluded
from rendered structure and novelty checks, so metadata-only edits do not
count as insight. Soft-wrapped paragraphs are reconstructed before validation.
Candidates are capped at 1 MiB, individual Markdown lines at 64 KiB, and the
rendered report at 4096 blocks. Projected bundles target 16 MiB and are capped
independently at 64 MiB; previous
portraits at 4 MiB. Both the host wrapper and Rust reader enforce the byte caps
before hashing, copying, or parsing.

A failed gate returns non-zero. One kernel-backed lock owns a run. The scheduled
wrapper gives the untrusted agent a unique writable staging directory and an
untrusted bundle copy; the trusted bundle, validator, skill, archive, history,
and index remain outside that writable root and are hash checked. Codex runs
ephemerally with user config/rules ignored, a minimal environment, and no
Mirror write grant. The agent
writes only `candidate.md`. The host validates and atomically publishes the
fixed v4 report name, evidence, and index row. A durable journal and INDEX
backup recover host crashes before throttle evaluation. Failed candidates never
enter the archive or throttle scan. Existing names, symlinks, and hard links
fail closed. Trusted run state lives under owner-only `~/.refine/`, not Mirror.

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
- at least 100,000 long-Unicode, high-cardinality observations producing a
  deterministic projected bundle below both internal and outer byte limits;
- repeated-process fixed-cutoff bundle SHA stability;
- bounded source/project/category strata, full-cohort digest/count disclosure,
  direct same-title cross-project provenance, and fail-closed reference
  invariants.

Historical v0-v3 reports are immutable evidence of the former process. Their
line counts remain historical metadata, not a v4 acceptance signal.
