# Mirror evidence, cohorts, and daily history

## Missing evidence

Every indicator carries an optional measured value and coverage. A missing
denominator produces `actual: null` and `Signal::Unknown`; it does not produce
a measured zero or a green signal. Coverage describes the valid labels or
project assignments behind the value, together with the eligible population.
Partial coverage remains visible without an arbitrary coverage cutoff.

For example, decisions with no extracted bugfixes can produce a measured
bug/decision ratio of zero. A cohort with no decisions has no denominator and
produces Unknown. Missing collaboration labels similarly make delegation and
mode diversity Unknown. A measured red indicator remains visible at layer
level; otherwise Unknown takes precedence over yellow and green.

Unknown values do not enter personal averages or trend comparisons. The
portfolio policy abstains when a required input is Unknown. Tension text only
describes the snapshot and does not independently issue portfolio actions.

Reason explicitness is a lexical proxy over distinct complete decision titles.
Whitespace and letter case are normalized for deterministic deduplication;
different titles sharing a prefix remain distinct. Rationale markers are
matched without case sensitivity. Choice verbs such as `选择` and `采用` do not
by themselves count as rationale. This value measures recorded rationale
markers, not correctness or overall decision quality.

## Source and time contract

Mirror and Insights use the same source-aware cohort builder. Supported sources
are Claude Code, Codex, Cursor, and Remem raw sessions. Non-session knowledge
documents, detached observations, unattended sessions, and subagent sessions
do not become eligible interactive-session evidence. Their exclusions remain
visible in data-quality counts.

Each command obtains observations and linked source metadata in one repository
snapshot. All windows compared by that command share the same snapshot and
project identity resolver. Score advice uses 90-day and 7-day windows; weekly
uses 90-day, current 7-day, and previous 7-day windows. Profile uses 90 days.
The existing two-window repository API supplies the enclosing range, which is
then split into the exact requested half-open windows in memory.

Pending curation is held out of the automatic cohort until reviewed. The score
method identity records this reviewed-curation contract, so scores computed
before this boundary do not silently enter the same metric baseline.

The event timestamp is `Document.captured_at`, currently the source session's
start time. This is a session-start cohort, not a claim that every message in
the session occurred in that window. Newly extracted content in an older
session keeps that session's time assignment. Missing document metadata falls
back to item creation time only for exclusion/accounting; it does not create
eligible linked evidence. Per-message evidence timestamps are a separate
provenance concern and do not silently redefine this history.

## Comparable score scope

Default `mirror score` and `mirror dashboard` produce canonical rolling-90-day
scores. A persisted scope contains:

- the canonical database path;
- the window kind and actual start/end timestamps;
- the scoring and source-cohort method version;
- the complete target configuration;
- the eligible cohort identity and data-quality counts.

Database aliases that resolve to the same path share history. A moved database
starts a separate baseline. This path-based identity does not detect an
unrelated database copied over an existing file at exactly the same path;
history should be reset or a new path selected when intentionally replacing a
database. Introducing a durable database UUID would require a separate storage
contract.

Window bounds and cohort identity are provenance, not equality requirements
across dates: a rolling window naturally contains a changing cohort. Database,
window kind, method, and target configuration must match before historical
metrics can be compared. Future timestamps and noncanonical scopes are
excluded.

## Daily history and migration

Schema v6 history keeps the latest canonical snapshot per scope and UTC date.
Concurrent writers hold the existing file lock across read, daily upsert,
retention, and atomic replacement. An older timestamp cannot replace a newer
snapshot merely by finishing last. Retention keeps 365 dates per scope rather
than 365 command invocations. The latest 365 legacy activity records are
retained separately.

Default score and dashboard share one local publisher for history, deterministic
advice, and statusline output. A publication lock serializes their writers.
An atomically replaced `score-publication.json` records the latest cutoff for
each database/window/method/targets scope, including empty or ineligible
snapshots. A publication must have a strictly later cutoff to advance that
scope; this ordering spans dates and does not depend on changing cohort or
quality metadata. An older process may render its requested view but cannot
append history or replace or invalidate newer derived output. Empty snapshots
advance the watermark without inventing a score, even on the first run.

The same watermark identifies the owner of the single advice/statusline cache.
A different scope may append its own history, but only a strictly later valid
publication can take cache ownership; equal cutoffs preserve the current
owner. An empty result only invalidates its own scope's cache, leaving another
database's output intact. Cache readers verify owner, scope, and cutoff rather
than matching a timestamp alone. On upgrade, existing canonical history seeds
the per-scope cutoffs; a legacy cache is accepted only when its timestamp and
cohort identify one matching scope. Unowned caches are not displayed and are
replaced or cleared by the next accepted publication.

Dashboard reads the recent 7-day cohort together with its 90-day
cohort, so `score → dashboard → motd` preserves a current matching policy cache.

Earlier schemas and v6 records without a canonical scope remain available to
activity/streak readers. Their metric values are not used for the new baseline.
Unknown future schemas still fail metric reads and writes visibly; the writer
does not discard them or overwrite their data.

Personal baselines use the last 28 days, require seven distinct observed dates
for each indicator, and select the latest snapshot on a date. Incompatible
scopes and degraded data-quality snapshots are excluded. Absolute target
signals remain unchanged; a personal trend is a separate comparison.

`--since` and `--all` are view-only windows. They do not write canonical score
history, advice caches, or statusline files, including when the selected window
is empty. `--require-advice` is valid only for the default window. Default empty
or ineligible cohorts invalidate stale advice/statusline output without
inventing a score. MOTD and dashboard metric history select the configured
database and targets before applying the display limit.

## Verification

Synthetic tests cover missing versus measured-zero evidence, partial coverage,
order-independent rationale deduplication, shared source exclusions and event
windows, database/target/window isolation, legacy activity compatibility, UTC
daily retention, concurrent writers, and view-only CLI behavior. CLI fixtures
run with isolated HOME and no API keys; deterministic score advice must still
be available without remote calls. Fixed-cutoff publication tests replay both
late-valid-after-empty and late-empty-after-valid orderings against real
watermark/history/cache files, including cross-scope and equal-cutoff cases.
