This is kdo, a Rust workspace manager for polyglot monorepos with MCP support.

Architecture: kdo-core -> kdo-resolver -> kdo-graph -> kdo-context -> kdo-mcp -> kdo-cli

Rules:
- Zero unwrap() in non-test code. Use miette errors and ? propagation.
- Zero println! for logging. Use tracing or colored eprintln! with owo-colors.
- All public functions need doc comments.
- cargo clippy --all-targets -- -D warnings must pass.
- cargo fmt --all --check must pass.

See skills/kdo/SKILL.md for complete usage documentation.
See CLAUDE.md for project structure details.
