# SPEC: Codex Session Ingestion for Mirror Weekly

Date: 2026-05-25
Status: Draft

## Goal

Mirror Weekly must analyze both Claude Code and Codex coding sessions. The weekly score, project clusters, and action card must be based on the same observation database, with Codex sessions parsed, attributed to real projects, and included without corrupting the current week during backfill.

## Current Finding

This is not a discovery problem. `refine ingest-sessions` is designed to scan both:

- Claude Code: `~/.claude/projects/*/*.jsonl`
- Codex: `~/.codex/sessions/**/*.jsonl`

The current local proof is:

```text
documents by source:
claude-code-session|7914
session-insights-v2|14
mirror-weekly|9
grok|9
mirror-profile|6
gemini|6
session-insights|3
```

There is no `codex-session` document source in the current database.

Dry-run proof:

```text
$ cargo run -q -p refine-cli -- ingest-sessions --source codex --dry-run --latest 20
发现 6690 个会话文件

[dry-run] 可处理 0, 跳过重复 0, 过滤 20
```

Claude Code comparison:

```text
$ cargo run -q -p refine-cli -- ingest-sessions --source claude --dry-run --latest 20
发现 1711 个会话文件
  [dry-run] ... | 19 msgs | 40947 chars | ClaudeCode
  [dry-run] ... | 34 msgs | 28977 chars | ClaudeCode

[dry-run] 可处理 2, 跳过重复 18, 过滤 0
```

Field-shape sampling of the latest 50 Codex JSONL files found:

```text
files=50
lines=35299
parse_errors=0
types=response_item:24865, event_msg:10104, turn_context:245, session_meta:50, compacted:35
response_roles=<missing>:22474, assistant:1772, user:310, developer:309
content_types=object:4185, output_text:1772, input_text:862, input_image:5
session_meta_keys=agent_nickname,agent_role,base_instructions,cli_version,cwd,dynamic_tools,git,id,model_provider,originator,source,thread_source,timestamp
```

The current parser expects older Codex shapes:

- top-level `type = "user_message"` for user text
- `response_item.payload.content[].type = "text"` for assistant text
- top-level `session_meta.model`

Current Codex sessions mainly use:

- `type = "response_item"`
- `payload.type = "message"`
- `payload.role = "user" | "assistant" | "developer"`
- `payload.content[].type = "input_text" | "output_text"`
- metadata under `session_meta.payload.*` and `turn_context.payload.*`

Because the parser does not map this shape into `MessageRole::User`, the existing filter sees `user_message_count() == 0` and rejects the session.

## Data Flow

Mirror Weekly does not read chat logs directly.

```text
Claude Code / Codex JSONL
  -> refine ingest-sessions
  -> Document(source = claude-code-session | codex-session)
  -> Observation Items
  -> cluster_observations()
  -> score::compute()
  -> mirror weekly report + action card
```

Important code surfaces:

- `packages/core/src/session/discovery.rs`: discovers Claude Code and Codex JSONL files.
- `packages/core/src/session/parser.rs`: converts JSONL into `Session`.
- `packages/core/src/session/filter.rs`: rejects sessions with no user messages or too few chars.
- `apps/cli/src/ingest_sessions.rs`: parses, filters, extracts facets, saves `Document` and `Item`.
- `packages/core/src/session/clustering.rs`: groups Observation items into real projects and metrics.
- `apps/mirror/src/weekly.rs`: reads Observation items, splits this week vs last week, computes weekly signals.
- `apps/mirror/src/weekly/action_card.rs` in the action-card branch: generates the project-grounded weekly action card.

## Non-Goals

- Do not make Mirror Weekly scan `~/.codex` directly.
- Do not introduce an LLM planner for the action card in v1.
- Do not ingest developer/system instructions as user intent.
- Do not silently weaken the quality filter to make bad parser output pass.
- Do not mix this ingestion fix into unrelated weekly UI copy changes.

## Design

### 1. Parse Current Codex Transcript Shape

Use `response_item.payload.type = "message"` as the canonical transcript source.

Mapping:

| Codex shape | Refine role | Text extraction |
|---|---|---|
| `payload.role = "user"` | `MessageRole::User` | `payload.content[].text` where type is `input_text` or legacy `text` |
| `payload.role = "assistant"` | `MessageRole::Assistant` | `payload.content[].text` where type is `output_text` or legacy `text` |
| `payload.role = "developer"` | skip | not user intent |
| `payload.role = "system"` | skip | not user intent |
| `payload.type = "reasoning"` | skip | internal reasoning summary, not transcript |
| `payload.type = "function_call"` | skip | tool call metadata |
| `payload.type = "function_call_output"` | skip | tool output metadata |
| `payload.type = "custom_tool_call"` | skip | tool call metadata |
| `payload.type = "custom_tool_call_output"` | skip | tool output metadata |
| `payload.type = "web_search_call"` | skip | tool call metadata |

