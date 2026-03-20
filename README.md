# Refine

[![CI](https://github.com/majiayu000/refine/actions/workflows/ci.yml/badge.svg)](https://github.com/majiayu000/refine/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

> Smart knowledge reuse engine — turn every AI conversation into a reusable asset

Extract knowledge from AI conversations (Claude Code, Codex, ChatGPT), get proactive recommendations when you need them.

[中文文档](./README.zh-CN.md)

## Features

- **Knowledge Extraction** — Auto-extract knowledge cards, skills, and code snippets from AI conversations
- **Full-Text Search** — SQLite FTS5 powered, supports mixed CJK/Latin search
- **Document Management** — Raw content storage with linked knowledge tracing
- **Session Insights** — Analyze Claude Code / Codex sessions for cognitive growth patterns
- **Growth Tracking** — Terminal motd, CLI dashboard, Claude Code statusline
- **Multi-Platform** — CLI / Desktop (Tauri) / Browser Extension / API Server

## Quick Start

```bash
# Install
cargo install --path apps/cli

# Configure LLM (.env file, supports any OpenAI-compatible API)
cat > .env << 'EOF'
REFINE_OPENAI_API_KEY=your_key
REFINE_OPENAI_BASE_URL=https://api.openai.com
REFINE_OPENAI_MODEL=gpt-4o
EOF

# Ingest your AI coding sessions
refine ingest-sessions

# Generate cognitive insights report
refine insights --prescription

# View growth dashboard
refine growth
```

## CLI Commands

### Session Insights

```bash
refine ingest-sessions                  # Ingest all sessions (incremental, skips processed)
refine ingest-sessions --source claude  # Claude Code only
refine ingest-sessions --limit 100      # Limit count
refine ingest-sessions --dry-run        # Preview without LLM calls

refine insights                         # Generate L1-L3 report
refine insights --prescription          # Include L4 growth prescription

refine growth                           # Cognitive dashboard
refine explore                          # Tag an exploration session
refine deep-inquiry                     # Tag a deep thinking session
```

### Knowledge Management

```bash
refine extract --stdin                  # Extract knowledge from stdin
refine search "query"                   # Search knowledge base
refine list                             # List all knowledge items
refine list --type observation          # List cognitive observations
refine show <id>                        # View details
refine docs                             # List session documents
refine doc-show <id>                    # View session/report details
```

## Growth Dashboard

`refine growth` output:

```
╔══════════════════════════════════════════════════════╗
║               Cognitive Growth Dashboard             ║
╠══════════════════════════════════════════════════════╣
║ Sessions: 824  Observations: 9740                    ║
╠══════════════════════════════════════════════════════╣
║ Cognitive Level                                      ║
║  expert      █░░░░░░░░░  11.5% ( 95)                ║
║  proficient  ███░░░░░░░  34.1% (281)                ║
║  competent   ████░░░░░░  39.8% (328)                ║
╠══════════════════════════════════════════════════════╣
║ Collaboration Mode                                   ║
║  delegation  █████░░░░░  45.5% (375)                ║
║  deep_inq    ██░░░░░░░░  18.8% (155)                ║
║  exploration ██░░░░░░░░  16.5% (136)                ║
╠══════════════════════════════════════════════════════╣
║ Key Metrics                                          ║
║  exploration   16.5%  target: >15%   ✓               ║
║  delegation    45.5%  target: <40%   ✗               ║
║  expert rate   11.5%  target: >15%   ✗               ║
╚══════════════════════════════════════════════════════╝
```

## Architecture

```
Claude Code / Codex session files (.jsonl)
    │
    ▼ refine ingest-sessions
    Parse → Filter → 12-dimension facet extraction → SQLite
    (3-way concurrent, resumable, exponential backoff retry)
    │
    ▼ refine insights
    Local clustering (by project) → 10-way concurrent LLM analysis → Merged report
    │
    ▼ 3-layer continuous tracking
    Terminal motd | refine growth | Claude Code statusline
```

### 12 Extracted Dimensions

| Dimension | Description |
|-----------|-------------|
| decisions | Technical decisions with rationale |
| bugs_fixed | Bug root cause + fix approach |
| patterns | Reusable code patterns |
| friction | AI mistakes, blockers, misdirections |
| project_progress | What was accomplished |
| questions | Questions asked (reflects knowledge boundaries) |
| knowledge_gained | New things learned |
| tools_discovered | New tools/libraries discovered |
| architecture | Architecture design and data flow |
| code_artifacts | Key code output |
| cognitive_level | novice → expert (Dreyfus model) |
| collaboration_mode | delegation / exploration / deep_inquiry / ... |

## Project Structure

```
refine/
├── packages/core/src/
│   ├── knowledge/          # Knowledge management (Item, Document, Repository)
│   ├── refinement/         # Knowledge extraction (Conversation, Extractor)
│   ├── session/            # Session analysis
│   │   ├── discovery.rs        # Session file discovery
│   │   ├── parser.rs           # JSONL parsing (Claude Code + Codex)
│   │   ├── facets.rs           # 12-dimension facet extraction
│   │   ├── clustering.rs       # Local clustering (group by project)
│   │   ├── analysis_routes.rs  # 10-way LLM analysis routes
│   │   └── report.rs           # Report merging
│   ├── search/             # Search engine (FTS5)
│   └── infra/              # Infrastructure (SQLite, LLM client)
├── apps/
│   ├── cli/                # CLI tool (refine command)
│   ├── server/             # API server (Axum)
│   └── desktop/            # Desktop app (Tauri)
└── scripts/
    ├── weekly-insights.sh       # Daily auto-analysis (launchd)
    └── reset-weekly-tracker.sh  # Weekly counter reset
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Core | Rust |
| Database | SQLite + FTS5 |
| LLM | OpenAI-compatible API (custom base_url supported) |
| Desktop | Tauri 2.0 |
| Browser Extension | Plasmo |

## License

MIT

