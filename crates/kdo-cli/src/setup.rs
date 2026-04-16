//! `kdo setup <agent>` — one-command wiring for Claude Code and OpenClaw.
//!
//! Design invariants:
//!
//! - **Idempotent.** Running the command twice converges to the same state.
//!   Markdown files are updated in-place between `<!-- kdo:start -->` /
//!   `<!-- kdo:end -->` sentinels; JSON files are parsed, merged at a JSON
//!   pointer, and re-serialized valid (no comments — JSON stays strictly JSON).
//! - **Dry-run is honest.** With `--dry-run`, every path + full content that
//!   would be written is printed and the filesystem is untouched.
//! - **Atomic writes.** Real writes go through `write_to_temp_then_rename`;
//!   readers never see partial files.
//!
//! `--global` writes to user-level locations (and skips workspace-only files
//! like `.kdo/memory/` and `.kdo/agents/`). Without `--global`, writes land in
//! the workspace the command was run from.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use miette::{IntoDiagnostic, Result};
use owo_colors::OwoColorize;
use serde_json::Value;

// ─────────────────────────── Public entry point ───────────────────────────

/// Run `kdo setup <agent>`. Dispatches to the right agent-specific plan.
pub fn cmd_setup(agent: &str, global: bool, dry_run: bool) -> Result<()> {
    let ctx = SetupCtx::new(global, dry_run)?;

    eprintln!(
        "{} {} {} {}",
        "kdo".cyan().bold(),
        "setup".bold(),
        agent.yellow().bold(),
        if dry_run {
            "dry-run".magenta().to_string()
        } else if global {
            "global".dimmed().to_string()
        } else {
            "workspace".dimmed().to_string()
        }
    );
    eprintln!();

    let mut actions = Actions::default();
    match agent {
        "claude" => plan_claude(&ctx, &mut actions)?,
        "openclaw" => plan_openclaw(&ctx, &mut actions)?,
        other => miette::bail!("unknown agent: {other} (expected 'claude' or 'openclaw')"),
    }

    actions.apply(&ctx)?;

    if dry_run {
        eprintln!();
        eprintln!("  {} no changes made.", "dry-run".magenta());
    } else {
        eprintln!();
        match agent {
            "claude" => eprintln!(
                "  {} Restart Claude Code to pick up the MCP server.",
                "done".green()
            ),
            "openclaw" => eprintln!(
                "  {} Restart OpenClaw to pick up the skill + MCP server.",
                "done".green()
            ),
            _ => {}
        }
    }
    Ok(())
}

// ─────────────────────────── Execution context ───────────────────────────

pub(crate) struct SetupCtx {
    /// Current workspace root (cwd). Always present so per-workspace setup
    /// can write into it; ignored for `--global` writes.
    pub workspace: PathBuf,
    /// User home dir (for `~/.openclaw/`, `~/.claude/` etc).
    pub home: PathBuf,
    pub global: bool,
    pub dry_run: bool,
}

impl SetupCtx {
    fn new(global: bool, dry_run: bool) -> Result<Self> {
        let workspace = std::env::current_dir().into_diagnostic()?;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| miette::miette!("HOME env var not set"))?;
        Ok(Self {
            workspace,
            home,
            global,
            dry_run,
        })
    }
}

// ─────────────────────────── Action model ───────────────────────────

/// An accumulated list of side-effecting actions. Built by a `plan_*`
/// function, then executed in order by `apply`.
#[derive(Default)]
pub(crate) struct Actions {
    items: Vec<Action>,
}

pub(crate) enum Action {
    /// Create a directory (and any missing parents). No-op if it exists.
    EnsureDir { path: PathBuf },

    /// Write a complete file. If it exists, overwrite.
    WriteFile {
        path: PathBuf,
        content: String,
        note: &'static str,
    },

    /// Merge a Markdown block between `<!-- kdo:start -->` and `<!-- kdo:end -->`.
    /// If those sentinels are missing in an existing file, append the block at
    /// the end. If the file is missing, create it with header + block.
    MergeMarkdown {
        path: PathBuf,
        /// The block body. Sentinels are added automatically.
        block: String,
        /// Header prepended when creating the file for the first time.
        initial_header: String,
    },

