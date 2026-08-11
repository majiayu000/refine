<h1 align="center">Refine</h1>

<p align="center">
  <a href="https://github.com/majiayu000/refine/actions/workflows/ci.yml"><img src="https://github.com/majiayu000/refine/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.75%2B-orange.svg" alt="Rust"></a>
</p>

<p align="center"><strong>Re + Fine — improve continuously, conversation by conversation.</strong></p>

<p align="center">Sync knowledge from AI conversations. Track cognitive growth from coding sessions.</p>

<p align="center"><a href="./README.zh-CN.md">中文文档</a></p>

## What It Does

1. **Knowledge Sync** — Capture conversations from ChatGPT, Claude, Gemini, Grok, Claude Code, Codex into one searchable knowledge base
2. **Session Analysis** — Extract 12 cognitive dimensions from AI coding sessions (decisions, bugs, patterns, friction, knowledge gained, etc.)
3. **Cognitive Tracking (Mirror)** — 3-layer signal lights, personal baseline, LLM-powered advice, trend tracking

## Quick Start

```bash
# Install the local stack: CLI tools, server, launchd jobs, and optional UI dev service
scripts/install-local.sh

# Configure unattended LLM credentials (review first; no values are printed)
bash scripts/configure-llm-env.sh --check
bash scripts/configure-llm-env.sh --migrate

# If supported variables are already exported in this shell, use:
# bash scripts/configure-llm-env.sh --from-env

# Set up Claude Code skills (one-time symlink)
ln -s "$(pwd)/skills/cognitive-portrait" ~/.claude/skills/cognitive-portrait
# If ~/.claude/skills/cognitive-portrait already exists as a directory (old copy),
# delete it first: rm -rf ~/.claude/skills/cognitive-portrait

# Import sessions (auto prefers remem and falls back to local when remem is absent)
refine ingest-sessions

# See your cognitive snapshot
mirror score
```

For local install, doctor, and upgrade details, see [Local Setup](docs/LOCAL_SETUP.md)
and [Local Release Flow](docs/LOCAL_RELEASE.md).

## Release Status

Refine is currently distributed as a source install from this repository. A
packaged GitHub Release for the current `0.1.3` workspace version has not been
cut yet; use `scripts/install-local.sh` from a checked-out commit for local
installation. Release notes are tracked in [CHANGELOG.md](CHANGELOG.md).

Support path: open a GitHub Issue with your OS, install method, command output,
and the relevant `~/.refine` log snippet if available.

### Current Limitations

- Local-first only: the server and extension are intended for a local trusted
  runtime unless you explicitly configure authentication and network exposure.
- LLM-backed extraction, advice, weekly reports, and profiles require a working
  OpenAI-compatible or Anthropic-compatible API key.
- Browser extension support is still a developer preview and should be tested
  against the local API before relying on it for unattended capture.
- Mirror personal baselines need enough history; before four weeks of data,
  signal lights use fixed thresholds instead of your own baseline.
- No hosted multi-user service or migration SLA is claimed by this repository.

## Mirror — Cognitive Growth Tracker

Mirror extracts cognitive fingerprints from your AI coding sessions and tracks growth over time.

### Daily Usage

```bash
mirror score                        # 3-layer signal lights + LLM advice
mirror motd                         # One-line briefing (add to .zshrc)
mirror dashboard                    # Full ASCII dashboard
mirror score --since 2026-03-20     # Filter by date
```

### Periodic Analysis

```bash
mirror weekly                       # Weekly delta report (requires LLM)
mirror profile                      # Cognitive portrait narrative (requires LLM)
/cognitive-portrait                  # Deep 5-framework analysis (~1000 lines, Claude Code skill)
```

### What Mirror Tracks

**3 Layers × 11 Indicators:**

| Layer | Indicators | What It Measures |
|-------|-----------|-----------------|
| **Depth** | Dreyfus level, Decision quality, Depth output, Knowledge rate | Are you thinking at a higher level? |
| **Breadth** | Exploration rate, Deep invest, Fragmentation | Are you investing wisely across projects? |
| **Collaboration** | Delegation rate, Mode diversity, Bug/decision ratio, Friction density | Is your AI collaboration healthy? |

**Signal Lights:** 🟢 Green (healthy) / 🟡 Yellow (watch) / 🔴 Red (act now)

**Personal Baseline:** After 4 weeks, signals are relative to your own average, not fixed thresholds.

### Terminal Integration

```bash
# Add to .zshrc — shows signal lights every time you open terminal
[ -x "$(command -v mirror)" ] && { mkdir -p ~/.refine && mirror motd 2>> ~/.refine/hooks-error.log; }
```

**StatusLine** (Claude Code bottom bar):
```
本周243 深度🟢 广度🔴 协作🔴 每周开1次新方向探索
```

**SessionStart hook** injects cognitive dashboard + LLM advice into every Claude Code conversation.

### Automation (launchd)

The local installer writes and reloads these jobs:

```bash
scripts/install-local.sh
scripts/doctor-local.sh
```

