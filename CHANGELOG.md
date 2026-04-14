# Changelog

All notable changes to kdo are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Rich `kdo.toml` schema: tasks can be declared as bare commands or full specs with
  `command`, `depends_on`, `inputs`, `outputs`, `cache`, `persistent`, and `env` fields.
- Task pipelines with three `depends_on` modes:
  - `"task"` — run this project's earlier task first
  - `"^task"` — run `task` in every upstream dependency project first
  - `"//task"` — run `task` across every workspace project first
- `[aliases]` table for short task names (`b = "build"` → `kdo run b`).
- `[env]` table and `env_files` for workspace-wide environment variables, merged per task.
- `[projects.<name>]` sections for per-project task and env overrides.
- `workspace.projects` and `workspace.exclude` globs for explicit project discovery.
- Pass-through args after `--` (e.g. `kdo run build -- --release`).
- Persistent task flag for long-running processes like `dev` servers.
- `kdo init` now generates a richly-commented `kdo.toml` with language-aware defaults.
- Prefix format changed from `[project]` to `[project:task]` for clearer pipeline output.

## [0.1.0-alpha.1] - 2026-04-13

First public alpha. Expect breakage. API surface is not stable.

### Added

- `kdo init` — workspace discovery and scaffolding
- `kdo new` — interactive project scaffolding for Rust, TypeScript, Python, Solana Anchor, Go
- `kdo run <task>` — task execution in topological order, with `--parallel`
- `kdo exec <command>` — arbitrary command in each project, with `--parallel`
- `kdo list` / `kdo graph` / `kdo context` / `kdo affected`
- `kdo doctor` — workspace health check
- `kdo completions <shell>` — bash, zsh, fish, powershell
- `kdo serve` — MCP server over stdio with 7 tools:
  `kdo_list_projects`, `kdo_get_context`, `kdo_read_symbol`,
  `kdo_dep_graph`, `kdo_affected`, `kdo_search_code`, `kdo_run_task`
- `kdo.toml` workspace config (committed) and `.kdo/` cache (gitignored)
- `.kdoignore` for context exclusion rules
- Manifest parsers for Cargo.toml, package.json, pyproject.toml, Anchor.toml, go.mod
- Dependency graph via petgraph with DFS/BFS queries and cycle detection
- Blake3 content hashing (parallelized, deterministic)
- Tree-sitter signature extraction for Rust, TypeScript, Python; line-based extraction for Go
- Token-budgeted CONTEXT.md generation
- JSON output mode on all commands (`--format json`)
- `.gitignore` and `.kdoignore` support via the `ignore` crate
- Colored CLI output (owo-colors) and progress bars (indicatif)
- Integration test suite against `fixtures/sample-monorepo`

[Unreleased]: https://github.com/vivekpal1/kdo/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/vivekpal1/kdo/releases/tag/v0.1.0-alpha.1
