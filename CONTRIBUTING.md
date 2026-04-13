# Contributing to kdo

Thanks for your interest in contributing! This document covers the basics.

## Development setup

```bash
git clone https://github.com/vivekpal1/kdo.git
cd kdo
cargo build
cargo test --all
```

### Prerequisites

- Rust 1.75+ (stable)
- Git

## Code standards

- Zero `unwrap()` in non-test code. Use `miette` for errors, `?` for propagation.
- Zero `println!` for logging. Use `tracing` with `tracing-subscriber`.
- Every public function has a doc comment.
- `cargo clippy --all-targets -- -D warnings` must pass.
- `cargo fmt --check` must pass.

## Pull requests

1. Fork the repo and create a branch from `main`.
2. If you added code, add tests.
3. Ensure `cargo test --all` passes.
4. Ensure `cargo clippy --all-targets -- -D warnings` passes.
5. Ensure `cargo fmt --check` passes.
6. Open a PR with a clear description of the change.

## Architecture

```
kdo-core       → types, errors, token estimator
kdo-resolver   → manifest parsers (Cargo, Node, Python, Anchor)
kdo-graph      → workspace graph (petgraph, discovery, hashing)
kdo-context    → tree-sitter extraction, CONTEXT.md generation
kdo-mcp        → MCP server (JSON-RPC 2.0 over stdio)
kdo-cli        → clap subcommands, tabled output
```

Crates are published in dependency order: core -> resolver -> graph -> context -> mcp.

## Reporting bugs

Open an issue on GitHub with:

- What you expected to happen
- What actually happened
- Steps to reproduce
- `kdo --version` output

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