    /// Merge a JSON value at a JSON Pointer path. The file is parsed (or
    /// seeded from `seed` if missing), the value is set at `pointer`, and the
    /// result is re-serialized pretty. Plain JSON only — no comments.
    MergeJson {
        path: PathBuf,
        pointer: String,
        value: Value,
        seed: Value,
        note: &'static str,
    },

    /// Shell out. If dry-run, only print. On real runs, inherit stdout/stderr
    /// so the user sees the tool's own output (e.g. `claude mcp add`).
    ShellCommand {
        program: String,
        args: Vec<String>,
        note: &'static str,
    },
}

impl Actions {
    pub(crate) fn push(&mut self, a: Action) {
        self.items.push(a);
    }

    fn apply(self, ctx: &SetupCtx) -> Result<()> {
        for action in self.items {
            match action {
                Action::EnsureDir { path } => ensure_dir(ctx, &path)?,
                Action::WriteFile {
                    path,
                    content,
                    note,
                } => write_file(ctx, &path, &content, note)?,
                Action::MergeMarkdown {
                    path,
                    block,
                    initial_header,
                } => merge_markdown(ctx, &path, &block, &initial_header)?,
                Action::MergeJson {
                    path,
                    pointer,
                    value,
                    seed,
                    note,
                } => merge_json(ctx, &path, &pointer, value, seed, note)?,
                Action::ShellCommand {
                    program,
                    args,
                    note,
                } => run_shell(ctx, &program, &args, note)?,
            }
        }
        Ok(())
    }
}

// ─────────────────────────── IO primitives ───────────────────────────

fn ensure_dir(ctx: &SetupCtx, path: &Path) -> Result<()> {
    print_action("mkdir", &path.display().to_string(), None);
    if !ctx.dry_run {
        fs::create_dir_all(path).into_diagnostic()?;
    }
    Ok(())
}

