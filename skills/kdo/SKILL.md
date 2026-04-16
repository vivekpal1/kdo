---
name: kdo
description: |
  Workspace manager for polyglot monorepos. Use when the user wants to initialize a workspace,
  create projects, run tasks, view dependency graphs, generate AI-optimized context, or manage
  a monorepo with Rust, TypeScript, Python, or Solana Anchor projects. Triggers on: "kdo",
  "workspace", "monorepo", "init workspace", "new project", "run build", "run test",
  "dependency graph", "context for", "affected projects", "workspace health".
---

# kdo — Workspace Manager for AI Agents

You are helping a user manage a polyglot monorepo with **kdo**, a Rust-based workspace tool
that provides dependency graphs, task execution, and AI-optimized context via MCP.

## When to use this skill

- User mentions "kdo", "workspace", or "monorepo"
- User wants to initialize, scaffold, or manage a multi-project repository
- User wants to run tasks (build/test/lint) across projects
- User asks about dependencies between projects
- User wants context or summaries of projects for AI consumption
- User mentions MCP server setup for workspace intelligence

## Installation

### From source (requires Rust toolchain)

```bash
cargo install --git https://github.com/vivekpal1/kdo
```

### Via install script

```bash
# Build from source
curl -fsSL https://vivekpal1.github.io/kdo/install.sh | bash

# Or download prebuilt binary
curl -fsSL https://vivekpal1.github.io/kdo/install.sh | bash -s -- --from-release
```

### Verify installation

```bash
kdo --version
```

If `kdo` is not installed, guide the user through installation using one of the methods above.

## Core workflow

### 1. Initialize a workspace

```bash
kdo init
```

**On an empty directory:** Creates `kdo.toml`, `.kdo/` cache, `.kdoignore`, and updates `.gitignore`.
Tells the user to run `kdo new <name>` to add their first project.

**On an existing repo:** Discovers all projects by scanning manifests (`Cargo.toml`, `package.json`,
`pyproject.toml`, `Anchor.toml`), builds the dependency graph, generates context files into
`.kdo/context/`, and creates `kdo.toml` with detected tasks.

**What gets created:**

| File/Dir | Purpose | Committed? |
|----------|---------|------------|
| `kdo.toml` | Workspace config + task definitions | Yes |
| `.kdo/` | Cache: context files, graph snapshot | No (gitignored) |
| `.kdo/context/*.md` | Per-project AI-readable context | No |
| `.kdo/graph.json` | Cached dependency graph | No |
| `.kdoignore` | Files excluded from context generation | Yes |
| `.gitignore` | Updated with language-specific patterns | Yes |

### 2. Create a new project

```bash
kdo new my-service
```

Interactive prompts:
- **Language:** rust / typescript / python / anchor
- **Type:** library / binary
- **Framework** (language-specific):
  - TypeScript: none / react / next
  - Python: none / fastapi / cli
  - Anchor: automatic

Creates a complete project skeleton with manifest, source files, and tests.
Automatically updates `.kdo/context/` after creation.

### 3. Run tasks

```bash
# Run a task across all projects
kdo run build
kdo run test
kdo run lint

# Run only in a specific project
kdo run test --filter vault-program
```

Task resolution order:
1. Project manifest scripts (e.g., `package.json` scripts)
2. `kdo.toml` `[tasks]` section
3. Language built-in defaults:
   - **Rust:** `cargo build`, `cargo test`, `cargo clippy`
   - **TypeScript:** `npm run build`, `npm test` (auto-detects bun/pnpm/yarn)
   - **Python:** `python3 -m pytest`, `ruff check .`, `ruff format .`

### 4. Execute arbitrary commands

```bash
# Run a command in every project directory
kdo exec "ls src"
kdo exec "wc -l **/*.rs" --filter vault-program
```

### 5. View workspace

```bash
# List all projects
kdo list
kdo list --format json

# Dependency graph
kdo graph
kdo graph --format json
kdo graph --format dot | dot -Tsvg > graph.svg
```

### 6. Generate context

```bash
# Get AI-optimized context for a project
kdo context vault-program --budget 2048

# JSON format for programmatic use
kdo context vault-program --format json
```

Context includes: project summary, public API signatures (extracted via tree-sitter),
dependency list. Enforces a token budget — truncates with a visible marker when exceeded.

### 7. Detect changes

```bash
# Projects affected since a git ref
kdo affected --base main
kdo affected --base HEAD~5 --format json
```

### 8. Health check

```bash
kdo doctor
```

Validates: `kdo.toml` exists and parses, `.kdo/` cache present, `.kdoignore` present,
`.gitignore` includes `.kdo/`, no circular dependencies, all context files up to date,
git status.

### 9. Shell completions

```bash
# Generate and install completions
kdo completions zsh >> ~/.zshrc
kdo completions bash >> ~/.bashrc
kdo completions fish > ~/.config/fish/completions/kdo.fish
```

