# Local Setup

Refine has two local surfaces:

- CLI tools: `refine` and `mirror`
- Local services: `refine-server`, daily ingest, weekly insights, and optional desktop UI dev server

Use the installer from the repository root:

```bash
scripts/install-local.sh
```

The installer is idempotent. It can be used for first install and for upgrades from a newer checkout.

## What The Installer Does

- Installs `refine`, `mirror`, and `refine-server` with `cargo install --path`.
- Installs desktop UI dependencies with `bun install` when Bun is available.
- Writes macOS user LaunchAgents under `~/Library/LaunchAgents/`.
- Starts or reloads the local server and UI dev service.
- Schedules daily ingest at 08:00 and weekly insights on Monday at 09:00.
- Enables local dashboard/API access with `REFINE_DEV_ANON=1` by default.

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

Token mode writes `REFINE_API_TOKEN` into the server LaunchAgent. Clients must send `Authorization: Bearer <token>`.

## Useful Variants

Install binaries only:

```bash
scripts/install-local.sh --no-launchd
```

Install services without the UI dev server:

```bash
scripts/install-local.sh --no-ui-dev
```

Write LaunchAgents without starting them:

```bash
scripts/install-local.sh --no-start
```

## Services

| Label | Role | Trigger | Log |
| --- | --- | --- | --- |
| `com.lifcc.refine-server` | Local API/dashboard | RunAtLoad + KeepAlive | `~/Library/Logs/refine-server.log` |
| `com.lifcc.refine-daily-ingest` | `refine ingest-sessions` + `mirror score` | Daily 08:00 | `~/Library/Logs/refine-daily-ingest.log` |
| `com.lifcc.refine-weekly-insights` | `refine insights --prescription` | Monday 09:00 | `~/Library/Logs/refine-insights.log` |
| `com.lifcc.refine-ui-dev` | Desktop UI Vite dev server | RunAtLoad + KeepAlive | `.run/launchd-refine-ui.*.log` |

## Health Check

Run:

```bash
scripts/doctor-local.sh
```

The doctor checks installed binaries, LaunchAgents, `/health`, a protected `/v1/*` API route, database freshness, log files, and UI dependencies.

Common symptoms:

- `/health` works but `/v1/items` returns Unauthorized: reinstall with default local mode or set a matching `REFINE_API_TOKEN`.
- `last-refresh-ok` is stale: run `scripts/daily-refresh.sh` once, then inspect `~/Library/Logs/refine-daily-ingest.log`.
- UI dev LaunchAgent exits with `vite: not found`: run `scripts/install-local.sh` so `apps/desktop/ui/node_modules` is installed.