fn write_file(ctx: &SetupCtx, path: &Path, content: &str, note: &str) -> Result<()> {
    print_action("write", &path.display().to_string(), Some(note));
    if ctx.dry_run {
        print_content_preview(content);
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    write_to_temp_then_rename(path, content.as_bytes())?;
    Ok(())
}

fn merge_markdown(ctx: &SetupCtx, path: &Path, block: &str, initial_header: &str) -> Result<()> {
    const START: &str = "<!-- kdo:start -->";
    const END: &str = "<!-- kdo:end -->";

    let existing = if path.exists() {
        fs::read_to_string(path).into_diagnostic()?
    } else {
        String::new()
    };

    let new_block = format!(
        "{START}\n<!-- Generated by `kdo setup`. Edit outside the sentinels, not inside. -->\n{block}\n{END}\n"
    );

    let updated = if existing.is_empty() {
        format!("{initial_header}\n{new_block}")
    } else if let (Some(start), Some(end)) = (existing.find(START), existing.find(END)) {
        // Replace the existing kdo block in place.
        let end_line = existing[end..]
            .find('\n')
            .map(|i| end + i + 1)
            .unwrap_or(existing.len());
        format!("{}{new_block}{}", &existing[..start], &existing[end_line..])
    } else {
        // No sentinels — append.
        let sep = if existing.ends_with('\n') { "" } else { "\n" };
        format!("{existing}{sep}\n{new_block}")
    };

    print_action(
        "merge",
        &path.display().to_string(),
        Some("markdown sentinels"),
    );
    if ctx.dry_run {
        print_content_preview(&updated);
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    write_to_temp_then_rename(path, updated.as_bytes())?;
    Ok(())
}

fn merge_json(
    ctx: &SetupCtx,
    path: &Path,
    pointer: &str,
    value: Value,
    seed: Value,
    note: &str,
) -> Result<()> {
    let mut doc: Value = if path.exists() {
        let raw = fs::read_to_string(path).into_diagnostic()?;
        if raw.trim().is_empty() {
            seed
        } else {
            serde_json::from_str(&raw)
                .map_err(|e| miette::miette!("{}: invalid JSON: {e}", path.display()))?
        }
    } else {
        seed
    };

    set_at_pointer(&mut doc, pointer, value)?;

    let serialized = serde_json::to_string_pretty(&doc).into_diagnostic()?;
    let serialized = format!("{serialized}\n");

    print_action(
        "merge",
        &format!("{} (at {})", path.display(), pointer),
        Some(note),
    );
    if ctx.dry_run {
        print_content_preview(&serialized);
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    write_to_temp_then_rename(path, serialized.as_bytes())?;
    Ok(())
}

/// Set `value` at the JSON pointer `/foo/bar/...`, creating intermediate
/// objects as needed. Only object-path pointers are supported.
fn set_at_pointer(doc: &mut Value, pointer: &str, value: Value) -> Result<()> {
    let parts: Vec<&str> = pointer
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        *doc = value;
        return Ok(());
    }

    let mut cursor = doc;
    for (i, part) in parts.iter().enumerate() {
        // JSON Pointer unescape: ~1 -> /, ~0 -> ~
        let key = part.replace("~1", "/").replace("~0", "~");

        if !cursor.is_object() {
            *cursor = Value::Object(serde_json::Map::new());
        }
        let obj = cursor.as_object_mut().unwrap();

        let is_last = i == parts.len() - 1;
        if is_last {
            obj.insert(key, value);
            return Ok(());
        }
        if !obj.contains_key(&key) {
            obj.insert(key.clone(), Value::Object(serde_json::Map::new()));
        }
        cursor = obj.get_mut(&key).unwrap();
    }
    Ok(())
}

fn run_shell(ctx: &SetupCtx, program: &str, args: &[String], note: &str) -> Result<()> {
    let cmdline = format!("{program} {}", args.join(" "));
    print_action("run", &cmdline, Some(note));
    if ctx.dry_run {
        return Ok(());
    }
    let status = Command::new(program)
        .args(args)
        .status()
        .into_diagnostic()?;
    if !status.success() {
        miette::bail!(
            "command failed ({}): {cmdline}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// Atomic file write on POSIX — temp file in the same directory, then rename.
fn write_to_temp_then_rename(dest: &Path, bytes: &[u8]) -> Result<()> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(
        ".{}.kdo-setup-{}",
        dest.file_name().and_then(|n| n.to_str()).unwrap_or("temp"),
        std::process::id()
    ));
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .into_diagnostic()?;
        f.write_all(bytes).into_diagnostic()?;
        f.sync_all().into_diagnostic()?;
    }
    fs::rename(&tmp, dest).into_diagnostic()?;
    Ok(())
}

// ─────────────────────────── Pretty-printing ───────────────────────────

fn print_action(verb: &str, target: &str, note: Option<&str>) {
    let verb = match verb {
        "mkdir" => verb.blue().bold().to_string(),
        "write" => verb.green().bold().to_string(),
        "merge" => verb.cyan().bold().to_string(),
        "run" => verb.yellow().bold().to_string(),
        _ => verb.to_string(),
    };
    if let Some(note) = note {
        eprintln!("  {} {}  {}", verb, target, format!("— {note}").dimmed());
    } else {
        eprintln!("  {} {}", verb, target);
    }
}

fn print_content_preview(content: &str) {
    let lines: Vec<&str> = content.lines().collect();
    let preview_lines = 40usize;
    let show = lines.iter().take(preview_lines);
    for line in show {
        eprintln!("    {}", line.dimmed());
    }
    if lines.len() > preview_lines {
        eprintln!(
            "    {}",
            format!("... ({} more lines)", lines.len() - preview_lines).dimmed()
        );
    }
}

// ─────────────────────────── Claude plan ───────────────────────────

fn plan_claude(ctx: &SetupCtx, actions: &mut Actions) -> Result<()> {
    // 1. MCP server registration (Flag 2 = shell out to `claude mcp add`).
    let scope = if ctx.global { "user" } else { "local" };
    actions.push(Action::ShellCommand {
        program: "claude".into(),
        args: vec![
            "mcp".into(),
            "add".into(),
            "--scope".into(),
            scope.into(),
            "kdo".into(),
            "--".into(),
            "kdo".into(),
            "serve".into(),
            "--transport".into(),
            "stdio".into(),
            "--agent".into(),
            "claude".into(),
        ],
        note: if ctx.global {
            "register kdo globally via Claude CLI"
        } else {
            "register kdo locally via Claude CLI"
        },
    });

    // 2. CLAUDE.md with kdo instructions.
    let claude_md_path = if ctx.global {
        ctx.home.join(".claude").join("CLAUDE.md")
    } else {
        ctx.workspace.join("CLAUDE.md")
    };
    actions.push(Action::MergeMarkdown {
        path: claude_md_path,
        block: claude_md_block(ctx.global).into(),
        initial_header: claude_md_initial_header(ctx.global).into(),
    });

    // 3–4. Workspace-scoped state: memory + per-agent config. Skip for --global.
    if !ctx.global {
        actions.push(Action::EnsureDir {
            path: ctx.workspace.join(".kdo").join("memory"),
        });
        actions.push(Action::WriteFile {
            path: ctx.workspace.join(".kdo").join("memory").join("MEMORY.md"),
            content: memory_md_template().into(),
            note: "seed shared agent memory",
        });
        actions.push(Action::EnsureDir {
            path: ctx.workspace.join(".kdo").join("agents").join("claude"),
        });
    }

    Ok(())
}

fn claude_md_initial_header(global: bool) -> &'static str {
    if global {
        "# Claude — personal\n\nThis file is read by Claude Code in every project.\n"
    } else {
        "# CLAUDE.md — project context\n\nThis file is read by Claude Code when working in this repo.\n"
    }
}

fn claude_md_block(global: bool) -> &'static str {
    if global {
        CLAUDE_BLOCK_GLOBAL
    } else {
        CLAUDE_BLOCK_WORKSPACE
    }
}

