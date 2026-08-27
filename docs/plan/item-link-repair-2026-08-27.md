# Detached Observation repair runbook

This runbook covers the one proven historical shadow-Document-ID incident. It
does not authorize fuzzy matching, deletion, or a provenance-mode change.

## Pinned evidence

- Evidence SQLite file: `/Users/lifcc/Library/Application Support/refine/refine.db.backup-codex-latest20-20260525-212200`
- SHA-256: `11c015feffa28e60c677f99f7442e38af297111229d8cb16c22efc3a4e44c140`
- Rule version: `shadow-document-id-exact-v1`
- Expected dry-run: `7,885` groups, `63,776` items, `0` target conflicts

The evidence path is operational input, not a compiled default. Both the path
and hash must be supplied explicitly. A symlink, schema mismatch, or hash
mismatch is a hard failure.

## Exact candidate rule

A historical group is eligible only when all of these checks pass:

1. Its evidence `document_id` has no parent in the evidence DB.
2. Exactly one evidence Observation title identifies a current Document title.
3. That title occurs on exactly one current Document.
4. The evidence Observation and current Document creation timestamps differ by
   no more than one second.
5. Every evidence item still exists as a current Observation and remains
   detached.
6. The current target Document exists and no other evidence group claims it.

Ambiguous, missing, late, already-linked, and otherwise unproven rows stay
`NULL` and remain visible in the dry-run counters. There is no nearest-time or
fuzzy fallback.

## Deployment and live sequence

1. Merge and install the runtime containing the `ON DELETE RESTRICT`,
   Observation insertion triggers, repair ledger, audit, and repair command.
2. Stop scheduled session ingestion and confirm no session mutation lock is
   held.
3. Record the deployed audit baseline and cutoff:

   ```sh
   refine --db "/Users/lifcc/Library/Application Support/refine/refine.db" \
     audit-item-links
   ```

4. Run the repair without `--apply` and require the pinned result:

   ```sh
   refine --db "/Users/lifcc/Library/Application Support/refine/refine.db" \
     repair-item-links \
     --evidence "/Users/lifcc/Library/Application Support/refine/refine.db.backup-codex-latest20-20260525-212200" \
     --evidence-sha256 11c015feffa28e60c677f99f7442e38af297111229d8cb16c22efc3a4e44c140
   ```

5. Choose a new, non-existent backup path on the same durable volume. Apply is
   a separate explicit command:

   ```sh
   refine --db "/Users/lifcc/Library/Application Support/refine/refine.db" \
     repair-item-links \
     --evidence "/Users/lifcc/Library/Application Support/refine/refine.db.backup-codex-latest20-20260525-212200" \
     --evidence-sha256 11c015feffa28e60c677f99f7442e38af297111229d8cb16c22efc3a4e44c140 \
     --apply \
     --backup "/Users/lifcc/Library/Application Support/refine/refine.db.pre-item-link-repair-20260827"
   ```

   Apply fails fast if the session mutation lock is held. Inside the lock, it
   creates a SQLite API backup, updates links and the append-only ledger in one
   transaction, preserves the total item count, and runs `quick_check` plus
   `foreign_key_check` before commit.

6. Re-run the dry-run. It must report zero candidates. Re-run the audit and pin
   its count/cutoff as the deployment guard. A later baseline breach exits
   non-zero:

   ```sh
   refine --db "/Users/lifcc/Library/Application Support/refine/refine.db" \
     audit-item-links \
     --baseline-detached-count <POST_REPAIR_COUNT> \
     --cutoff <DEPLOYED_RFC3339_TIMESTAMP>
   ```

7. Only after database verification should ingestion and cognitive-report jobs
   resume.

Relinking establishes a proven Document relationship only. It does not promote
`session_mode_unknown` to `session_mode_interactive`; analytics must continue to
exclude unknown, unattended, and subagent modes according to their cohort rule.
