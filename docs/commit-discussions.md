# Commit discussion prototype

Refine's Knowledge Console has a **从 commit / PR 找讨论** entry. Supply the exact
Remem project string and a hexadecimal SHA (7–64 digits) or public GitHub PR URL.
The same application query serves native Tauri and authenticated
`GET /v1/commit-context?project=...&reference=...`. The endpoint uses the existing
Bearer/dev-anonymous contract; no browser content script receives these records.

The query reuses `remem commit show --project ... --json` and raw-session v2
summaries/messages. Only `capture_git_evidence` links confirm an explicitly
successful captured commit. The link must resolve to one unique exact raw
selector under that project; another host/root/project is not guessed. Weak
legacy links, missing sources and ambiguity show **不知道**. Process timeout,
output overflow, malformed data and snapshot drift remain errors, not successful
empty results. Commit metadata does not prove why the change was made.

The UI shows the original discussion (including attempts), current decision
Items with their referenced original messages, and up to 20 archived projection
revisions with their previous decisions. Original text retains Remem's message
ID, sender and timestamp. Quotations require the current snapshot hash and machine
reference ledger; curated or unmatched edits do not inherit machine evidence. Current document,
items, evidence and history are read in one SQLite snapshot. Existing authoritative
override records suppress machine references immediately after a human edit,
including edits without a curation tag; mismatched evidence/source versions
produce unknown quotations.
A valid reference proves that a message exists, not that an extracted claim is
semantically correct. Missing decision evidence remains unknown. The current
storage contract does not record explicit semantic supersession between decisions;
history is available for comparison and the supersession answer remains unknown.
No new scoring model or inferred replacement relationship is introduced.

Raw messages are loaded on demand and are never persisted to the Refine database.
Refine adds no collector, original-format parser or raw transcript store.
Projection recipe identity, source-version hashes, publication, human overrides
and tombstones retain the existing session-projection contract.

Public PR lookup uses GitHub's official paginated commits endpoint. It requires
network access, propagates non-success responses and refuses PRs with 250 or more
commits because the endpoint cannot prove a complete list at that limit. Private
PR authentication is not added. Each returned SHA is scoped to the requested
Remem project. A PR with missing local sources returns unknown rather than model
inference. GitHub supplies commit membership, not private discussion evidence.

## Architecture choice and evidence

Required capability: a human can follow a commit/PR to already-captured source
messages and derived decisions, without a second ingestion chain or guessed
intent. Adopt Remem's successful-Git/identity contracts and adapt Refine's existing
UI, service, evidence ledger and projection history. No new library or service is
needed; local process guards and SQLite remain the operational boundaries.

Maintained GitLens offers commit inspection, metadata and issue/PR autolinks
([official documentation](https://help.gitkraken.com/gitlens/side-bar/)). Its docs
verify that user-facing capability; they do not establish an integration with
Remem's private raw archive, extraction identities or semantic supersession.
Those boundaries are unknown, so replacing this evidence path with GitLens is
rejected for this task. GitHub's
[official PR commits API](https://docs.github.com/en/rest/pulls/pulls#list-commits-on-a-pull-request)
is adopted only to resolve public PR membership, not as an intent source.
Remem owns captured tool success, source identities and raw messages; Refine owns
human-readable projections and review. Costs are bounded local process work and
public PR API calls; no model call or new hosted storage is required. Current
lookup may reload project summary/projection metadata and is a prototype rather
than a scale claim. Source availability, API rate limits and the absent semantic
replacement contract are the principal risks. Validation covers fake-contract
errors, exact source quotations and 20 real local Git associations separately
from the author/reviewer timing experiment.

## Safe acceptance setup

The existing Remem `commit show` command opens its store for governance checks;
querying it can quarantine unsafe memory summaries. For this acceptance, it is
run only against a consistent encrypted private snapshot with `REMEM_DATA_DIR`
pointing to the snapshot. Raw messages use Remem's read-only export. Production
Remem and agent-sessions source and data are not modified by the acceptance.
The public prototype delegates to that existing CLI; it does not weaken Remem's
poisoning/error contract or claim that every CLI query is side-effect-free.

See `docs/refine-delivery-20261007.md` for completed checks and remaining real
user/release acceptance conditions. Source validation and actual timing by an
author/reviewer are separate measurements.