const CLAUDE_BLOCK_WORKSPACE: &str = r#"
## kdo — workspace intelligence

**Before editing code in this repo, orient with kdo:**

1. `kdo_list_projects` — one call, cheap (~200 tokens). Shows every project.
2. `kdo_get_context <project>` — structured summary + public API signatures.
3. `kdo_dep_graph <project>` / `kdo_affected` — understand blast radius.
4. `kdo_read_symbol` — only when you need a specific function body.
5. `kdo_search_code` — cross-project pattern search.
6. `kdo_run_task` — run build/test/lint from the conversation.

**Do not walk the filesystem to discover projects.** That burns 5–10× more tokens
than kdo does the same job with.

**Loop avoidance:** the kdo MCP server returns a structured error if it sees the
same tool called 5 times with identical arguments. Don't retry the same call —
change arguments, switch tools, or ask the human for clarification.
"#;

const CLAUDE_BLOCK_GLOBAL: &str = r#"
## kdo — installed globally

kdo is registered as a user-scope MCP server. In any workspace that's been
`kdo init`'d, these tools are available:

- `kdo_list_projects`, `kdo_get_context`, `kdo_dep_graph`, `kdo_affected`,
  `kdo_read_symbol`, `kdo_search_code`, `kdo_run_task`
- Resources: `kdo://context/<project>`

Orient with `kdo_list_projects` before reading files by hand — it's 5–10× cheaper.
"#;

// ─────────────────────────── OpenClaw plan ───────────────────────────

