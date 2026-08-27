# SPEC: Portfolio-aware cognitive advice

## Problem

Mirror currently evaluates breadth one window at a time. A low rolling-90-day
exploration rate can therefore produce an expansion recommendation even when
the rolling-7-day exploration rate is already high and one-off project share is
non-green. The LLM advice path and the deterministic weekly action card also
apply separate policies.

## Policy

Every portfolio recommendation consumes two `ScoreResult` values computed from
the same linked-interactive cohort contract:

- long term: rolling 90 days by event time;
- recent: rolling 7 days by event time.

The policy is ordered by guard strength, not indicator declaration order:

1. If either window has non-green `fragmentation`, choose consolidation. The
   output must contain explicit promote, hold, and stop decisions with a
   verifiable next action. It must not recommend a new project, a new direction,
   or another one-off experiment.
2. Exploration is allowed only when both fragmentation signals are green and
   both exploration signals are non-green.
3. Otherwise, preserve the active portfolio and deepen or validate existing
   work. Recent healthy exploration overrides low long-term exploration.

The regression fixture `90d exploration=14.4%, 7d exploration=29.2%, one-off
share=35-48%` must select consolidation, never expansion.

The deterministic action card and the LLM acknowledgement prompt use the same
computed policy. The LLM returns only a structured policy acknowledgement;
user-visible short and full advice are always rendered deterministically by the
service. LLM free text is never an authorization or output boundary.

The action card uses the same named-project denominator as fragmentation:
`other` and zero-session entries are not eligible candidates. When no named
candidate exists, the action card is omitted without aborting the weekly report;
the signal table and data-quality evidence remain visible. With one named
candidate, that project can be promoted but cannot also be stopped; with two or
more, the highest-volume candidate is promoted and the lowest-volume candidate
is stopped. Candidate evidence comes from the window that triggered
consolidation: a non-green rolling-90-day fragmentation signal selects the
rolling-90-day cluster, even when the recent window is healthy; otherwise a
recent-only trigger selects the rolling-7-day cluster.

An action card is emitted when exploration, mature-project share
(`deep_invest`), or fragmentation is non-green in either window. A non-green
`deep_invest` signal with green exploration and fragmentation selects the
Deepen policy. Session-count fallback evidence names its actual candidate
window: “past 90 days” for the long-term cohort and “past 7 days” for the recent
cohort.

Before any optional LLM call, `mirror score` writes the current deterministic
policy to cache. Cache revision v5 binds policy, current score timestamp,
rolling-90-day cohort identity, and rolling-7-day cohort identity. If the
portfolio cohort cannot be computed, the previous cache is invalidated.
Statusline and MOTD only read a cache whose score timestamp exactly matches the
current score, so an older Explore recommendation cannot survive a failed or
disabled LLM call.

## Profile context contract

Profile context is stored as a versioned JSON envelope containing:

- `generated_at`;
- `window`;
- `schema_version`;
- `source_revision`;
- `cohort_identity`;
- the summary text.

`mirror profile` reads the same rolling-90-day event-time window used by score
advice, and writes the resulting cluster identity into this envelope. It does
not write an all-history identity that the advice loader can never match.

Advice injects it only when the schema and source revision are supported, the
generation timestamp is less than 14 days old, and `cohort_identity` is a
strict `sha256:<64hex>` value exactly matching the expected rolling-90-day
advice cohort. The envelope records the relationship as
`exact-source-snapshot`; an injected prompt records it as `exact-match`. Legacy
text, malformed JSON, weak identities, future timestamps, unsupported
revisions, stale summaries, and different cohorts are never injected. Read or
validation errors other than a missing file are returned clearly.

## Files

- `apps/mirror/src/advice.rs` and `apps/mirror/src/advice/`: shared policy,
  deterministic rendering, score-bound cache, structured LLM acknowledgement,
  and profile-context validation.
- `apps/mirror/src/score.rs`: compute and pass the rolling-7-day score alongside
  the rolling-90-day score.
- `apps/mirror/src/weekly.rs`: compute and pass a rolling-90-day score alongside
  the rolling-7-day weekly score.
- `apps/mirror/src/weekly/action_card.rs`: render the shared policy using actual
  project evidence.
- `apps/mirror/src/profile.rs`: write the versioned profile-context envelope.

## Verification

- combination-matrix tests for fragmentation, exploration, and recent-window
  overrides;
- fixed 14.4/29.2/35-48 regression;
- stale, legacy, malformed, weak-identity, different-cohort, and fresh exact
  profile-context tests;
- structured-LLM bypass, negation, and short-output tests;
- cache revision, score timestamp, policy, and cohort binding tests;
- action-card 0/1/2 named-project boundary tests;
- synthetic-`other` weekly integration test proving safe card omission;
- long-term fragmentation with an old one-off and two healthy recent projects;
- deep-invest-only Deepen-card and window-accurate fallback evidence tests;
- rolling-90-day profile writer-to-advice-loader round trip;
- `cargo test -p mirror` and formatting checks.
