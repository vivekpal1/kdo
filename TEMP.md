```markdown
# kdo — ship v0.2.0 (agent-agnostic runtime + setup commands)

Current status: v0.1.0-alpha.3 shipped. MCP server works with 7 tools. We now need
to make kdo the workspace runtime that optimizes ANY MCP-compatible coding agent,
starting with Claude Code and OpenClaw. OpenClaw is priority #1 because its users
hit deadly token-burn loops daily — kdo prevents those loops.

## STOP-AND-ASK PROTOCOL (unchanged, still in effect)

When you hit uncertainty about ANY external library or protocol:
1. STOP. Do not web-search.
2. Paste the exact compiler/runtime error.
3. State what you've tried.
4. Wait for me to give you the fix.

Never guess APIs. Never try multiple versions of a dep hoping one works. Pin what
I pinned. If a pin is wrong, tell me — I'll repin.

## Scope of this shipment (v0.2.0)

Five deliverables. Ship them in order. Each must compile, test, and work end-to-end
before moving to the next.

### Deliverable 1 — `kdo setup <agent>` command

Two subcommands. Both support `--global` and `--dry-run`.

```
kdo setup claude [--global] [--dry-run]
kdo setup openclaw [--global] [--dry-run]
```

**What `kdo setup claude` does:**
- Writes `kdo` MCP server entry to the right config:
  - Without `--global`: `.claude/mcp_servers.json` in the current workspace
  - With `--global`: `~/.claude/mcp_servers.json`
- Generates or updates `CLAUDE.md` at workspace root with a kdo usage block
  (orientation commands, when-to-use rules, loop-avoidance instructions)
- Creates `.kdo/memory/` directory with initial `MEMORY.md` file
- Creates `.kdo/agents/claude/` for Claude-specific config
- Prints a summary of what it did + "Restart Claude Code to pick up the MCP server"

**What `kdo setup openclaw` does:**
- Writes kdo as a skill under `~/.openclaw/workspace/skills/kdo/SKILL.md` with
  proper AgentSkills-spec YAML frontmatter
- Writes `.kdo/agents/openclaw/SKILL.md` in the workspace (for per-project skills)
- Registers kdo MCP server in `~/.openclaw/openclaw.json` under `mcpServers` key
  (structure: `{ "kdo": { "command": "kdo", "args": ["serve", "--agent", "openclaw"] } }`)
- Generates `AGENTS.md` at workspace root (OpenClaw reads this automatically)
- Creates `.kdo/memory/` with `MEMORY.md` (same as Claude, shared memory pool)
- Prints summary + "Restart OpenClaw to pick up the skill"

**What `--global` changes:**
- Writes to user-level config instead of workspace-level
- Installs kdo as a globally-available skill/server, not tied to one repo
- For OpenClaw: registers at `~/.openclaw/skills/kdo/` (user-level), not workspace-level

**What `--dry-run` changes:**
- Prints every file path kdo would write/modify
- Prints the full content of each file that would be written
- Exits 0 without touching disk

**Critical implementation notes:**
- Never clobber existing config blocks. Detect kdo's block by a sentinel comment
  (`<!-- kdo:start -->` / `<!-- kdo:end -->` for Markdown,
  `/* kdo:start */` / `/* kdo:end */` for JSON-with-comments).
- For JSON files that already exist: parse, merge kdo's keys, re-serialize
  preserving formatting. Use `serde_json::Value` + a stable key order.
- If a config file doesn't exist: create it with a minimal valid structure.
- All writes go through a helper that takes a `Dryrun` enum: `Dryrun::Yes` prints,
  `Dryrun::No` writes atomically (write to temp + rename).

### Deliverable 2 — Agent profiles (`--agent` flag on serve)

Add a flag to `kdo serve`:

```
kdo serve [--transport stdio|sse] [--agent claude|openclaw|generic] [--budget N]
```

`--agent` switches behavior:

- `claude`: default budget 4096 per context call, standard MCP spec
- `openclaw`: default budget 2048 per context call (OpenClaw loops burn tokens
  faster), aggressive loop detection, shorter tool descriptions to minimize the
  per-turn tool-definition overhead OpenClaw pays
- `generic`: no agent-specific tuning

Store the agent profile in a new `AgentProfile` struct:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentProfile {
    Claude,
    OpenClaw,
    Generic,
}

impl AgentProfile {
    pub fn default_context_budget(self) -> usize {
        match self {
            Self::Claude => 4096,
            Self::OpenClaw => 2048,
            Self::Generic => 3072,
        }
    }

    pub fn loop_detection_window(self) -> usize {
        match self {
            Self::Claude => 5,    // last N tool calls
            Self::OpenClaw => 3,  // tighter — OpenClaw loops faster
            Self::Generic => 5,
        }
    }

    pub fn max_tool_output_tokens(self) -> usize {
        match self {
            Self::Claude => 10_000,  // matches Claude Code's default warning
            Self::OpenClaw => 4_000,  // OpenClaw has no built-in warning
            Self::Generic => 8_000,
        }
    }
}
```

