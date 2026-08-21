# SPEC: Mirror Scoring and Codex Weekly Semantics

Date: 2026-08-12
Status: Accepted for issue #145

## Goal

Re-evaluate the remaining Mirror scoring experiment from PR #141 commit
`544bf8187256b85678d2a5d1fa726febf2b51278` on top of current `main`.
The implementation must keep the ingestion, data-safety, and scheduler fixes
that were already merged through PRs #142, #143, and #144.

## Current Main Comparison

The old PR mixed independent work streams. Current `main` already contains
these parts, so this issue must not replay them:

| Area from PR #141 | Current `main` state | Decision |
| --- | --- | --- |
| Codex transcript parser | Current `response_item.payload.type = "message"` schema, roles, text types, metadata, and timestamp extraction are already implemented in `packages/core/src/session/parser.rs`. | Do not edit parser transport. |
| Codex project attribution | `apps/cli/src/ingest_sessions.rs` already falls back from discovery project to `session.meta.project` before `facets_to_items`. | Do not replay ingest changes. |
| Event-time storage | Session documents have `captured_at`; session ingestion sets it from `SessionMeta.started_at` or file mtime. | Preserve. |
| Weekly event-time window | `mirror weekly` already uses `find_observations_by_event_range`. | Preserve. |
| Score/dashboard event-time window | `mirror score` and dashboard already use the same event-time query for default and `--since` windows. | Preserve. |
| Scheduled workflow hardening | PR #143 made daily/weekly automation locked, serialized, and failure-visible. | Do not edit scheduler scripts. |
| Cognitive portrait archive | PR #144 extracted generated reports and scheduling. | Do not add generated reports. |

The remaining portable work is therefore limited to scoring semantics,
statusline/advice visibility, weekly action cards, clustering input stability,
and this specification.

## Event-Time Window Semantics

Mirror windows are based on when the session happened, not when it was
ingested.

| Command | Default window | Override | Time source |
| --- | --- | --- | --- |
| `mirror score` | Rolling 90 days | `--since YYYY-MM-DD`, `--all` | `Document.captured_at`, falling back to `Item.created_at` for unlinked legacy observations |
| `mirror dashboard` | Rolling 90 days | `--since YYYY-MM-DD`, `--all` | Same event-time query |
| `mirror weekly` | Rolling 7 days vs prior 7 days | None | Same event-time query |

This avoids a historical Codex or Claude backfill appearing as current-week
work merely because the observations were inserted today.

## Scoring Semantics

Signals and trends are separate:

- `Signal` color answers: does the current score meet the fixed target?
- `Trend` arrow answers: is the current score better than the user's recent
  4-week average?
- Personal trends never rewrite indicator or layer signals.
- Band metrics, currently `deep_invest`, do not get personal trend arrows
  because "higher than average" is not necessarily better.

Mirror now scores three layers with eight live indicators:

| Layer | Indicator | Direction | Target source |
| --- | --- | --- | --- |
| Depth | `dreyfus` | Higher is better | Weighted cognitive level average |
| Depth | `decision_quality` | Higher is better | Decision titles with explicit reason keywords |
| Breadth | `exploration` | Higher is better | Exploration observations over all collaboration-mode observations |
| Breadth | `deep_invest` | Band | Share of valid projects with at least 20 sessions |
| Breadth | `fragmentation` | Lower is better | Share of valid projects with exactly one session |
| Collaboration | `delegation` | Lower is better | Delegation observations over all collaboration-mode observations |
| Collaboration | `mode_diversity` | Higher is better | Count of observed collaboration modes |
| Collaboration | `bug_decision` | Lower is better | Bugfix count over decision count |

The experiment drops three noisy live indicators:

- `depth_output`, because it mixed deep-inquiry observations with expert-level
  observations and did not reliably describe output quality.
- `knowledge_rate`, because extracted knowledge-item volume was sensitive to
  extraction style and could contradict its target text.
- `friction_density`, because friction item granularity was not stable enough
  to be a color-bearing collaboration metric.

Historical `scores.jsonl` entries that contain those old indicators remain
deserializable. The baseline registry simply stops computing personal averages
for removed live indicators.

## Breadth Weighting

`deep_invest` and `fragmentation` are project-bucket metrics. Projects named
`other` and projects with zero sessions are excluded from the denominator. For
example:

```text
solo-a: 1 session
solo-b: 1 session
deep-a: 20 sessions
```

The resulting rates are:

- `deep_invest = 1 / 3 = 33.3%`
- `fragmentation = 2 / 3 = 66.7%`

These values describe the shape of the project portfolio, not the share of
sessions spent in each bucket.

## Statusline and Advice Cache

`~/.mirror/statusline.txt` is written by `mirror score` so shell prompts can
read one short line cheaply.

Format:

```text
mirror-marker + depth/breadth/collab lights + optional trend arrow + optional streak + advice
```

The three lights are absolute target signals. The optional arrow is the
overall personal trend. Advice cache handling is deliberately visible:

- Generation only reuses a fresh cache entry for the exact score/model key.
- Display callers can still load stale cache entries so they can show
  `advice stale` instead of silently showing nothing.
- MOTD falls back to static tips when cached advice is stale, and marks that
  fallback.

## Weekly Action Card

`mirror weekly` builds one cluster for this week, computes this week's score
from that cluster, and passes the same cluster into the action-card builder.

Action cards only trigger from non-green breadth indicators:

- `exploration`
- `deep_invest`
- `fragmentation`

Project choice must be deterministic and evidence-backed:

- exploration or fragmentation risk selects the least-worked real project;
- deep-invest risk selects the main current project;
- evidence comes from questions, progress, patterns, knowledge, architecture,
  summary excerpts, decisions, or bugfixes in the same weekly cluster.

## Clustering Stability

Project clustering must not let ingestion artifacts distort breadth metrics:

- `tool` and `tools` path segments are both generic and are removed during
  project normalization.
- `agent_<hex>` session identifiers are not project names.
- `session_count` is deduplicated per project, while `global_stats.total_sessions`
  remains globally deduplicated.
- Profile project shares use the sum of per-project session assignments as
  their denominator, matching breadth scoring when one session belongs to
  multiple projects.

The per-project dedupe matters when observations from one source document
carry different project tags. Each affected project should count that source
document once; the old global-only set made the result depend on observation
iteration order.

## Validation Scope

Focused validation for this issue:

```bash
cargo fmt --all --check
cargo test -p mirror
cargo test -p refine-core session::clustering
git diff --check
```

Broader workspace tests are useful before release, but this issue intentionally
does not change parser transport, LLM ingest, SQLite migrations, or scheduler
scripts.
