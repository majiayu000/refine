# Mirror Pending Issues Signal Report (2026-03-24)

Scope: GitHub issues `#13, #12, #11, #10, #9, #8, #6, #4, #3` in `majiayu000/refine`, validated against current `main` code snapshot.

## Diagnostic Signals

1. `#13 feat(mirror): detect isatty for ANSI output support no-color` (P2)
- Root cause: ANSI rendering policy is only partially centralized; TTY detection exists, but `NO_COLOR` opt-out is not implemented.
- Evidence:
  - `apps/mirror/src/score/types.rs` uses `std::io::stdout().is_terminal()` in `supports_ansi_on_stdout()`.
  - No `NO_COLOR` handling found in `apps/mirror/src`.
- Likely touch files:
  - `apps/mirror/src/score/types.rs`
  - `apps/mirror/src/dashboard.rs` (rendering verification)
  - `apps/mirror/src/score/tests.rs` (behavior tests)
- Status: actionable.

2. `#12 fix(mirror): growth-tracker path hardcoded not using refine_core resolve` (P2)
- Root cause (historical): hardcoded `~/.refine/growth-tracker.json`.
- Evidence of current state:
  - `apps/mirror/src/main.rs` resolves DB path via `resolve_db_path(&[])`.
  - `apps/mirror/src/score.rs` derives tracker path from `db_path` and falls back to legacy path only if needed.
- Likely touch files (if reworked): `apps/mirror/src/main.rs`, `apps/mirror/src/score.rs`.
- Status: already fixed in current code; skip candidate.

3. `#11 refactor(mirror): share llm_with_retry across weekly/profile/advice` (P2)
- Root cause (historical): inconsistent retry logic across modules.
- Evidence of current state:
  - Shared module exists at `apps/mirror/src/llm_retry.rs`.
  - `weekly.rs`, `profile.rs`, `advice.rs` all import/use `llm_with_retry`.
- Likely touch files (if reworked): `apps/mirror/src/llm_retry.rs`, `apps/mirror/src/{weekly,profile,advice}.rs`.
- Status: already fixed in current code; skip candidate.

4. `#10 refactor(mirror): deduplicate save_to_document in weekly and profile` (P2)
- Root cause (historical): duplicated document save logic.
- Evidence of current state:
  - Shared helper `apps/mirror/src/document_save.rs`.
  - `weekly.rs` and `profile.rs` both call `save_report_to_document(...)`.
- Likely touch files (if reworked): `apps/mirror/src/document_save.rs`, `apps/mirror/src/{weekly,profile}.rs`.
- Status: already fixed in current code; skip candidate.

5. `#9 refactor(mirror): reduce indicator extension cost from 7 files to 1` (P2)
- Root cause: indicator definitions, thresholds, scoring, and presentation are still split across multiple modules.
- Evidence:
  - Indicator metadata in `apps/mirror/src/score/indicators.rs`.
  - Threshold schema in `apps/mirror/src/config.rs` (`Targets` fields).
  - Scoring signal construction in `apps/mirror/src/score/compute.rs`.
- Likely touch files:
  - `apps/mirror/src/score/indicators.rs`
  - `apps/mirror/src/score/compute.rs`
  - `apps/mirror/src/config.rs`
  - `apps/mirror/src/score/tests.rs`
- Status: actionable.

6. `#8 feat(mirror): add rotate to weekly-history.jsonl (52 week cap)` (P2)
- Root cause (historical): unbounded weekly history growth.
- Evidence of current state:
  - `apps/mirror/src/weekly.rs` defines `WEEKLY_HISTORY_LIMIT: usize = 52`.
  - Persistence path trims old lines before atomic rewrite.
- Likely touch files (if reworked): `apps/mirror/src/weekly.rs`.
- Status: already fixed in current code; skip candidate.

7. `#6 fix(mirror): add serde(default) to ScoreResult and WeeklyRecord` (P2)
- Root cause (historical): backward compatibility breaks on older JSONL snapshots.
- Evidence of current state:
  - `apps/mirror/src/score/types.rs`: `ScoreResult` has `#[serde(default)]` + `Default`.
  - `apps/mirror/src/weekly.rs`: `WeeklyRecord` has `#[serde(default)]` + `Default`.
- Likely touch files (if reworked): `apps/mirror/src/score/types.rs`, `apps/mirror/src/weekly.rs`.
- Status: already fixed in current code; skip candidate.

8. `#4 refactor(mirror): deduplicate Signal→string conversion (5 copies)` (P1)
- Root cause (historical): repeated signal/string rendering logic.
- Evidence of current state:
  - `Signal` central API in `apps/mirror/src/score/types.rs`: `as_str()`, `emoji()`, `Display`.
  - Callers use centralized methods in `weekly.rs`, `advice.rs`, `motd.rs`, `dashboard.rs`.
- Likely touch files (if reworked): `apps/mirror/src/score/types.rs`, `apps/mirror/src/{weekly,advice,motd,dashboard}.rs`.
- Status: already fixed in current code; skip candidate.

9. `#3 refactor(mirror): split score.rs (1295 lines) into sub-modules` (P1)
- Root cause (historical): oversized multipurpose module.
- Evidence of current state:
  - `apps/mirror/src/score.rs` now delegates to modules:
    - `score/types.rs`
    - `score/compute.rs`
    - `score/baseline.rs`
    - `score/persistence.rs`
    - `score/display.rs`
    - `score/indicators.rs`
- Likely touch files (if reworked): `apps/mirror/src/score.rs`, `apps/mirror/src/score/*`.
- Status: already fixed in current code; skip candidate.

## Dependency Notes

- Actionable items are `#13` and `#9`.
- No hard file-level ordering dependency between `#13` and `#9`.
- P1 items (`#4`, `#3`) are already complete and therefore removed from critical path.