## MCP server setup

kdo exposes workspace intelligence via the Model Context Protocol. Set up for your agent:

### Claude Code

Add to `~/.claude/mcp_servers.json` or `.claude/mcp_servers.json`:

```json
{
  "mcpServers": {
    "kdo": {
      "command": "kdo",
      "args": ["serve", "--transport", "stdio"]
    }
  }
}
```

### Cursor

Add to `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "kdo": {
      "command": "kdo",
      "args": ["serve", "--transport", "stdio"]
    }
  }
}
```

### Other agents

Any agent supporting MCP over stdio can use kdo. Start the server with:

```bash
kdo serve --transport stdio
```

### Available MCP tools

| Tool | Description | When to use |
|------|-------------|-------------|
| `kdo_list_projects` | List all projects with summaries | Orient yourself in the workspace |
| `kdo_get_context` | Token-budgeted context bundle | Understand a specific project's API |
| `kdo_read_symbol` | Read a function/struct/trait body | Need exact implementation details |
| `kdo_dep_graph` | Dependency closure or dependents | Understand what depends on what |
| `kdo_affected` | Projects changed since git ref | Scope your changes |
| `kdo_search_code` | Search pattern across all source | Find usage of a symbol or pattern |

**Recommended agent workflow:**
1. Call `kdo_list_projects` first to orient
2. Call `kdo_get_context` for the project you need to work on
3. Use `kdo_read_symbol` only when you need a specific function body
4. Use `kdo_search_code` to find cross-project usage
5. Use `kdo_dep_graph` to understand blast radius before changes

## kdo.toml reference

```toml
[workspace]
name = "my-monorepo"

[tasks]
build = "cargo build"
test = "cargo test"
lint = "cargo clippy -- -D warnings"
dev = "cargo watch -x run"
deploy = "scripts/deploy.sh"
```

Tasks defined here are workspace-level defaults. Project-specific tasks (from `package.json`
scripts, etc.) take priority.

## .kdoignore reference

Works like `.gitignore`. Controls which files kdo excludes from context generation and
content hashing. Default template:

```
node_modules/
target/
dist/
build/
__pycache__/
.git/
.kdo/
*.lock
```

## File structure

```
my-monorepo/
├── kdo.toml              # Workspace config (committed)
├── .kdoignore            # Context exclusion rules (committed)
├── .kdo/                 # Cache directory (gitignored)
│   ├── config.toml       # Internal config
│   ├── cache/            # Content hash cache
│   ├── context/          # Per-project context files
│   │   ├── project-a.md
│   │   └── project-b.md
│   └── graph.json        # Cached dependency graph
├── project-a/
│   ├── Cargo.toml
│   └── src/
├── project-b/
│   ├── package.json
│   └── src/
└── ...
```

## Supported languages

| Language | Manifest | Signature extraction | Task defaults |
|----------|----------|---------------------|---------------|
| Rust | `Cargo.toml` | `pub fn`, `pub struct`, `pub enum`, `pub trait` | cargo build/test/clippy |
| TypeScript | `package.json` + `tsconfig.json` | `export function/class/interface/type` | npm/bun/pnpm/yarn |
| JavaScript | `package.json` | `export function/class/const` | npm/bun/pnpm/yarn |
| Python | `pyproject.toml` | `def`, `class`, top-level annotations | pytest, ruff |
| Solana Anchor | `Anchor.toml` | Same as Rust | cargo build/test |

## Troubleshooting

**`kdo init` finds 0 projects:**
Ensure manifest files exist (`Cargo.toml`, `package.json`, `pyproject.toml`, or `Anchor.toml`)
in project directories. kdo skips `node_modules/`, `target/`, `.git/`, `.kdo/`, `dist/`, `build/`.

**`kdo run` fails for a project:**
Check that the task exists in the project manifest or `kdo.toml`. Use `kdo run <task> --filter <project>`
to isolate. For Node projects, ensure `package.json` has the script in the `"scripts"` section.

**Context files are empty or minimal:**
Run `kdo init` to regenerate. Ensure source files have public API signatures (e.g., `pub fn` in Rust,
`export function` in TypeScript). Check `.kdoignore` isn't excluding source directories.

**MCP server not connecting:**
Verify `kdo serve --transport stdio` runs without error. Check the MCP config path matches your agent.
Test with: `echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.0.1"}}}' | kdo serve --transport stdio`

## Quick reference

```
kdo init                              Initialize workspace
kdo new <name>                        Create project interactively
kdo run <task> [--filter project]     Run task across projects
kdo exec <cmd> [--filter project]     Run command in each project
kdo list [--format table|json]        List projects
kdo graph [--format text|json|dot]    Dependency graph
kdo context <project> [--budget N]    AI-optimized context
kdo affected [--base ref]             Changed projects since ref
kdo doctor                            Workspace health check
kdo completions <shell>               Shell completions
kdo serve [--transport stdio]         Start MCP server
```
