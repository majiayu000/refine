# Contributing to Refine

Thanks for your interest in contributing!

## Development Setup

```bash
# Clone
git clone https://github.com/majiayu000/refine.git
cd refine

# Build
cargo build

# Run tests
cargo test

# Run CLI
cargo run --bin refine -- --help
```

## Guidelines

- Follow existing code style
- Add tests for new features
- Keep commits atomic — one change per commit
- Commit messages: `<type>: <description>` (feat/fix/refactor/docs/test/chore)

## Pull Requests

1. Fork the repo and create your branch from `main`
2. Make your changes
3. Ensure `cargo check` and `cargo test` pass
4. Submit a PR with a clear description

## Reporting Issues

Use [GitHub Issues](https://github.com/majiayu000/refine/issues) with:
- Steps to reproduce
- Expected vs actual behavior
- OS and Rust version
