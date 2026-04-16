# kdo

**Workspace manager for the agent era. Cuts Claude Code token consumption 2x on polyglot monorepos.**

kdo scans your workspace, builds a dependency graph, and serves structured context via MCP instead of letting agents traverse the filesystem blindly. Current tools (Turbo, Nx, Moon, Bun) burn 60-80% of agent tokens on navigation. kdo fixes this.

## Install

kdo is pre-`0.1.0` — all current releases are alphas. You have to opt in explicitly.

**Prebuilt binary (fastest — no Rust toolchain needed):**

```bash
curl -fsSL https://raw.githubusercontent.com/vivekpal1/kdo/main/install.sh | bash -s -- --from-release
```

**From crates.io (requires the version flag for pre-releases):**

```bash
cargo install kdo --version "0.1.0-alpha.3"
```

> `cargo install kdo` without `--version` fails with "could not find `kdo` in registry"
> because Cargo skips pre-releases by default. Once `0.1.0` stable ships, the plain
> `cargo install kdo` command will just work.

**From source (requires Rust toolchain):**

```bash
cargo install --git https://github.com/vivekpal1/kdo
# or
curl -fsSL https://raw.githubusercontent.com/vivekpal1/kdo/main/install.sh | bash
```

**Docker:**

```bash
docker pull ghcr.io/vivekpal1/kdo:latest
docker run -v $(pwd):/workspace ghcr.io/vivekpal1/kdo list
```

**Upgrade later:**

```bash
kdo upgrade               # pull the latest release binary
kdo upgrade --dry-run     # show what would change
```

## Quick start

```bash
# Initialize a new workspace (or adopt an existing repo)
kdo init

# Create a new project interactively
kdo new my-service
# ? Language: [rust] / typescript / python / anchor
# ? Project type: [library] / binary
# ? Framework: [none] / react / next

# List all projects
kdo list

# Show dependency graph
kdo graph --format dot | dot -Tsvg > graph.svg

# Get agent-optimized context for a project (within token budget)
kdo context vault-program --budget 2048

# Run a task across all projects (build, test, lint, dev, etc.)
kdo run build
kdo run test --filter vault-program

# Run an arbitrary command in each project
kdo exec "ls src"

# Find projects affected by recent changes
kdo affected --base main

# Check workspace health
kdo doctor

# Generate shell completions
kdo completions zsh >> ~/.zshrc

# Start MCP server for AI agents
kdo serve --transport stdio
```

## How it works

### `kdo init`

On an **empty directory**: scaffolds a workspace template with `kdo.toml`, `.kdo/`, `.kdoignore`, and `.gitignore` — ready for `kdo new`.

On an **existing repo**: discovers all projects, builds the dependency graph, and generates context files.

Creates two things:

1. **`kdo.toml`** at workspace root (committed) — workspace declaration + task definitions:

```toml
[workspace]
name = "my-monorepo"

[tasks]
build = "cargo build"
test = "cargo test"
lint = "cargo clippy"
```

2. **`.kdo/`** directory (gitignored) — cache for agents:

```
.kdo/
├── config.toml       # workspace configuration
├── cache/            # content hash cache for incremental updates
├── context/          # per-project context files (agents read here)
│   ├── vault-program.md
│   ├── vault-sdk.md
│   └── ...
└── graph.json        # cached dependency graph snapshot
```

### `kdo new <name>`

Interactive project scaffolding. Prompts for language, project type, and framework, then generates a complete project skeleton with manifest, source files, and tests. Supports:

- **Rust**: library or binary, clean Cargo.toml
- **TypeScript**: plain, React, or Next.js, with tsconfig
- **Python**: plain, FastAPI, or CLI (click), with pyproject.toml
- **Anchor**: Solana program skeleton with Anchor.toml

### `.kdoignore`

Works like `.gitignore` — controls which files kdo excludes from context generation and content hashing. Created automatically by `kdo init` with sensible defaults:

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

## Benchmark

Measured on a Solana monorepo (Anchor program + TS SDK + Python tool):

| Method | ~Tokens | Description |
|--------|---------|-------------|
| `find + cat *.rs *.ts *.py` | ~12,400 | Raw filesystem traversal |
| `kdo context vault-program` | ~1,800 | Structured, budgeted context |
| **Reduction** | **~7x** | Only public API signatures, summaries, deps |

## Architecture

```mermaid
graph LR
    A[kdo-cli] --> B[kdo-graph]
    A --> C[kdo-context]
    A --> D[kdo-mcp]
    B --> E[kdo-resolver]
    B --> F[kdo-core]
    C --> B
    C --> F
    D --> B
    D --> C
    E --> F
```

| Crate | Purpose |
|-------|---------|
| `kdo-core` | Types (`Project`, `Dependency`, `Language`), errors (`KdoError`), token estimator |
| `kdo-resolver` | Manifest parsers: `Cargo.toml`, `package.json`, `pyproject.toml`, `Anchor.toml` |
| `kdo-graph` | `WorkspaceGraph` via petgraph — discovery, DFS/BFS queries, blake3 hashing, cycle detection |
| `kdo-context` | Tree-sitter signature extraction, context generation, token budget enforcement |
| `kdo-mcp` | MCP server (JSON-RPC 2.0 over stdio) exposing 5 tools |
| `kdo-cli` | Clap subcommands, interactive scaffolding, tabled output |

## MCP setup

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

## MCP tools

| Tool | Description |
|------|-------------|
| `kdo_list_projects` | List all projects with name, language, summary, dep count |
| `kdo_get_context` | Token-budgeted context bundle (summary + API signatures + deps) |
| `kdo_read_symbol` | Read a specific function/struct/trait body via tree-sitter |
| `kdo_dep_graph` | Dependency closure or dependents for a project |
| `kdo_affected` | Projects changed since a git ref |
| `kdo_search_code` | Search for a pattern across all workspace source files |

## Supported languages

- **Rust** — `Cargo.toml`, tree-sitter signature extraction
- **TypeScript / JavaScript** — `package.json`, `tsconfig.json` detection
- **Python** — `pyproject.toml` (PEP 621 + Poetry)
- **Solana Anchor** — `Anchor.toml`, CPI dependency tracking

## CLI reference

```
kdo init                              # Initialize workspace
kdo new <name>                        # Create project interactively
kdo run <task> [--filter project]     # Run task across projects
kdo exec <cmd> [--filter project]     # Run command in each project
kdo list [--format table|json]        # List projects
kdo graph [--format text|json|dot]    # Show dependency graph
kdo context <project> [--budget N]    # Generate context bundle
kdo affected [--base ref]             # Changed projects since ref
kdo doctor                            # Validate workspace health
kdo completions <shell>               # Generate shell completions
kdo serve [--transport stdio]         # Start MCP server
```

All commands support `--format json` for scripting.

## Composability

kdo operates at the **workspace layer** — discovering projects, building the dependency graph, and serving structured context. For symbol-level intelligence within a project, see [scope-cli](https://github.com/nicholasgasior/scope-cli) — they compose cleanly.

## Roadmap

See [TODO.md](TODO.md) for the full roadmap. Key upcoming items:

- Content-addressable task output caching
- Watch mode with filesystem events
- SSE transport for MCP
- Go, Java/Kotlin language support
- Remote cache (S3/GCS)
- `kdo run` parallel execution with dependency ordering

## License

MIT
