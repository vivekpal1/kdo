# Changelog

All notable changes to kdo are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0-alpha.1] - 2026-04-16 (staged)

### Added — agent runtime

- **Agent profiles** (`kdo serve --agent <claude|openclaw|generic>`) that tune
  the default context budget, loop-detection window, max tool-output tokens, and
  tool-description verbosity per agent.
- **Loop detection** — every `tools/call` is gated by a sliding-window
  `LoopGuard`. Three identical `(tool, args)` calls (under the OpenClaw profile)
  or five (Claude/Generic) return a structured error instead of silently burning
  tokens. A separate thrash detector catches high-frequency distinct calls.
- **rmcp 0.16 migration.** `crates/kdo-mcp` is now built on the official rmcp
  SDK using `#[tool_router]` + `#[tool]` macros + `ServerHandler`. All seven
  tools and resource endpoints preserved; wire protocol unchanged.
- **`kdo setup claude` / `kdo setup openclaw`** — one-command wiring for both
  agents, with `--global` (user-level install) and `--dry-run` (print every
  file + command, touch nothing).
  - Claude: shells out to `claude mcp add --scope <user|local>`, merges a kdo
    block into `CLAUDE.md` between `<!-- kdo:start -->` / `<!-- kdo:end -->`
    sentinels, seeds `.kdo/memory/MEMORY.md`, creates `.kdo/agents/claude/`.
  - OpenClaw: writes an AgentSkills-spec `SKILL.md`, merges the MCP registration
    into `~/.openclaw/openclaw.json` at `/mcpServers/kdo` via JSON Pointer
    (plain JSON, no comments), seeds `AGENTS.md` and shared memory.
- **`kdo bench`** — reproducible benchmark, two modes:
  - **proxy** (default): measures `baseline` (bytes an fs-walking agent would
    read) vs `kdo` (bytes actually returned by `kdo_list_projects` +
    `kdo_get_context`). Apples-to-apples, no mocking.
  - **`--from-log <path>`**: parses a Claude Code session JSONL and reports
    observed input/output/cache-read token totals.
  - Results persisted at `.kdo/bench/results/<timestamp>.json`. Task definitions
    in `.kdo/bench/tasks.toml` (scaffolded on first run).

### Changed

- Idempotency is a first-class invariant of `kdo setup`: re-running converges.
  Markdown files are updated in-place between sentinels; JSON files are parsed,
  merged at a JSON Pointer, and re-serialized valid.

### Tooling

- Atomic writes (`write_to_temp_then_rename`) for every file `kdo setup` emits.
- New shared `AgentProfile` enum with `FromStr`/`Display`, parsed once at the
  CLI boundary.

### Fixed (carried from post-alpha.3 work)

- Project-discovery globs honor `/` as a literal separator so `packages/*`
  matches only direct children (pnpm/turbo semantics). Previously `packages/*`
  incorrectly matched nested paths like `packages/a/b/c`.
- README documents the `--version` requirement for pre-release `cargo install kdo`.


## [0.1.0-alpha.3] - 2026-04-14

### Added

- `kdo upgrade [--version X] [--dry-run]` — self-update the binary from the latest
  GitHub release (or a pinned version) with atomic replace + rollback on failure.
- `kdo run --dry-run` — print the resolved task pipeline without executing.
- `kdo similar <project>` — find structurally similar projects by language + shared
  dependencies (Jaccard score + same-language bonus).
- `kdo source <symbol>` — look up a symbol's definition across all workspace source
  files, respecting `.gitignore` / `.kdoignore`.
- pnpm workspace support: `pnpm-workspace.yaml` `packages:` globs (including `!exclude`
  entries) are honored during project discovery, merged with `kdo.toml` filters.
- MCP resource endpoints — context files under `.kdo/context/` are now exposed via
  `resources/list` and `resources/read` as `kdo://context/<project>` URIs. MCP clients
  can attach them directly without calling `kdo_get_context`.

### Changed

- `resources` capability is now advertised in the MCP `initialize` response.

### Tests

- Property tests for the `pnpm-workspace.yaml` parser (proptest).

## [0.1.0-alpha.2] - 2026-04-14

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

### Changed

- Task output prefix changed from `[project]` to `[project:task]` for clearer pipeline logs.
- Release workflow is now idempotent — already-published crates are skipped.
- Removed redundant `rustsec/audit-check` job; `cargo-deny` handles RustSec advisories.

### Fixed

- `deny.toml` migrated to cargo-deny v2 schema (`version = 2`).

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

[Unreleased]: https://github.com/vivekpal1/kdo/compare/v0.2.0-alpha.1...HEAD
[0.2.0-alpha.1]: https://github.com/vivekpal1/kdo/compare/v0.1.0-alpha.3...v0.2.0-alpha.1
[0.1.0-alpha.3]: https://github.com/vivekpal1/kdo/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/vivekpal1/kdo/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/vivekpal1/kdo/releases/tag/v0.1.0-alpha.1