Keep legacy support:

- top-level `type = "user_message"` with `content`
- old `response_item.payload.content[].type = "text"`
- old top-level `session_meta.model`

### 2. Treat `event_msg` as Fallback, Not Canonical

`event_msg` often duplicates transcript-visible content. The parser should not use it as the primary transcript source.

Allowed fallback:

- If a file has no canonical `response_item.payload.type = "message"` transcript, accept `event_msg.payload.type = "user_message" | "agent_message"` after text-hash de-duplication.

Skipped event metadata:

- token counts
- task lifecycle events
- turn aborts
- context compact markers
- UI/status events

### 3. Extract Codex Metadata

Codex parser should populate `SessionMeta` from current metadata shapes:

Priority:

1. `session_meta.payload.timestamp` -> `SessionMeta.started_at`
2. `session_meta.payload.cwd` -> `SessionMeta.project`
3. latest `turn_context.payload.cwd` -> `SessionMeta.project` fallback
4. latest `turn_context.payload.model` -> `SessionMeta.model`
5. `session_meta.payload.model` -> model fallback
6. legacy `session_meta.model` -> model fallback

Project value should be normalized the same way Claude project tags are normalized downstream. For v1, storing the raw cwd in `SessionMeta.project` is acceptable if `facets_to_items` already normalizes tags through clustering.

### 4. Preserve Real Project Attribution

Codex discovery currently sets `project: None`. If this remains unchanged, Codex observations will cluster under `other`, which makes the action card generic or wrong.

In ingest:

```rust
let project = ds.project.clone().or_else(|| session.meta.project.clone());
```

Then pass `project.as_deref()` into `facets_to_items`.

This keeps Claude Code behavior unchanged while allowing Codex to inherit project identity from `cwd`.

### 5. Backfill Without Polluting This Week

Current Mirror Weekly windows are based on `Item.created_at()`. A historical Codex backfill would save old sessions today and make them look like this week's work.

Required design choices:

1. Add a session event-time field to the saved observation path, using `SessionMeta.started_at` when available and file mtime as fallback.
2. Make weekly filtering use session event time, not save time, for session-derived observations.
3. If the current storage model cannot support this immediately, provide an explicit backfill mode that writes into a temp DB first and does not run production weekly until time semantics are fixed.

Minimum v1 acceptance can be split:

- Parser PR: makes Codex sessions parse and pass dry-run.
- Project attribution PR: makes parsed Codex observations cluster under real projects.
- Backfill/time PR: prevents historical imports from changing the current week.

### 6. Action Card Integration

The action card should stay inside `mirror weekly`.

Generation rule:

- Trigger only when breadth indicators are non-green:
  - `exploration`
  - `deep_invest`
  - `fragmentation`
- Select a real project from this week's `ClusterResult`.
- Include project evidence from actual observations:
  - questions
  - progress
  - patterns
  - knowledge gained
  - architecture
  - summary excerpt
- If no project evidence exists, skip the card instead of falling back to a generic template.

The action-card branch already follows this direction. The ingestion fix is what makes Codex-derived project evidence available to it.

## Implementation Plan

### Phase 1: Parser Compatibility

Files:

- `packages/core/src/session/parser.rs`

Tasks:

1. Add helper to parse `response_item.payload.type = "message"`.
2. Extract `input_text`, `output_text`, and legacy `text`.
3. Map `payload.role = user` to `MessageRole::User`.
4. Map `payload.role = assistant` to `MessageRole::Assistant`.
5. Skip developer/system/tool/reasoning events.
6. Keep old Codex tests passing.

Expected proof:

```bash
cargo run -q -p refine-cli -- ingest-sessions --source codex --dry-run --latest 20
```

should show non-zero processable sessions and real user/assistant message counts.

### Phase 2: Codex Project Attribution

Files:

- `packages/core/src/session/parser.rs`
- `apps/cli/src/ingest_sessions.rs`
- possibly `packages/core/src/session/clustering.rs` tests

Tasks:

1. Read `cwd` from `session_meta.payload.cwd`.
2. Read fallback `cwd` from `turn_context.payload.cwd`.
3. In ingest, prefer discovery project for Claude and parser metadata for Codex fallback.
4. Verify Codex observations do not all cluster under `other`.

