# Spec: kdo in-process factory

## Objective

Turn `kdo` into a plug-and-play agent factory for any monorepo it already understands. One binary. A spec becomes a run. Micro-agents plan, implement, and review inside a git worktree. The built-in tester runs the workspace test task. A passing review merges onto the current branch only when that checkout is clean. Otherwise the run waits. Keys stay with the user. Plugins are TOML, not native code.

## Commands

```bash
make ci
kdo apply -f spec.yaml
kdo factory status
kdo factory tick
kdo factory daemon
kdo factory logs <run-id>
kdo factory merge <run-id>
kdo keys status
kdo tui
kdo doctor
```

`KDO_FACTORY_MOCK=1` forces the mock provider. With no credentials configured, the mock provider is also the fallback. If any key is configured and the selected model’s key is missing, the run fails with a missing-credential error.

## Layout

- `crates/kdo-factory` — loop, plugins, keys, providers, worktree, tools, merge
- `crates/kdo-cli/src/factory.rs` — apply / factory / keys commands
- `crates/kdo-cli/src/tui.rs` — `kdo tui`
- Plugins: `~/.kdo/plugins/*.toml`, then `<workspace>/.kdo/plugins/*.toml` (later overrides same provider name or agent role)
- Credentials: `$KDO_CREDENTIALS` or `~/.kdo/credentials.toml` mode `0600`, `[keys]` table. Environment variables win.
- Database: `<workspace>/.kdo/factory.db`
- Worktrees: `<workspace>/.kdo/worktrees/<id>` on branch `kdo/run/<id>`

## Pipeline

`plan → implement → test → review → merge`

- Plan: tools `graph`, `read`. Read-only.
- Implement: tools `graph`, `read`, `write`, `diff`. Writes only inside the worktree.
- Test: built-in agent is command-only. It runs the `test` task from `kdo.toml` inside the worktree, in `metadata.project` when that project exists. No task means the tester is skipped, not failed. A plugin agent with `role = "tester"` replaces this.
- Review: tools `graph`, `read`, `diff`. Must return `{"pass": bool, "notes": "..."}`. `pass: false` fails the run and keeps the worktree.
- Merge: not a model. `git merge --no-ff` when `git status --porcelain` has nothing outside `.kdo/`. A dirty tree sets the run to `awaiting_merge`. `kdo factory merge <id>` retries. Conflicts fail the run and keep the worktree.

Model turns are one JSON object: `{"done": true, "summary": "..."}` or `{"tool": "read", "input": {...}}`. Non-JSON text from a planner or implementer is treated as done. A reviewer that does not return a verdict fails.

Each model call counts toward `max_iterations`, `max_tokens`, and `max_cost_usd`. The prompt includes the workspace graph and, when `metadata.project` resolves, a token-budgeted context bundle.

## Plugins

```toml
[plugin]
name = "deepseek"
kind = "provider" # provider | agent

[provider]
protocol = "openai" # openai | anthropic
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
models = ["deepseek-chat"]

[agent]
role = "reviewer"
model = "deepseek-chat"
tools = ["graph", "read", "diff"]
system = "Return JSON."
```

Built-in providers: `anthropic` (prefix `claude`), `openai` (prefix `gpt-`, `o1`, `o3`, `o4`, `chatgpt`). Built-in agents: planner, implementer, reviewer, and a command-only tester. Spec `agents.<role>` overrides the model when set. Secrets are redacted from stored output, events, and errors. They are never printed.

## Boundaries

- Always: `make ci` must pass. No `unwrap()` outside tests. No `println!` in library code. Tools cannot escape the worktree, including through symlinks. The model cannot choose a shell command; `run` only executes a named `kdo.toml` task.
- Ask first: new crates, native plugin loading, pull-request automation, editing the main checkout without a passing review.
- Never: log credentials, merge a dirty tree, load `.so` or WASM plugins.

## Success criteria

- A temp git repo plus a scripted provider writes a file in the worktree and, on a clean tree, that file is on the main checkout after merge.
- A dirty main checkout stops in `awaiting_merge` and merges after it is clean.
- A `../` write and a symlink that points outside the worktree are denied.
- Plugin TOML overrides a built-in role. Key lookup checks the environment, then the credentials file.
- `kdo keys status` prints provider names and `set` or `missing` only.
- `kdo tui` shows specs, runs, tasks, events, key status, and a one-line graph summary.
- `make ci` passes.
