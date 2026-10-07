# Session extraction, revisions, and bounded reprocessing

Remem remains the owner of raw transcripts and their identities. Refine stores
derived observations, recipe identities, revision history, and evidence
references. None of these tables stores a second raw transcript.

## Preview and rebuild

```sh
# Preview the next 20 eligible missing/stale-recipe projections.
refine ingest-sessions --reprocess --latest 20 --dry-run

# Publish at most 20 eligible projections. Repeating this command advances
# through stale recipes because completed current-recipe projections are skipped.
refine ingest-sessions --reprocess --latest 20

# Explicitly regenerate even the current recipe, still with a final bound.
refine ingest-sessions --reprocess=all --latest 5 --dry-run
refine ingest-sessions --reprocess=all --latest 5
```

`--reprocess` requires `--latest N` with `N > 0`; a pre-filter summary `--limit`
does not satisfy that bound. Normal ingestion still refreshes changed source
snapshots and does not silently regenerate the entire archive after an upgrade.
Reprocessing retains the normal low-signal, scheduled-session, identity, and
quarantine checks. Use `--retry-quarantined` explicitly when retrying rejected
source snapshots is intended.

The preview does not call the LLM or publish Items, recipe metadata, or history.
When LLM configuration is available, the preview reports the exact target recipe.
Without that configuration, the target recipe is reported as unknown and existing
projections are conservatively selected for review; configure the intended model
before relying on the preview as an exact stale-recipe count.

The recipe identity hashes the extraction schema/algorithm version, system and
user prompt templates, and the LLM client's provider/model/endpoint identity.
Credentials and transcript text are excluded. It is stored independently in
`session_projections`; Remem `source_version` keeps its existing hash/mode format.
Changing extraction semantics outside the prompt requires bumping the algorithm
identity in `facet_recipe_identity`.

## Valid empty results and failures

The response must have a non-empty `session_summary` and supported
`cognitive_level` / `collaboration_mode` values. Empty facet arrays are valid.
When evidence is insufficient, use `unknown` for the relevant label and explain
the absence of observations in the summary. Empty objects, error envelopes,
unsupported fields/enums, whitespace-only array entries, and over-limit arrays
are rejected. They use the bounded regeneration path and cannot replace the
previous valid projection.

The Document, derived Items, recipe identity, and revision archive commit in one
SQLite transaction. A failure in any component leaves the previous state intact.

## Complete facet request limit

Facet extraction has a fixed local input limit, `FACET_REQUEST_MAX_BYTES = 65,536`.
Its unit is UTF-8 bytes: the fully rendered user prompt (including the template,
role/provenance headers, newlines, and reduction separators), plus the full
`FACET_SYSTEM_PROMPT`, plus a fixed 1,024-byte framing allowance. Requests exactly
at the limit are accepted; requests above it are rejected before a provider call.
This is a bound on logical prompt/system text with reserved framing space, not an
exact serialized HTTP-body size, provider token count, or guarantee of fitting any
model's context window. JSON escaping and provider-specific model/wire metadata
are not measured by this contract.

The 30,000-byte chunk activation threshold and 25,000-byte message-boundary target
remain soft chunking choices. A whole oversized message retains its source ID,
role, time, and text; it is never split or silently truncated. All initial chunks
are preflighted before the session's first provider request, so an oversized later
chunk spends nothing on earlier chunks. The same check at the shared facet call
boundary covers unchunked sessions, the final reduction, parse regeneration, and
the unchanged input used by provider retries. There is no extraction recipe bump:
accepted input, message identities, prompts, and reduction semantics are unchanged.

`InfraError::FacetRequestTooLarge` reports the stage and measured/allowed sizes,
without source text. It is a deterministic non-retryable error and enters the
existing quarantine as `facet_request_too_large`. Unchanged quarantined snapshots
are skipped unless `--retry-quarantined` is explicit; adjust the source input before
retrying. No partial projection is published, any previous projection remains
intact, and a failed incremental ingest does not advance its success cursor.
Unrelated sessions in the selected batch continue; request-size rejection does not
set the batch quota flag. A final reduction can fail after successful chunk calls:
the diagnostic reports completed chunks, and their provider attempts remain in the
existing usage ledger. The separate process-wide attempt/token budget is unchanged.

## Message evidence

Verified Remem messages retain their original ID, sender role, and timestamp.
The extraction view includes those references; canonical transcript text and
Remem snapshot hashes retain their existing format. Local legacy records without
an authoritative message identity keep missing provenance explicitly unknown.

The optional `evidence` array links a field and zero-based index to real message
IDs, for example `{"field":"decisions","index":0,"message_ids":[41,42]}`.
Unknown IDs, nonexistent field positions, duplicate references, and model-supplied
roles/times are rejected before publication. Chunk results pass the same checks;
the final reducer may cite only IDs retained by those results. Missing references
are stored as `unknown`, even when source messages exist.

`session_projections.evidence_json` stores the source ID/role/time ledger and
reference mapping for machine candidate observations, without message bodies.
The status `references_validated` means the source reference exists, not that the
claim was proven correct or that the message sender authored every quoted word.
Human corrections do not inherit a claim of extractor validation. Archived
revisions retain the previous mapping for inspection with `projection-history`.

This change does not infer per-observation event times or move historical reporting
windows. Reports still use the Document's captured session time; deciding which
event time represents a multi-message claim is a separate semantic decision.

## Human edits, deletions, and old IDs

Explicit edits through the Item repository (including the desktop editor) are
saved as overrides. Reprocessing applies a matching override to its new machine
candidate. An unmatched human edit remains visible with `curation_needs_review`;
Refine does not guess that a reworded machine claim is the same decision.
Pending corrections are excluded from automatic metrics, evidence, and project
resolution, so the old edit and a new machine candidate cannot both count as a
confirmed decision. Their count is exposed as `curation_excluded_observations`
and marks the analysis cohort DEGRADED. The stored correction remains available
for review. A matched `curated` observation participates normally.

Explicit Item deletion records a tombstone. Reprocessing cannot recreate the
same logical observation, including repeated copies in a model response.
Machine replacement and migration deletes do not create user-deletion tombstones.
The logical identity uses the exact session reference and facet kind, plus
whitespace-normalized claim text; the session summary has one stable identity.
Semantic paraphrases are different candidates and need human review.

Pre-upgrade observations have no reliable record distinguishing machine text
from earlier human edits. Their complete derived payload and old Item IDs are
archived before replacement; they are not silently discarded or guessed to be
newly verified claims. To inspect previous observations:

```sh
refine projection-history <document-id> --limit 20
```

This reads up to 1,000 archived revisions as JSON without loading Remem text or
calling an LLM. History includes old Item IDs, source references, recipe identities
when known, and derived payloads. Current observations remain available through
`refine doc-show` / `refine show`.

## Incremental migration lookup

Legacy filename/epoch candidates are indexed once per ingest run. Only legacy
Documents enter that index. A fully migrated archive therefore performs no
per-summary scan over all current Documents. Remem's complete summary identity
validation remains in place; `--latest` continues to bound the eligible final
processing set rather than weakening identity checks upstream.
