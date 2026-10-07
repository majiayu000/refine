# Refine 0.1.4 delivery candidate — 2026-10-07

This work keeps all six assigned scopes. The source starts at canonical
[#258](https://github.com/majiayu000/refine/pull/258), `f46d98d`, which is also
[#260](https://github.com/majiayu000/refine/pull/260)'s head. The original `main`
checkout is retained unchanged; work is isolated on `codex/refine-delivery-20261007`.
Remem/agent-sessions production source and data are not changed.

| Scope | Implementation and completed evidence | Remaining acceptance |
| --- | --- | --- |
| #258 integration and delivery | Ten original drafts mapped in `refine-delivery-integration.md`. Starting-head #258/#260 each have ten green checks. Candidate versions align at 0.1.4. | Local install and macOS/CLI/extension candidates are complete. Candidate PR/CI delivery is proceeding through the GitHub object API after CLI authentication failed. Merge/public release and Developer ID/notarization/manual use are still pending. |
| Private recommendations | Existing popup owns preview titles, summaries, tags and content. Only explicit selected insertion reaches the page. Real MV3/Chromium DOM-reader checks pass, including stale input/URL, site-off and failure receipts. | Current signed-in provider editors and human toolbar install/use are unverified. |
| #259 complete request size | Fixed 65,536 UTF-8 byte logical request limit covers prompt/template/system plus 1,024 bytes of reserved framing. All chunks preflight, final reduction/regeneration/retry share the boundary; non-retryable failures quarantine without partial publication or success cursor advance. Soft 25k/30k and the process budget remain separate. | Exact provider wire/token/context suitability is not claimed. No paid provider call is needed for this input-contract acceptance. |
| Mirror | README/product/UI/CLI/profile describe experimental reflection. Delegation and other configured directions do not establish progress. | Actual user effect remains unmeasured; scoring model is unchanged. |
| Commit/PR discussion | Shared native/HTTP query and Knowledge Console entry reuse successful Git capture, exact raw-session v2 messages, current projections/evidence and archived decisions. A public PR resolves membership with GitHub's real API. | Exact identities are missing for 17 of the real product samples. Explicit semantic replacement and real author/reviewer timing remain unknown/unperformed. |
| Ownership boundaries | Raw acquisition/parsing remains with Remem/agent-sessions; Refine loads original messages on demand without persisting them. Projection recipe/source hashes, revisions, overrides and tombstones remain in Refine. | Worktree commit links need an authoritative full raw selector from the owner to recover mismatched-project sources; this candidate does not guess or backfill. |

## Actual source validation

`docs/eval/commit-source-20261007.json` contains aggregate results without original
message bodies. Twenty actual local Git objects and stored source links were
checked, then all twenty were queried through the running 0.1.4 API. They span
Loom worktrees, rclean, rui, Harness and shipwise; the local Refine archive has no
commit links, so they are not advertised as twenty Refine commits.

- 20/20 API identities (SHA, project, session and link source) match real records.
- 3 commits return verified original messages from 2 exact sessions, 26 distinct
  messages. IDs, role, text, time, selector and snapshot hash match independent
  read-only exports. Two commits share the same 17-message source session.
- 17 return unknown with no body/hash/selector: four worktree/project identity
  mismatches and thirteen historical weak links without current raw identity.
- Seven commits have independent explicit successful capture evidence. Independent
  exact hydration can recover seven through their known selectors; that broader
  operator knowledge is not a license for the product to guess four project links.
- Three invalid/empty-project/no-link error cases pass. At the validated starting
  head `f46d98d`, public PR #258 expands nine commits, then correctly returns no
  local Refine associations. Updating the PR changes membership; this is baseline
  API evidence rather than a claim about a later head.

The browser checks run the actual Refine UI and API for a recovered source and a
missing source. Four additional UI cases use clearly synthetic projection data
for quotation references, archived alternatives, curated edits and clearing stale
results. They are not real model extraction/semantic accuracy measurements.

Production Remem access was SQLCipher read-only plus Remem's read-only raw export.
Commit CLI/API acceptance used a consistent encrypted private snapshot, because
Remem's existing commit query may run its governance gate. Full transcripts,
private API responses and keys are outside Git under a 0700 evidence directory;
keys have 0600 permissions. No credentials or private transcript bodies are
included in this candidate.

The actual author/reviewer experiment is prepared as an unfilled 20-row record in
the task evidence directory. No participant, timing or user improvement is claimed.
The UI shows archived alternative decisions, while semantic supersession remains
explicitly unknown because no authoritative relation exists in the current contract.

## Verification and publication status

Extension: 33 tests, TypeScript and production MV3 build pass; Chromium DOM-reader
acceptance passes against candidate version 0.1.4. Desktop UI: 27 tests and
TypeScript/Vite production build pass. Actual UI Playwright checks pass.
The full workspace including native completed 770 tests, zero failures and four
existing ignored tests. Final provenance corrections then passed 426 core/server
tests, including the direct-edit, full-tag and mismatched-version regressions.
Stable workspace check and Clippy with warnings denied pass, as do formatting
and patch whitespace checks. The unused general version metadata extension was
removed; existing ingestion version queries retain their original contract.
Rust 1.88 workspace MSRV and all CI shell suites pass. The clean committed source
passes `test-install-local.sh`, including seven binary-replacement tests. Actual
CLI, Mirror and server install into an isolated prefix; CLI/Mirror report 0.1.4,
CLI search succeeds, server health passes, and all twenty real source API checks
pass again through the installed server. Browser acceptance against that server
passes six cases: two actual sources and four synthetic projection cases.

A debug macOS arm64 `.app` builds, uses ad-hoc signing, passes deep strict signature
verification, and is copied into an isolated Applications directory from a
verified read-only mounted DMG. Native GUI use and Apple notarization are not
claimed. CLI/Mirror/server and the production MV3 extension are also packaged.
Artifact hashes, build source and acceptance records are outside Git in the
task evidence directory; no private database or source body is packaged.
RustSec reports zero vulnerabilities, with existing unmaintained/unsound
dependency warnings preserved.

The owner gh credential is invalid, the other stored gh account is read-only,
HTTPS Git cannot obtain a username, and SSH Git rejects its key. GitHub connector
auth/repository read tools return internal errors, but its Git blob writer works;
source/PR delivery therefore uses the object API with a checked branch lease.
Current candidate CI will be recorded against the resulting exact remote head.
No merge or new public release is claimed. Manual account/native tests and actual
author/reviewer timing remain pending rather than being replaced by synthetic
results.
