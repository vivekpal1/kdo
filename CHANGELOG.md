# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-04-13

### Added

- Workspace discovery with parallel manifest parsing
- Manifest parsers for Cargo.toml, package.json, pyproject.toml, Anchor.toml
- Dependency graph via petgraph with DFS/BFS queries
- Cycle detection with diagnostic reporting
- Blake3 content hashing (parallelized, deterministic)
- Tree-sitter signature extraction for Rust, TypeScript, Python
- Token-budgeted CONTEXT.md generation
- MCP server (JSON-RPC 2.0 over stdio) with 5 tools
- CLI with `init`, `list`, `graph`, `context`, `affected`, `serve` commands
- JSON output mode on all commands (`--format json`)
- `.gitignore` and `.kdoignore` support via the `ignore` crate

[0.1.0]: https://github.com/vivekpal1/kdo/releases/tag/v0.1.0
