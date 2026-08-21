# Local Setup

Refine has two local surfaces:

- CLI tools: `refine` and `mirror`
- Local services: `refine-server`, daily ingest, weekly insights, and optional desktop UI dev server

Use the installer from the repository root:

```bash
scripts/install-local.sh
```

Session ingestion defaults to `--provider auto`: it prefers a compatible
`remem` binary on `PATH` (or an explicit `REFINE_REMEM_BIN` path), and uses the
local Claude/Codex scanner only when that executable is absent. Remem
subprocess, malformed JSON, contract, and pagination failures are ingest
failures and never silently fall back. Use `--provider remem` for strict remem
operation or `--provider local` for supported local discovery; the deprecated
`--legacy-local-scan` option remains an alias for `--provider local`.

The installer is idempotent. It can be used for first install and for upgrades from a newer checkout.

## What The Installer Does

- Installs `refine`, `mirror`, and `refine-server` with `cargo install --locked --path`.
- Copies unattended runtime scripts into `~/.refine/scripts`. The server,
  daily ingest, weekly insights, and optional cognitive portrait LaunchAgents
  execute from that installed prefix rather than from the git checkout.
- Installs desktop UI dependencies with `bun install` when Bun is available.
- Writes macOS user LaunchAgents under `~/Library/LaunchAgents/`.
- Starts or reloads the local server and UI dev service. The server uses the
  same validated LLM credential loader as scheduled jobs.
- Schedules daily ingest at 08:00 and weekly insights on Sunday at 09:00.
- Enables local dashboard/API access with `REFINE_DEV_ANON=1` by default.
- Creates `~/.refine` with user-only permissions. It never migrates or copies
  LLM credentials; when they are absent it prints the read-only configure
  command.

## Configure LLM credentials for launchd

The canonical unattended credential file is `~/.refine/llm.env`. It must be a
regular, non-symlink file owned by the current user with no group/other bits
(normally mode `0600`). Scheduled scripts do not source `~/.zshrc`.

Review a migration without changing files:

```bash
bash scripts/configure-llm-env.sh --check
```

After reviewing the redacted check, perform the one-time migration of literal
`export BASE_URL=...`, `export BASE_API_KEY=...`, and `export BASE_MODEL=...`
definitions:

```bash
bash scripts/configure-llm-env.sh --migrate
```

If the supported variables are already exported in the current shell, the
explicit alternative is:

```bash
bash scripts/configure-llm-env.sh --from-env
```

When upgrading an older checkout that used a repository `.env`, explicitly
import only its supported literal assignments before reinstalling:

```bash
bash scripts/configure-llm-env.sh --from-file .env
```

The installer stops before rewriting LaunchAgents when it detects this legacy
case. It never evaluates the source file or prints credential values.

The loader precedence is current non-empty process credentials, then the
secure file, then an explicitly supplied repository `.env` fallback. It
supports the Anthropic aliases `REFINE_ANTHROPIC_API_KEY`,
`ANTHROPIC_AUTH_TOKEN`, and `ANTHROPIC_API_KEY`; the OpenAI aliases
`REFINE_OPENAI_API_KEY` and `OPENAI_API_KEY`; and the compatibility
`BASE_API_KEY`, together with their URL/model variables. Values are never
printed by the loader, migration helper, or credential preflight.

The server health response includes `llm_configured` so `doctor-local.sh` can
detect a query-only server that would reject extraction jobs. Its append-only
logs include a timestamped startup boundary, making errors from an earlier
process distinguishable from the current run.

## Auth Modes

Default local mode:

```bash
scripts/install-local.sh
```

This writes `REFINE_DEV_ANON=1` into `com.lifcc.refine-server.plist`. It is intended for loopback local development.

Token mode:

```bash
REFINE_API_TOKEN=your-token scripts/install-local.sh --token-auth
```

Token mode writes the token to `~/.refine/refine-server.token` with mode `0600`
and passes only that file path to the shared server startup wrapper. The token
value is not written into the plist. Clients must send
`Authorization: Bearer <token>`.

## Useful Variants

Install binaries only:

```bash
scripts/install-local.sh --no-launchd
```

Install services without the UI dev server:

```bash
scripts/install-local.sh --no-ui-dev
```

This also unloads and removes an existing `com.lifcc.refine-ui-dev` LaunchAgent. Verify the same shape with:

```bash
scripts/doctor-local.sh --no-ui-dev
```

Write LaunchAgents without starting them:

```bash
scripts/install-local.sh --no-start
```

## Services

| Label | Role | Trigger | Log |
| --- | --- | --- | --- |
| `com.lifcc.refine-server` | Local API/dashboard | RunAtLoad + KeepAlive | `~/Library/Logs/refine-server.log` |
| `com.lifcc.refine-daily-ingest` | `~/.refine/scripts/daily-refresh.sh` | Daily 08:00 | `~/Library/Logs/refine-daily-ingest.log` |
| `com.lifcc.refine-weekly-insights` | `~/.refine/scripts/weekly-insights.sh` | Sunday 09:00 | `~/Library/Logs/refine-insights.log` |
| `com.lifcc.refine-cognitive-portrait` | `~/.refine/scripts/cognitive-portrait.sh` (opt-in; repository remains its workspace/output root) | Sunday 10:00 | `~/Library/Logs/refine-portrait.log` |
| `com.lifcc.refine-ui-dev` | Checkout-bound desktop UI Vite dev server | RunAtLoad + KeepAlive | `.run/launchd-refine-ui.*.log` |

## Health Check

Run:

```bash
scripts/doctor-local.sh
```

The doctor checks installed binaries, LaunchAgents, a launchd-like clean
environment credential preflight, `/health`, a protected `/v1/*` API route,
database freshness, log files, and UI dependencies. It fails when unattended
credentials are missing or the secure file has invalid ownership, type, or
permissions. Use `scripts/doctor-local.sh --no-ui-dev` for installs that
intentionally disable the desktop UI dev service.

Common symptoms:

- `/health` works but `/v1/items` returns Unauthorized: reinstall with default local mode or set a matching `REFINE_API_TOKEN`.
- `last-refresh-ok` is stale: run `~/.refine/scripts/daily-refresh.sh` once, then inspect `~/Library/Logs/refine-daily-ingest.log`.
- Doctor reports a stale runtime script: re-run `scripts/install-local.sh` from a clean checkout.
- UI dev LaunchAgent exits with `vite: not found`: run `scripts/install-local.sh` so `apps/desktop/ui/node_modules` is installed.
