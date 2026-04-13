# Agent Instructions — kdo

This repository is **kdo**, a polyglot workspace manager for AI agents.

## For all agents

- The skill file at `skills/kdo/SKILL.md` contains complete usage instructions
- Run `make ci` to verify changes (fmt + clippy + test + doc)
- Fixture at `fixtures/sample-monorepo/` can be used to test all commands
- Zero `unwrap()` in non-test code — use `?` and `miette` errors
- Zero `println!` for logging — use `tracing` or colored `eprintln!`

## Building

```bash
cargo build --release
```

## Testing changes

```bash
make ci                    # Full CI pipeline
make fixture               # Demo on sample monorepo
cd fixtures/sample-monorepo && kdo init && kdo list && kdo run build
```

## MCP server testing

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.0.1"}}}' | kdo serve --transport stdio
```