fn plan_openclaw(ctx: &SetupCtx, actions: &mut Actions) -> Result<()> {
    // 1. SKILL.md — per-project or user-level.
    let skill_dir = if ctx.global {
        ctx.home.join(".openclaw").join("skills").join("kdo")
    } else {
        ctx.workspace.join(".kdo").join("agents").join("openclaw")
    };
    actions.push(Action::EnsureDir {
        path: skill_dir.clone(),
    });
    actions.push(Action::WriteFile {
        path: skill_dir.join("SKILL.md"),
        content: openclaw_skill_md().into(),
        note: "AgentSkills-spec skill definition",
    });

    // 2. MCP server registration at ~/.openclaw/openclaw.json (always user-level).
    let openclaw_cfg = ctx.home.join(".openclaw").join("openclaw.json");
    actions.push(Action::MergeJson {
        path: openclaw_cfg,
        pointer: "/mcpServers/kdo".into(),
        value: serde_json::json!({
            "command": "kdo",
            "args": ["serve", "--transport", "stdio", "--agent", "openclaw"]
        }),
        seed: serde_json::json!({ "mcpServers": {} }),
        note: "register kdo MCP server in OpenClaw gateway config",
    });

    // 3–5. Workspace-scoped: AGENTS.md, memory, per-project skill. Skip for --global.
    if !ctx.global {
        actions.push(Action::MergeMarkdown {
            path: ctx.workspace.join("AGENTS.md"),
            block: agents_md_block().into(),
            initial_header:
                "# AGENTS.md — agent-facing project context\n\nOpenClaw reads this automatically.\n"
                    .into(),
        });

        actions.push(Action::EnsureDir {
            path: ctx.workspace.join(".kdo").join("memory"),
        });
        actions.push(Action::WriteFile {
            path: ctx.workspace.join(".kdo").join("memory").join("MEMORY.md"),
            content: memory_md_template().into(),
            note: "seed shared agent memory",
        });
    }

    Ok(())
}

fn openclaw_skill_md() -> &'static str {
    r#"---
name: kdo
description: Context-native workspace manager for polyglot monorepos. Use before reading source files by hand — orient with kdo_list_projects, then pull token-budgeted context with kdo_get_context. Exposes seven tools plus resources at kdo://context/<project>.
---

# kdo

kdo is a polyglot workspace manager (Rust, TypeScript, Python, Go, Anchor).
It ships an MCP server with seven tools and a resources endpoint.

## When to use

Any time you need to understand what's in a workspace, what a project's public
API looks like, or what a change might affect. Prefer kdo over filesystem walks
— kdo gives you the same information in 5–10× fewer tokens.

## Recommended flow

1. `kdo_list_projects` — orient. Cheap, one call.
2. `kdo_get_context <project>` — public API + deps, budgeted.
3. `kdo_dep_graph` / `kdo_affected` — blast radius.
4. `kdo_read_symbol` — only when you need a function body.
5. `kdo_search_code` — cross-workspace pattern search.
6. `kdo_run_task` — run build/test/lint from conversation.

## Loop guard

The kdo server rejects the third identical tool call in a row with a structured
error. If you see it, don't retry — change arguments, switch tools, or ask the
user for clarification.

## Fallback CLI

If the MCP connection is unavailable, shell out:

- `kdo list` — list projects
- `kdo graph` — show dependency edges
- `kdo context <project>` — generate context bundle
- `kdo affected --base main` — changed projects
- `kdo run <task>` — run a task across projects

Full reference: `kdo --help` or https://vivekpal1.github.io/kdo/docs/cli
"#
}

fn agents_md_block() -> &'static str {
    r#"
## kdo — workspace intelligence

`kdo` is wired as an MCP server. Before reading code by hand, call:

1. `kdo_list_projects` — orient
2. `kdo_get_context <project>` — structured context bundle
3. `kdo_dep_graph` / `kdo_affected` — blast radius before editing
4. `kdo_read_symbol` — only for specific function bodies
5. `kdo_search_code` — cross-project pattern search

Full catalog + docs: https://vivekpal1.github.io/kdo/docs/mcp

**Loop avoidance:** the server returns an error on the 3rd identical call.
"#
}

// ─────────────────────────── Shared templates ───────────────────────────

fn memory_md_template() -> &'static str {
    r#"# kdo agent memory

This file is shared memory for every AI agent that works in this workspace
(Claude Code, OpenClaw, any MCP-capable client). kdo's agent skills point
here as the canonical source of cross-session state.

Add entries below as durable facts that agents should know about this repo:
- ownership / reviewer conventions
- deploy gotchas and recovery runbooks
- data-shape decisions that aren't visible from the code alone
- external system URLs with terse descriptions of what they're for

Do not paste secrets here — this file is committed to the repo.

