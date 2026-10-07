# Refine delivery integration review (2026-10-07)

## Reviewed source and published version

The starting integration is `f46d98ddda58effb850071b71b01911ffef6b4fc`.
[PR #258](https://github.com/majiayu000/refine/pull/258) is the canonical review
entry. [PR #260](https://github.com/majiayu000/refine/pull/260) has the same head
and the same public diff at this review; it is not a second sequential merge.
No source draft was closed by this work.

The published [v0.1.3](https://github.com/majiayu000/refine/releases/tag/v0.1.3)
release points to `2c1dcab` and predates these changes. Installing the current
integration from source does not mean that a new packaged version was released.

## Existing draft implementation mapping

The public diffs were compared with the starting source, including implementation
conditions and their regression tests. The ten draft heads are not required to
be ancestors: integrated and subsequently changed code is checked directly.
“Retained” below is a source-level mapping, not a claim of exhaustive behavioral
equivalence or a fresh run of every test.

| Original PR | Original behavior | Current implementation and retained regression |
| --- | --- | --- |
| [#236](https://github.com/majiayu000/refine/pull/236) | Future scores cannot activate or distort a baseline. | `apps/mirror/src/score/baseline.rs` still requires `timestamp <= now`, now also isolates compatible scope/cohort evidence. Both `future_only_scores_do_not_activate_personal_baseline` and `future_scores_do_not_change_personal_baseline_or_trend` remain in `score/tests/baseline.rs`. |
| [#239](https://github.com/majiayu000/refine/pull/239) | Legacy import must preserve the observation update guard. | `packages/core/src/infra/observation_integrity.rs` drops only the insert trigger. Import restores/verifies triggers transactionally. That file and `legacy_import/observation_import_tests.rs` exactly reuse the draft blobs, including `newer_detached_legacy_copy_cannot_clear_an_existing_observation_link`. Later capture-publication fencing is additional. |
| [#241](https://github.com/majiayu000/refine/pull/241) | Obsolete UI results cannot replace a newer selection/search/list. | `Spotlight.tsx` retains the active-effect fence and cleanup; its implementation and test blobs exactly match the draft. `lib/store.ts` retains request generations and selection-by-ID deletion, extended with explicit delete receipts and failure feedback. All seven original asynchronous regression titles remain in the corresponding tests. |
| [#243](https://github.com/majiayu000/refine/pull/243) | Empty canonical scores invalidate advice/statusline; ad hoc views preserve them; cached render language must match. | `advice/cache.rs` retains `render_language` in cache identity and read rejection. `score/publication.rs::invalidate_empty_score_cache` performs owner/cutoff-ordered invalidation only for a canonical scope, preserves NotFound handling and reports other I/O failures. The original language and empty/custom-window regressions remain; later publication fencing extends them. |
| [#245](https://github.com/majiayu000/refine/pull/245) | Known Grok author roles override parity fallback. | The complete draft patch reverse-applies to the starting source. The role-regression file exactly matches the draft blob and covers known selectors, precedence, and unmatched fallback. |
| [#247](https://github.com/majiayu000/refine/pull/247) | Reject over-cap facet arrays and use bounded parse regeneration without truncation. | All three parse formats still call `validate_facet_limits`; all ten original caps remain. `parse_facet_response_enforces_every_array_cap_in_every_format`, `parse_facet_response_preserves_all_at_limit_entries`, and `facet_array_limit_error_uses_bounded_parse_regeneration` remain. Later field/evidence validation and request-size limits retain this response contract. |
| [#249](https://github.com/majiayu000/refine/pull/249) | Weekly is local, baseline requirements are explicit, and snapshot tension does not claim growth. | `cli.rs` retains local Weekly help and the README test; `score/display.rs` retains seven distinct dates/28-day explanation. `score/compute.rs` describes captured evidence only, and `snapshot_tension_does_not_claim_personal_growth` remains. This delivery further labels Mirror experimental without changing scoring. |
| [#251](https://github.com/majiayu000/refine/pull/251) | SQLite startup/schema upgrade decisions happen under an IMMEDIATE write transaction. | `infra/mod.rs::prepare_sqlite_db` and `db_migration.rs::prepare_migration_state` retain IMMEDIATE transactions and commit/error handling. Both startup regression files exactly match the draft blobs and cover blocking plus overlapping initializers. |
| [#254](https://github.com/majiayu000/refine/pull/254) | Probe a staged portrait binary before replacing the installed binary. | All three installer/test files exactly match the draft blobs. The complete draft reverse-applies; a candidate is copied, secured, help-probed, hashed and only then promoted. |
| [#257](https://github.com/majiayu000/refine/pull/257) | CLI result IDs round-trip into show/doc-show. | The complete draft reverse-applies. `support.rs` and `tests/copyable_result_ids.rs` exactly reuse the draft blobs, including both item/document round-trip regressions. |

Public diff snapshots, exact blob comparison, reverse-patch results, and GitHub
HTML evidence are saved separately under the task evidence directory. They are
review inputs; reverse application alone does not prove runtime correctness.

## Current starting-head CI evidence

Both public runs at the exact starting head completed successfully:

- [#258 run 37595372864](https://github.com/majiayu000/refine/actions/runs/37595372864)
- [#260 run 37594674589](https://github.com/majiayu000/refine/actions/runs/37594674589)

Each run exposes ten successful jobs: check, test, MSRV, Clippy, fmt, shell-test,
extension, desktop-ui, RustSec, and skill-check. This is starting-head evidence.
It does not verify the new local delivery changes. Public logged-out pages do
not expose full logs, so this review does not infer test counts from them.

## Mirror interpretation in this delivery

Mirror remains experimental session reflection. README, CLI help, score,
dashboard, MOTD, Weekly and profile instructions describe signals as configured
working preferences. Delegation and bug/decision directions are not validated
measures of user progress. Profile instructions distinguish observed data from
hypotheses and allow delegation to fit the task. The indicator formulas, targets,
trend algorithm, serialization and cache/error contracts are unchanged.

The main product wording centers on recovering decisions and reusing lessons,
including failed approaches. Historical generated portraits are retained as
historical evidence rather than rewritten to imply a fresh user evaluation.

## Verification boundary

This mapping and wording review completed source inspection, public CI/release
verification, scoped Rust formatting, and `git diff --check`. The integrated
Rust/UI runtime checks, real installation, extension browser boundary, commit
prototype source checks, and release publication have their own delivery
records; none is inferred from this document. Author/reviewer timing experiments
and macOS Finder/signing acceptance require actual participants/environments.