| Schedule | Task | What It Does |
|----------|------|-------------|
| Daily 8:00 AM | `scripts/daily-refresh.sh` | `refine ingest-sessions` → `mirror score` → writes `~/.refine/last-refresh-ok` |
| Weekly Sunday 09:00 | `scripts/weekly-insights.sh` | `refine insights --prescription` (10-way LLM) |

### LLM credential loading

Unattended jobs use `~/.refine/llm.env` (mode `0600`, owned by the current
user) and never source `~/.zshrc`. Loading order is: non-empty credentials
already in the process, the secure user file, then an explicit repository
`.env` fallback used by the daily and weekly scripts for development. Lower
priority sources never replace an already-set credential.

The migration helper accepts only literal `export BASE_URL=...`,
`export BASE_API_KEY=...`, and `export BASE_MODEL=...` definitions. It creates
a private backup under `~/.refine/backups`, removes only those definitions,
and adds one managed source block for interactive shells. `--check` is a
read-only dry run; `--migrate` is the explicit write operation. The installer
does not copy or migrate secrets. Run `scripts/doctor-local.sh` after
configuration to verify the same clean-environment preflight used by launchd.

For a manual development-only fallback, a repository `.env` may contain
supported variables such as `REFINE_OPENAI_API_KEY`,
`REFINE_OPENAI_BASE_URL`, and `REFINE_OPENAI_MODEL`; scheduled scripts pass
that path explicitly to the loader rather than sourcing it automatically.

### Configuration

```toml
# ~/.mirror/config.toml (optional, all have defaults)
[targets]
delegation_green = 0.40      # delegation < 40% = green
exploration_green = 0.15     # exploration > 15% = green
knowledge_green = 0.5        # knowledge rate > 0.5/session = green
friction_green = 1.0         # friction < 1.0/session = green
```

### Data Flow

```
remem raw sessions/messages         ← preferred Claude Code + Codex raw archive
    │
    ▼ refine ingest-sessions        (auto: remem, or local discovery if remem is absent)
    │
SQLite (observations, documents)    ← Shared data store
    │
    ├─ mirror score/dashboard       (local clustering → signal lights)
    ├─ mirror motd                  (reads cached scores + LLM advice)
    ├─ mirror weekly                (delta analysis via LLM)
    ├─ mirror profile               (cognitive portrait via LLM)
    └─ /cognitive-portrait          (5-framework deep analysis, Claude Code skill)
```

## Refine CLI Commands

`refine ingest-sessions` defaults to `--provider auto`: it prefers a compatible
`remem` binary on `PATH` (or the path in `REFINE_REMEM_BIN`) and falls back to
the local Claude/Codex session scanner only when that executable is absent.
`--provider remem` is strict and fails visibly on subprocess, JSON, contract,
or pagination errors; those errors never trigger a local fallback.
`--provider local` is a supported provider and accepts `--source claude|codex`.
The deprecated `--legacy-local-scan` flag remains an alias for
`--provider local`. A matching local path-keyed Document/items and its remem
replacement facets are changed in one transaction; ambiguous legacy identity
fails closed and the remem identity is reused instead of creating a second
active facet set.

### Session Analysis

```bash
refine ingest-sessions                  # auto: remem, then local only if remem is absent
refine ingest-sessions --provider remem # Strict remem raw archive provider
refine ingest-sessions --provider local # Supported local Claude/Codex scanner
refine ingest-sessions --provider local --source claude
refine ingest-sessions --latest 20      # Most recent sessions from the selected provider
refine ingest-sessions --dry-run        # Preview without LLM calls
refine ingest-sessions --legacy-local-scan --source claude
                                        # Deprecated alias for --provider local
refine insights --prescription          # L1-L4 cognitive report
mirror dashboard                        # Cognitive growth dashboard
```

### Knowledge Management

```bash
refine search "query"              # Search knowledge base
refine list --type observation     # List cognitive observations
refine show <id>                   # View item details
refine docs                        # List session documents
```

## Extension Preview

![Refine Browser Extension Dashboard](docs/images/extension-dashboard.png)

## Browser Extension (Plasmo)

```bash
cargo run --package refine-server  # Start API server
cd apps/extension && bun install && bun run dev
```

## Architecture

```
refine/
├── packages/core/          # Shared: SQLite, LLM client, session analysis, knowledge
├── apps/
│   ├── cli/                # refine command (ingest, insights, search, growth)
│   ├── mirror/             # mirror command (score, motd, dashboard, weekly, profile)
│   │   └── src/score/      # Signal light engine (11 indicators, personal baseline)
│   ├── server/             # API server (Axum)
│   ├── desktop/            # Desktop app (Tauri)
│   └── extension/          # Browser extension (Plasmo)
└── scripts/
    ├── daily-refresh.sh    # Daily: ingest + mirror score
    └── weekly-insights.sh  # Weekly: full LLM analysis
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Core | Rust |
| Database | SQLite + FTS5 |
| LLM | OpenAI-compatible API |
| Terminal | unicode-width, ANSI (isatty-aware) |
| Desktop | Tauri 2.0 |
| Extension | Plasmo |

## License

MIT