---
"#
}

// ─────────────────────────── Tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn set_at_pointer_creates_nested_objects() {
        let mut doc = json!({});
        set_at_pointer(&mut doc, "/mcpServers/kdo/command", json!("kdo")).unwrap();
        set_at_pointer(
            &mut doc,
            "/mcpServers/kdo/args",
            json!(["serve", "--transport", "stdio"]),
        )
        .unwrap();
        assert_eq!(
            doc,
            json!({
                "mcpServers": {
                    "kdo": {
                        "command": "kdo",
                        "args": ["serve", "--transport", "stdio"]
                    }
                }
            })
        );
    }

    #[test]
    fn set_at_pointer_preserves_siblings() {
        let mut doc = json!({
            "mcpServers": {
                "other": { "command": "other-mcp" }
            }
        });
        set_at_pointer(
            &mut doc,
            "/mcpServers/kdo",
            json!({ "command": "kdo", "args": ["serve"] }),
        )
        .unwrap();
        assert_eq!(doc["mcpServers"]["other"]["command"], "other-mcp");
        assert_eq!(doc["mcpServers"]["kdo"]["command"], "kdo");
    }

    #[test]
    fn markdown_merge_inserts_sentinels_on_fresh_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let ctx = SetupCtx {
            workspace: dir.path().to_path_buf(),
            home: dir.path().to_path_buf(),
            global: false,
            dry_run: false,
        };
        merge_markdown(&ctx, &path, "\n## kdo\n\nhello from kdo\n", "# project\n").unwrap();
        let out = fs::read_to_string(&path).unwrap();
        assert!(out.contains("<!-- kdo:start -->"));
        assert!(out.contains("<!-- kdo:end -->"));
        assert!(out.contains("hello from kdo"));
        assert!(out.starts_with("# project\n"));
    }

    #[test]
    fn markdown_merge_replaces_existing_block() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("CLAUDE.md");
        fs::write(
            &path,
            "# project\n\npre-existing content\n\n<!-- kdo:start -->\nOLD BLOCK\n<!-- kdo:end -->\n\ntrailing user notes\n",
        )
        .unwrap();
        let ctx = SetupCtx {
            workspace: dir.path().to_path_buf(),
            home: dir.path().to_path_buf(),
            global: false,
            dry_run: false,
        };
        merge_markdown(&ctx, &path, "\nNEW BLOCK\n", "# project\n").unwrap();
        let out = fs::read_to_string(&path).unwrap();
        assert!(out.contains("pre-existing content"));
        assert!(out.contains("NEW BLOCK"));
        assert!(out.contains("trailing user notes"));
        assert!(!out.contains("OLD BLOCK"));
    }

    #[test]
    fn json_merge_preserves_other_servers() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("openclaw.json");
        fs::write(
            &path,
            r#"{
  "mcpServers": {
    "github": { "command": "github-mcp" }
  },
  "otherSetting": true
}"#,
        )
        .unwrap();
        let ctx = SetupCtx {
            workspace: dir.path().to_path_buf(),
            home: dir.path().to_path_buf(),
            global: false,
            dry_run: false,
        };
        merge_json(
            &ctx,
            &path,
            "/mcpServers/kdo",
            json!({ "command": "kdo", "args": ["serve"] }),
            json!({ "mcpServers": {} }),
            "test",
        )
        .unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let doc: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(doc["mcpServers"]["github"]["command"], "github-mcp");
        assert_eq!(doc["mcpServers"]["kdo"]["command"], "kdo");
        assert_eq!(doc["otherSetting"], true);
    }

    #[test]
    fn dry_run_does_not_touch_filesystem() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let ctx = SetupCtx {
            workspace: dir.path().to_path_buf(),
            home: dir.path().to_path_buf(),
            global: false,
            dry_run: true,
        };
        let mut actions = Actions::default();
        actions.push(Action::WriteFile {
            path: path.clone(),
            content: "hello".into(),
            note: "test",
        });
        actions.apply(&ctx).unwrap();
        assert!(!path.exists(), "dry-run must not create files");
    }
}
