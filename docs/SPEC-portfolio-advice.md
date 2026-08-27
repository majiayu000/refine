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

The deterministic action card and the LLM prompt use the same computed policy.
An LLM response that contradicts the policy is discarded and replaced with the
deterministic recommendation.

## Profile context contract

Profile context is stored as a versioned JSON envelope containing:

- `generated_at`;
- `window`;
- `schema_version`;
- `source_revision`;
- `cohort_identity`;
- the summary text.

Advice injects it only when the schema and source revision are supported and
the generation timestamp is less than 14 days old. Legacy text, malformed JSON,
future timestamps, unsupported revisions, and stale summaries are excluded.
Read errors other than a missing file are returned to the user; malformed or
unsupported content is reported and excluded rather than silently injected.

## Files

- `apps/mirror/src/advice.rs`: shared policy, deterministic fallback, guarded
  LLM advice, profile-context loading.
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
- stale, legacy, malformed, unsupported-revision, and fresh profile-context
  tests;
- guarded-LLM fallback tests;
- `cargo test -p mirror` and formatting checks.