Expected proof:

```sql
select source, count(*) from documents group by source;
select tags, count(*) from items where item_type='observation' group by tags order by count(*) desc limit 20;
```

### Phase 3: Event-Time Backfill

Files to inspect before implementation:

- `packages/core/src/knowledge/types.rs`
- `packages/core/src/infra/sqlite/*`
- `apps/mirror/src/weekly.rs`
- `apps/mirror/src/score.rs`

Tasks:

1. Identify whether Item or Document already has a usable captured/event timestamp.
2. If no usable field exists, add one in a migration rather than overloading `created_at`.
3. Make Weekly's this-week/last-week split use event time for session-derived observations.
4. Add a controlled backfill path, for example `--source codex --backfill` or `--ignore-cursor`, with dry-run proof first.

### Phase 4: Weekly Verification With Action Card

Files:

- `apps/mirror/src/weekly.rs`
- `apps/mirror/src/weekly/action_card.rs`

Tasks:

1. Build weekly report with the same `this_cluster` used for scoring.
2. Generate action card only from actual project evidence.
3. Confirm Codex observations can affect project selection and evidence.
4. Keep output deterministic.

## Tests

Parser unit tests:

1. Current Codex schema:
   - `session_meta.payload`
   - `turn_context.payload.model`
   - `response_item.payload.type = "message"`
   - `payload.role = "user" | "assistant"`
   - content types `input_text` and `output_text`
2. Legacy Codex schema remains supported.
3. Developer/system messages do not increase `user_message_count`.
4. Tool/reasoning/function events do not become transcript messages.
5. Event fallback does not duplicate canonical `response_item` messages.
6. `SessionMeta.project` is populated from `cwd`.

Ingest tests:

1. `ds.project.or(session.meta.project)` is passed into `facets_to_items`.
2. Codex project tag appears on generated observations.
3. Duplicate document URL still prevents double insert.

Weekly tests:

1. Weekly uses event time for session-derived observations.
2. Backfilled old Codex sessions do not appear in the current-week report.
3. Action card uses a real Codex project when that project is the selected breadth-risk project.

Recommended commands after implementation:

```bash
cargo test -p refine-core session::parser session::discovery session::clustering
cargo test -p refine-cli ingest_sessions
cargo test -p mirror weekly
cargo check --workspace
cargo test --workspace
```

Local data validation:

```bash
TMP_DIR="$(mktemp -d)"
TMP_DB="$TMP_DIR/refine.db"

REFINE_DB_PATH="$TMP_DB" cargo run -q -p refine-cli -- ingest-sessions --source codex --dry-run --latest 20
REFINE_DB_PATH="$TMP_DB" cargo run -q -p refine-cli -- ingest-sessions --source claude --dry-run --latest 20

sqlite3 "$TMP_DB" "select source,count(*) from documents group by source;"
sqlite3 "$TMP_DB" "select tags,count(*) from items where item_type='observation' group by tags order by count(*) desc limit 20;"
cargo run -q -p mirror -- --db "$TMP_DB" weekly
```

## Acceptance Criteria

- `refine ingest-sessions --source codex --dry-run --latest 20` shows processable sessions after parser fix.
- `documents.source` includes both `claude-code-session` and `codex-session` after real ingest.
- Codex observations have project tags derived from `cwd` and do not all fall into `other`.
- Mirror Weekly reads Codex-derived observations through the same DB path as Claude Code.
- Historical Codex backfill does not inflate the current week's exploration/deep-invest/fragmentation metrics.
- When breadth is yellow/red, the action card names a real project and includes real observation evidence.
- Re-running ingestion does not duplicate already ingested session files.

## Risks

- Backfill can corrupt weekly interpretation if save time is used as event time.
- Project attribution can remain noisy if raw cwd is not normalized consistently.
- Full Codex backfill may be expensive because ingestion uses LLM facet extraction.
- `event_msg` fallback can duplicate transcript messages if de-duplication is missing.
- `mirror score` applies a personal baseline, while current `mirror weekly` uses raw thresholds; this can make daily and weekly signal lights differ.

## Open Decisions

1. Should event time be stored on `Item`, `Document`, or both?
2. Should backfill be a first-class CLI flag or a documented cursor-reset procedure?
3. Should weekly and score share personal-baseline semantics, or should weekly remain absolute-threshold only?
4. Should Codex developer messages ever be retained in raw `Document.raw_content` while excluded from metrics?

