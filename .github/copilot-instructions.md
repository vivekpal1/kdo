# GitHub Copilot Instructions — kdo

This is kdo, a Rust workspace manager for polyglot monorepos.

## Code conventions

- Rust 2021 edition, MSRV 1.75
- Error handling: `miette` for diagnostics, `thiserror` for error types, `?` for propagation
- No `unwrap()` in non-test code
- Logging: `tracing` crate, never `println!`
- CLI output: `owo-colors` for colors, `tabled` for tables
- All public functions have doc comments

## Architecture

Crates in dependency order: kdo-core -> kdo-resolver -> kdo-graph -> kdo-context -> kdo-mcp -> kdo-cli

## Testing

Run `make ci` (or `cargo clippy --all-targets -- -D warnings && cargo fmt --all --check && cargo test --all`).
