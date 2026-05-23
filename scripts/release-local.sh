#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/release-local.sh [OPTIONS]

Run the local release/upgrade flow:
  1. verify repository state
  2. run Rust and UI checks
  3. install/upgrade local services
  4. run doctor smoke checks

Options:
  --allow-dirty      Allow running with uncommitted changes.
  --skip-tests       Skip cargo test.
  --skip-clippy      Skip cargo clippy.
  --skip-ui          Skip desktop UI install/build checks.
  --skip-install     Do not run scripts/install-local.sh.
  --no-ui-dev        Pass --no-ui-dev through to install-local.sh.
  -h, --help         Show this help.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
allow_dirty=0
skip_tests=0
skip_clippy=0
skip_ui=0
skip_install=0
no_ui_dev=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --allow-dirty)
      allow_dirty=1
      ;;
    --skip-tests)
      skip_tests=1
      ;;
    --skip-clippy)
      skip_clippy=1
      ;;
    --skip-ui)
      skip_ui=1
      ;;
    --skip-install)
      skip_install=1
      ;;
    --no-ui-dev)
      no_ui_dev=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

run() {
  printf '\n[release-local] %s\n' "$*"
  "$@"
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "[release-local] ERROR: missing required command: $1" >&2
    exit 1
  }
}

need_cmd cargo
need_cmd git

cd "$repo_root"

if [[ "$allow_dirty" != "1" && -n "$(git status --porcelain)" ]]; then
  echo "[release-local] ERROR: working tree is dirty. Commit/stash changes or pass --allow-dirty." >&2
  git status --short >&2
  exit 1
fi

run bash scripts/check_port_contract.sh
run cargo fmt --all -- --check
run cargo check --workspace
if [[ "$skip_tests" != "1" ]]; then
  run cargo test --workspace
fi
if [[ "$skip_clippy" != "1" ]]; then
  run cargo clippy --workspace -- -D warnings
fi

if [[ "$skip_ui" != "1" ]]; then
  if command -v bun >/dev/null 2>&1 && [[ -d apps/desktop/ui ]]; then
    run bash -lc 'cd apps/desktop/ui && bun install && bun run build'
  else
    echo "[release-local] WARN: Bun or desktop UI missing; skipped UI build" >&2
  fi
fi

if [[ "$skip_install" != "1" ]]; then
  install_args=()
  if [[ "$no_ui_dev" == "1" || "$skip_ui" == "1" ]]; then
    install_args+=(--no-ui-dev)
  fi
  run scripts/install-local.sh "${install_args[@]}"
fi

run scripts/doctor-local.sh

printf '\n[release-local] local release flow completed\n'
