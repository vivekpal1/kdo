# kdo for DeepSeek Harness

Installed by `kdo setup dsh`.

- Registers nothing by itself until Harness restarts and loads `~/.dsh/plugins/kdo`.
- Settings → kdo imports a Claude Code, Codex, OpenCode, or Grok Build chat.
- The plugin shells out to `kdo chats export` and appends the visible turns to a new DSH session.

`kdo` must be on `PATH`.
