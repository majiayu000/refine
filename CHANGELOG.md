# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-03-23

### Added — Mirror CLI (`apps/mirror/`)
- **mirror score**: 3-layer signal lights (Depth/Breadth/Collaboration) with 11 indicators
- **mirror motd**: One-line briefing with signal lights + LLM advice, integrated into .zshrc
- **mirror dashboard**: Full ASCII dashboard with cognitive/collaboration distributions
- **mirror weekly**: Weekly delta analysis with LLM (this week vs last week)
- **mirror profile**: Cognitive portrait narrative via LLM
- Bilingual support (en/zh) with `--lang` flag, default Chinese
- Personal baseline: 4-week sliding window replaces fixed thresholds after sufficient history
- `--since YYYY-MM-DD` time filter for score and dashboard
- New indicators: knowledge_rate (L1) and friction_density (L3)
- Recovery feedback: celebratory message when a layer recovers from red
- Pending ingest detection: warns when sessions are unanalyzed
- LLM advice caching with short (≤10 chars) + full versions, 72h TTL
- scores.jsonl auto-rotation (365 entry cap)
- Trend arrows (↑↓) in motd comparing current vs previous signals
- Data staleness detection (48h warning)

### Added — Skills
- `/cognitive-portrait`: Deep 5-framework analysis (~1000 lines) using Dreyfus/Bloom/double-loop learning/metacognition/knowledge structure

### Added — Automation
- `scripts/daily-refresh.sh`: Daily ingest + mirror score (launchd, 8:00 AM)
- Weekly insights automation (launchd, Monday 9:00 AM)
- StatusLine integration: `本周N 深度🟢 广度🔴 协作🟡 短建议`
- SessionStart hook: injects cognitive dashboard + LLM advice into conversation context
- Stop hook: tracks real session count from JSONL files + collaboration modes from DB

### Fixed — Audit (13 issues resolved via Harness)
- PersonalBaseline avg divisor bug (divided by total instead of matched count)
- Silent error swallowing in JSONL parsing (scores, weekly, advice)
- score.rs split from 1295 lines into 7 sub-modules
- Signal→string conversion deduplicated (5 copies → centralized)
- persist_score made atomic (fs2 file lock + temp file rename)
- Added serde(default) to ScoreResult and WeeklyRecord
- Config parse error now warns instead of silently using defaults
- Weekly-history.jsonl rotate (52 week cap)
- Indicator extension cost reduced via declarative registry
- save_to_document deduplicated across weekly/profile
- llm_with_retry shared across weekly/profile/advice
- Growth-tracker path resolved from DB location instead of hardcoded
- isatty detection for ANSI output (no garbled pipe output)

### Fixed — Data Pipeline
- Session discovery: scans nested UUID directories in Claude Code projects
- Project clustering: picks most specific tag (longest path) instead of first
- Atomic items (decision/bugfix) inherit project from session summary via document_id
- Filter threshold: min_user_messages lowered from 2 to 1 (recovered ~649 sessions)

### Changed
- Default language changed to Chinese (zh)
- Box width unified to 76 for both languages
- display_width uses unicode-width crate instead of hand-written is_wide()

## [0.0.1] - 2026-03-18

### Added
- Initial refine CLI: ingest-sessions, insights, growth, search, list, show
- Browser extension (Plasmo) for ChatGPT/Claude/Gemini/Grok
- API server (Axum)
- Desktop app (Tauri)
- 12-dimension facet extraction from AI coding sessions
- 10-way concurrent LLM analysis for insights reports
- SQLite + FTS5 knowledge base