Pipe this profile through to every tool handler. Short tool descriptions for
OpenClaw mode — strip examples, keep the one-liner.

### Deliverable 3 — Loop detection + circuit breakers (the OpenClaw killer feature)

Add `crates/kdo-mcp/src/guards.rs` with:

```rust
use std::collections::VecDeque;
use std::time::Instant;

pub struct LoopGuard {
    recent_calls: VecDeque<(String, serde_json::Value, Instant)>,
    window_size: usize,
    duplicate_threshold: usize,  // same call N times = loop
}

impl LoopGuard {
    pub fn new(window_size: usize) -> Self { ... }

    /// Record a tool call. Returns Err(LoopDetected) if caller is looping.
    pub fn record(
        &mut self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<(), LoopError> { ... }
}

#[derive(Debug, thiserror::Error)]
pub enum LoopError {
    #[error("loop detected: {tool} called {count} times with identical args in {window_ms}ms. Break the loop and try a different approach.")]
    IdenticalArgs { tool: String, count: usize, window_ms: u64 },

    #[error("thrash detected: {count} distinct calls in {window_ms}ms exceeds threshold. Pause and reconsider.")]
    HighFrequency { count: usize, window_ms: u64 },
}
```

Wire this into every tool in `KdoServer`:

```rust
#[tool(description = "...")]
async fn kdo_get_context(
    &self,
    Parameters(args): Parameters<GetContextArgs>,
) -> Result<CallToolResult, McpError> {
    self.loop_guard.lock().await
        .record("kdo_get_context", &serde_json::to_value(&args)?)
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

    // ... existing logic
}
```

When a loop is detected, return an error to the agent — Claude/OpenClaw will see
the error and change behavior. This is the single feature that stops OpenClaw's
deadly loops. It's also ~100 lines of code.

### Deliverable 4 — `kdo bench` command (reproducible benchmark)

The "90% token reduction" claim needs a script anyone can run. Ship it.

```
kdo bench [--agent claude|openclaw] [--task <name>] [--iterations N]
```

Workflow:
1. Read `.kdo/bench/tasks.toml` for task definitions
2. For each task, run TWO measurements:
   - **Without kdo:** launch the agent with no MCP server, give it the task prompt,
     measure token consumption via the agent's output log
   - **With kdo:** launch the agent with kdo MCP server configured, same prompt,
     measure token consumption
3. Output a table:

```
Task                    | Without kdo | With kdo  | Reduction
-----------------------|-------------|-----------|----------
Fix withdraw bug       | 12,450 tok  | 1,820 tok | 85.4%
Add new vault method   | 18,700 tok  | 2,940 tok | 84.3%
Refactor fee harvest   | 24,100 tok  | 3,120 tok | 87.1%
-----------------------|-------------|-----------|----------
AVERAGE                | 18,416 tok  | 2,626 tok | 85.7%
```

Store raw results in `.kdo/bench/results/<timestamp>.json` for reproducibility.
Ship a `fixtures/bench-monorepo/` directory with 3 realistic tasks that produce
the headline numbers.

**Do not fake numbers.** Run the bench. If the real number is 60%, ship 60%.
Credibility > marketing.

### Deliverable 5 — Update TODO and README

Replace the current TODO with:

```markdown
# kdo — Development TODO

## v0.2.0 — Agent Runtime (IN PROGRESS)

- [x] `kdo setup claude` / `kdo setup openclaw` with `--global` and `--dry-run`
- [x] Agent profiles (`--agent claude|openclaw|generic`)
- [x] Loop detection + circuit breakers in MCP layer
- [x] `kdo bench` reproducible benchmark harness
- [x] Workspace MEMORY.md layer (shared between Claude and OpenClaw)
- [x] AgentSkills-spec SKILL.md generation per project (`kdo skill-compile`)

## v0.3.0 — Cross-host Memory Sync

- [ ] Git-native memory sync (`.kdo/memory/` commit workflow)
- [ ] Memory indexing for semantic retrieval (embedded, no cloud)
- [ ] `kdo memory add/list/search/forget` CLI
- [ ] Per-project scoped memory with inheritance
- [ ] Team memory sync via git remote (opt-in)

## v0.4.0 — Multi-agent Orchestration

- [ ] Per-agent context budgets (`kdo_allocate_budget`)
- [ ] Parallel agent execution with workspace partitioning
- [ ] Session handoff protocol (Claude Code → OpenClaw, vice versa)
- [ ] Agent-aware git diff (`kdo diff`)

## Carried forward from v0.1.x

- [ ] Content-addressable cache for task outputs
- [ ] Incremental context regeneration
- [ ] Watch mode with file-system events
- [ ] Java/Kotlin parsers (build.gradle, pom.xml)
- [ ] Yarn PnP support
- [ ] SSE transport for MCP
- [ ] Streaming context for large projects
- [ ] Remote cache (S3/GCS)
- [ ] Distributed task execution
- [ ] Benchmark suite (criterion)
- [ ] Windows cross-platform CI
```

Update README with:
- New "Supported Agents" section (Claude Code, OpenClaw, any MCP-compatible)
- `kdo setup` quickstart for both agents
- Benchmark table with real numbers from `kdo bench`
- Removed any "90%" claim that isn't backed by the bench output

## Implementation order

1. **Loop guards first** (Deliverable 3). It's the most valuable feature, ~100
   LOC, and unblocks confidence in everything else.
2. **Agent profiles** (Deliverable 2). Thread `AgentProfile` through the server.
3. **`kdo setup`** (Deliverable 1). Depends on profile knowledge.
4. **`kdo bench`** (Deliverable 4). Run it against fixtures, record real numbers.
5. **TODO + README update** (Deliverable 5). Last — reflects what actually shipped.

## Verification protocol

Before declaring each deliverable done:

```bash
cargo build --release -p kdo
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo test --workspace
```

End-to-end:

```bash
# Deliverable 1 smoke test
mkdir /tmp/kdo-e2e && cd /tmp/kdo-e2e
<set up a real sample workspace>
kdo init
kdo setup claude --dry-run    # must print config, not write
kdo setup claude              # must write to .claude/mcp_servers.json
cat .claude/mcp_servers.json  # verify kdo entry present, other entries preserved
kdo setup openclaw --dry-run  # must print SKILL.md and json diff
kdo setup openclaw            # must write skill + register MCP

# Deliverable 2 smoke test
kdo serve --agent openclaw --help    # must accept the flag
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0.0.1"}}}' | kdo serve --agent openclaw

# Deliverable 3 smoke test
# Hand-craft a test where the same tool is called 4 times with identical args
# Must return a LoopError on the 4th call (for openclaw profile, threshold=3)

# Deliverable 4 smoke test
kdo bench --task fix-withdraw-bug --iterations 1
# Must produce a real measurement, not a hardcoded number

# Deliverable 5 — manual review of README.md and TODO.md
```

If any step fails, STOP and report.

## What you are NOT building in v0.2.0

Explicitly skip (mark as v0.3+ in TODO):
- Semantic memory retrieval (embeddings) — too much scope
- Git-native memory sync — v0.3
- Multi-agent orchestration (`kdo_allocate_budget`) — v0.4
- Session handoff protocol — v0.4
- SSE transport — v0.3
- Any Windows-specific work — later

## Agent-agnosticism principle

Every feature must work with `--agent generic` (no profile-specific assumptions).
Claude and OpenClaw are the first-class profiles. Cursor, Aider, Zed, and anything
else MCP-compatible will work out of the box via the generic profile. Do not
hardcode Claude or OpenClaw paths anywhere except in the `setup` command
implementations.

## Pinned dependencies (unchanged from v0.1.x)

rmcp = "0.16.0" — if higher versions exist, DO NOT upgrade without asking me.
All other deps: keep what v0.1.0-alpha.3 has pinned.

## Final checklist before tagging v0.2.0-alpha.1

- [ ] All 5 deliverables pass smoke tests
- [ ] CHANGELOG.md updated under `[Unreleased]` with all new features
- [ ] README updated with `kdo setup` quickstart + real bench numbers
- [ ] TODO.md updated to reflect v0.2.0 completion
- [ ] `cargo publish --dry-run` passes for every crate in dep order
- [ ] Version bumped to `0.2.0-alpha.1` in every crate's Cargo.toml

When all checks pass, report back with:
1. Paths of every file you modified
2. Output of the `kdo bench` run against the sample fixtures
3. Any deviations from this spec and why

Start with Deliverable 3 (loop guards). Show me the `LoopGuard` struct compiling
before moving on.
```