# SPEC: Mirror Scoring and Codex Weekly Semantics

Date: 2026-08-13
Status: Accepted scoring schema v3

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
| Codex transcript parser | Current `response_item.payload.type = "message"` schema, roles, text types, metadata, and timestamp extraction are already implemented in `packages/core/src/session/parser.rs`. | Preserve message transport; extend only explicit metadata provenance. |
| Codex project attribution | Discovery paths and session metadata can disagree or encode the same repository differently. | Prefer repository metadata and normalize path aliases. |
| Event-time storage | Session documents have `captured_at`; session ingestion sets it from `SessionMeta.started_at` or file mtime. | Preserve. |
| Weekly event-time window | `mirror weekly` already uses `find_observations_by_event_range`. | Preserve. |
| Score/dashboard event-time window | `mirror score` and dashboard already use the same event-time query for default and `--since` windows. | Preserve. |
| Scheduled workflow hardening | PR #143 made daily/weekly automation locked, serialized, and failure-visible. | Preserve locking and failure propagation; add the metadata reconciliation step. |
| Cognitive portrait archive | PR #144 extracted generated reports and scheduling. | Do not add generated reports. |

The remaining portable work is therefore limited to scoring semantics,
statusline/advice visibility, weekly action cards, clustering input stability,
metadata reconciliation, and this specification.

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
| Depth | `decision_quality` | Higher is better | Decision titles with explicit reason keywords; displayed as **Reason Explicitness**, not decision quality |
| Breadth | `exploration` | Higher is better | Exploration observations over all collaboration-mode observations |
| Breadth | `deep_invest` | Band | Projects with at least 20 sessions over scored projects; displayed as **Mature Project Share** |
| Breadth | `fragmentation` | Lower is better | Single-session projects over scored projects; displayed as **One-off Project Share** |
| Collaboration | `delegation` | Lower is better | Delegation observations over all collaboration-mode observations |
| Collaboration | `mode_diversity` | Higher is better | Count of observed collaboration modes |
| Collaboration | `bug_decision` | Lower is better | Extracted bugfix count over extracted decision count; displayed as an extraction ratio |

The experiment drops three noisy live indicators:

- `depth_output`, because it mixed deep-inquiry observations with expert-level
  observations and did not reliably describe output quality.
- `knowledge_rate`, because extracted knowledge-item volume was sensitive to
  extraction style and could contradict its target text.
- `friction_density`, because friction item granularity was not stable enough
  to be a color-bearing collaboration metric.

Historical `scores.jsonl` entries that contain those old indicators remain
deserializable and remain available as score-run activity for streak tracking.
They do not participate in current MOTD, dashboard, or personal-trend reads:
new entries carry `score_schema_version = 3`, and metric consumers only compare
entries with the current scoring semantics. All unversioned entries and v1/v2
entries are activity-only. This prevents incompatible project denominators
from contaminating current baselines while preserving score-run streaks.

## Breadth Weighting

`deep_invest` and `fragmentation` use project buckets because their fixed
thresholds were defined as percentages of projects. Mixing a session-weighted
numerator with project-level target bands produced false red signals. For example:

```text
solo-a: 1 session
solo-b: 1 session
deep-a: 20 sessions
```

The resulting rates are:

- `deep_invest = 1 / 3 = 33.3%`
- `fragmentation = 2 / 3 = 66.7%`

The synthetic `other` bucket is excluded. These indicators describe portfolio
shape, not time allocation or context switching; their display names must not
claim otherwise.

## Personal Cohort Provenance

Mirror's personal score includes direct interactive sessions and legacy
sessions whose provenance is unknown. It excludes documents explicitly tagged
as unattended Codex exec or subagent work. The classification is based only on
Codex `session_meta` fields (`originator` and `thread_source`), never on titles,
prompt keywords, project names, or inferred scheduler intent.

`codex_exec` proves an unattended execution surface, not that a scheduler
started it. Mirror therefore calls this cohort `unattended`, not `scheduled`.
Metadata tags are persisted on every observation item. A local metadata-only
backfill can tag already extracted remem-backed observations without another
LLM extraction via `refine ingest-sessions --provider local --source codex
--backfill-session-metadata`.

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
- Cache versions include the scoring schema, so advice generated from an older
  score formula is rejected immediately after an upgrade.
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
- `/` and `-users-...` path encodings normalize through the same path-segment
  rules; generic grouping directories such as `infra` do not become projects.
- Codex `git.repository_url` is preferred to cwd/discovery aliases when present.
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
cargo test -p refine-core session::
cargo test -p refine-cli ingest_sessions
git diff --check
```

Broader workspace tests are useful before release. This v3 correction changes
metadata parsing and metadata-only ingest backfill, but not message transport,
LLM facet semantics, SQLite schema, or scheduler scripts.
