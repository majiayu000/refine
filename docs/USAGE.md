# Refine Usage Guide

This guide is task-oriented. If you only remember one thing:

- Primary workflow: sync conversation knowledge from multiple AI platforms into one knowledge base.
- Then use extraction/search/recommendation/insights on top of synced data.

## 1. Typical Workflows

### A) Cross-platform knowledge sync (recommended)

Use this when your main goal is collecting chat knowledge from ChatGPT/Claude/Gemini/Grok.

1. Start server (`apps/server`)
2. Run browser extension (`apps/extension`)
3. Extract/sync conversations from supported sites
4. Search or review synced data in CLI/API

### B) Local session insights (CLI-first)

Use this when your main goal is analyzing Claude Code / Codex coding sessions.

1. `refine ingest-sessions`
2. `refine insights --prescription`
3. `mirror dashboard`

## 2. Setup

### Prerequisites

- Rust 1.75+
- Bun (for extension)
- Chromium-based browser (for loading unpacked extension)

### Install Local Stack

Recommended:

```bash
scripts/install-local.sh
scripts/doctor-local.sh
```

This installs `refine`, `mirror`, and `refine-server`, writes macOS LaunchAgents,
starts the local server, schedules daily/weekly jobs, and installs desktop UI
dependencies when Bun is available.

For details, see [LOCAL_SETUP.md](./LOCAL_SETUP.md).

### Install CLI Only

```bash
cargo install --path apps/cli
cargo install --path apps/mirror
cargo install --path apps/server
```

### Configure LLM (optional but recommended)

```bash
cat > .env << 'EOF'
REFINE_OPENAI_API_KEY=your_key
REFINE_OPENAI_BASE_URL=https://api.openai.com
REFINE_OPENAI_MODEL=gpt-4o
EOF
```

## 3. Run Sync Stack (Server + Extension)

### Start server

```bash
cargo run --package refine-server
```

Default server address: `http://127.0.0.1:21567`. If that port is occupied by another process, `refine-server` tries `21568`, `21569`, then `21570`.

For unauthenticated local dashboard/API access, set:

```bash
REFINE_DEV_ANON=1 cargo run --package refine-server
```

For token auth, set `REFINE_API_TOKEN` and send `Authorization: Bearer <token>`
from the client.

Optional server bind overrides:

```bash
REFINE_SERVER_HOST=127.0.0.1 REFINE_SERVER_PORT=21567 cargo run --package refine-server
```

### Start extension

```bash
cd apps/extension
bun install
bun run dev
```

Optional custom API base:

```bash
cd apps/extension
PLASMO_PUBLIC_REFINE_API_BASE=https://api.refine.so bun run dev
```

Build/package extension:

```bash
cd apps/extension
bun run build
bun run package
```

## 4. CLI Commands You Will Actually Use

### Session ingestion and insights

```bash
refine ingest-sessions
refine ingest-sessions --source claude
refine ingest-sessions --source codex
refine ingest-sessions --dry-run
refine insights --prescription
mirror dashboard
mirror score
```

### Knowledge and document operations

```bash
refine extract --stdin
refine search "query"
refine list
refine list --type observation
refine add --title "t" --summary "s" --type knowledge
refine show <id>
refine delete <id>
refine docs
refine doc-show <id>
refine doc-search "query"
```

## 5. API Quick Checks

After starting server:

```bash
curl http://127.0.0.1:21567/health
curl "http://127.0.0.1:21567/v1/items?cursor=0&limit=20"
curl "http://127.0.0.1:21567/v1/recommendations?q=rust&limit=5"
```

If `/health` succeeds but `/v1/items` returns Unauthorized, the server is
running without `REFINE_DEV_ANON=1` and without a matching `REFINE_API_TOKEN`.
Run `scripts/install-local.sh` for the default local setup.

## 6. Data and Paths

- Unified DB path priority:
  - `REFINE_DB_PATH`
  - entry-specific fallback (for server: `REFINE_SERVER_DB_PATH`)
  - platform default data-local path (`.../refine/refine.db`)

## 7. Where To Go Next

- Project overview: [00_OVERVIEW.md](./00_OVERVIEW.md)
- Local setup: [LOCAL_SETUP.md](./LOCAL_SETUP.md)
- Local release flow: [LOCAL_RELEASE.md](./LOCAL_RELEASE.md)
- Server details: [../apps/server/README.md](../apps/server/README.md)
- API details: [11_API_SPEC.md](./11_API_SPEC.md)
- Architecture details: [09_ARCHITECTURE.md](./09_ARCHITECTURE.md)
