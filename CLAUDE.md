# kdo — Workspace Manager

This is the kdo monorepo. A Rust-based polyglot workspace manager for AI agents.

## Project structure

- `crates/kdo-core` — types, errors, token estimator
- `crates/kdo-resolver` — manifest parsers (Cargo, Node, Python, Anchor)
- `crates/kdo-graph` — workspace graph (petgraph, discovery, hashing)
- `crates/kdo-context` — tree-sitter extraction, context generation
- `crates/kdo-mcp` — MCP server (JSON-RPC 2.0 over stdio)
- `crates/kdo-cli` — CLI entry point with all commands
- `fixtures/sample-monorepo` — test fixture (Anchor + TS + Python workspace)

## Code standards

- Zero `unwrap()` in non-test code. Use `miette` for errors, `?` for propagation.
- Zero `println!` for logging. Use `tracing` for debug/info, `eprintln!` with `owo-colors` for CLI output.
- `cargo clippy --all-targets -- -D warnings` must pass.
- `cargo fmt --all --check` must pass.

## Commands

```bash
make ci          # Run full CI locally (fmt + lint + test + doc)
make fixture     # Run full demo on sample-monorepo
make install     # Build and install to /usr/local/bin
```

## kdo skill

See `skills/kdo/SKILL.md` for the agent skill that teaches how to use kdo.
