# Mirror Sprint Signal Report (2026-03-24)

This report diagnoses each open Mirror issue before scheduling work.

## Scope
- Repository: `majiayu000/refine`
- Module focus: `apps/mirror/src/*`
- Source of truth: issue bodies + current code inspection

## Diagnostics

### #1 P0 fix(mirror): PersonalBaseline avg divisor bug
- Root cause: `compute_personal_baseline` divides indicator sum by `recent.len()` even when an indicator is missing in some records.
- Code evidence: `apps/mirror/src/score.rs:141-149`
- Touched files: `apps/mirror/src/score.rs` (tests in same file)
- Status: open, valid

### #2 P1 fix(mirror): silent error swallowing in JSONL parsing
- Root cause: parse/file errors are dropped via `.ok()` in score/weekly/advice loaders.
- Code evidence:
- `apps/mirror/src/score.rs:566-575`
- `apps/mirror/src/weekly.rs:205-213`
- `apps/mirror/src/advice.rs:17-21`
- Touched files: `score.rs`, `weekly.rs`, `advice.rs`
- Status: open, valid

### #3 P1 refactor(mirror): split score.rs
- Root cause: monolithic `score.rs` (1295 LOC) mixes signal types, compute logic, persistence, CLI path checks, and tests.
- Code evidence: `apps/mirror/src/score.rs` (1295 lines)
- Touched files: `apps/mirror/src/score.rs` + new `apps/mirror/src/score/*` modules + import callsites
- Status: open, valid

### #4 P1 refactor(mirror): deduplicate Signal -> string conversion
- Root cause: multiple conversions implemented separately (`Display`, ANSI glyphs, labels).
- Code evidence:
- `apps/mirror/src/score.rs:44-51`
- `apps/mirror/src/dashboard.rs:136-142`
- `apps/mirror/src/motd.rs:21-27`
- `apps/mirror/src/weekly.rs:131-137`
- `apps/mirror/src/advice.rs:41-47`
- Touched files: `score.rs`, `dashboard.rs`, `motd.rs`, `weekly.rs`, `advice.rs`
- Status: open, valid

### #5 P1 fix(mirror): persist_score non-atomic write + rotate race
- Root cause: append + reread + overwrite on same file can race and lose data under concurrent writes.
- Code evidence: `apps/mirror/src/score.rs:542-561`
- Touched files: `apps/mirror/src/score.rs`
- Status: open, valid

### #6 P2 fix(mirror): add serde(default) to ScoreResult and WeeklyRecord
- Root cause: structs currently deserialize strictly; backward compatibility for missing fields is not explicit.
- Code evidence:
- `apps/mirror/src/score.rs:81-86`
- `apps/mirror/src/weekly.rs:25-30`
- Touched files: `apps/mirror/src/score.rs`, `apps/mirror/src/weekly.rs`
- Status: open, valid

### #7 P2 fix(mirror): config.toml parse error should warn
- Root cause: parse failure path silently falls back to defaults.
- Code evidence: `apps/mirror/src/config.rs:91-96`
- Touched files: `apps/mirror/src/config.rs`
- Status: open, valid

### #8 P2 feat(mirror): rotate weekly-history.jsonl (52 week cap)
- Root cause: weekly history appends forever, no retention bound.
- Code evidence: `apps/mirror/src/weekly.rs:260-287`
- Touched files: `apps/mirror/src/weekly.rs`
- Status: open, valid

### #9 P2 refactor(mirror): reduce indicator extension cost
- Root cause: indicator definitions/targets/rendering are hardcoded in many locations.
- Code evidence:
- `apps/mirror/src/score.rs` (`layer1/layer2/layer3`, `indicator_display`, `PersonalBaseline` fields)
- `apps/mirror/src/config.rs` (`Targets` explicit fields)
- Touched files: `apps/mirror/src/score.rs`, `apps/mirror/src/config.rs` (and likely tests)
- Status: open, valid

### #10 P2 refactor(mirror): deduplicate save_to_document
- Root cause: near-identical async persistence helpers in weekly/profile.
- Code evidence:
- `apps/mirror/src/weekly.rs:290-310`
- `apps/mirror/src/profile.rs:205-220`
- Touched files: `apps/mirror/src/weekly.rs`, `apps/mirror/src/profile.rs` (+ shared helper module)
- Status: open, valid

### #11 P2 refactor(mirror): share llm_with_retry
- Root cause: retry policy implemented only in weekly; profile/advice call LLM without shared retry wrapper.
- Code evidence:
- retry helper: `apps/mirror/src/weekly.rs:312-350`
- direct calls: `apps/mirror/src/profile.rs:250-253`, `apps/mirror/src/advice.rs:132-135`
- Touched files: `apps/mirror/src/weekly.rs`, `apps/mirror/src/profile.rs`, `apps/mirror/src/advice.rs` (+ shared helper module)
- Status: open, valid

### #12 P2 fix(mirror): growth-tracker path hardcoded
- Root cause: path is hardcoded to `~/.refine/growth-tracker.json`, ignoring unified path resolution strategy.
- Code evidence: `apps/mirror/src/score.rs:658-663`
- Touched files: `apps/mirror/src/score.rs` (possibly `packages/core/src/infra/paths.rs` if helper is added)
- Status: open, valid

### #13 P2 feat(mirror): detect isatty for ANSI output support
- Root cause: ANSI sequences emitted unconditionally in signal display paths, including non-TTY output.
- Code evidence:
- `apps/mirror/src/score.rs:44-51`
- `apps/mirror/src/dashboard.rs:136-142`
- Touched files: `apps/mirror/src/score.rs`, `apps/mirror/src/dashboard.rs`
- Status: open, valid

## Skip Decisions
- None. All 13 listed issues are currently open and still reproducible in current code.
