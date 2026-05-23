# Local Release Flow

Use the local release flow when promoting a checkout to the machine-local Refine install.

```bash
scripts/release-local.sh
```

The command is intentionally conservative:

1. Requires a clean working tree unless `--allow-dirty` is passed.
2. Runs the port/default contract check.
3. Runs Rust format, check, tests, and clippy.
4. Builds the desktop UI when Bun is available.
5. Runs `scripts/install-local.sh`.
6. Runs `scripts/doctor-local.sh` as a smoke check.

## Upgrade From A Branch

```bash
git switch main
git pull --ff-only
scripts/release-local.sh
```

For a feature branch under active development:

```bash
scripts/release-local.sh --allow-dirty
```

Use `--allow-dirty` only when validating a local change before commit.

## Faster Inner Loop

Skip slower checks only while iterating:

```bash
scripts/release-local.sh --allow-dirty --skip-tests --skip-clippy --skip-ui --skip-install
```

Before publishing a PR or declaring a local release complete, run the full command again.

## CI Relationship

GitHub CI remains the repository gate for pull requests. The local release flow is the machine install gate: it proves this checkout can be built, installed, reloaded, and smoke-tested on the current macOS user account.

## Issue Tracking

This flow addresses:

- #112 local one-command installer
- #115 local doctor
- #114 reproducible local release and upgrade flow
- #113 consolidated local setup documentation
