//! Built-in tools. `write` and `read` cannot leave the worktree.
//! `run` executes a named `kdo.toml` task, never a shell string from the model.

use crate::error::{FactoryError, FactoryResult};
use crate::gitutil;
use crate::plugin::ToolName;
use kdo_core::WorkspaceConfig;
use kdo_graph::WorkspaceGraph;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const READ_LIMIT: usize = 32_000;
const WRITE_LIMIT: usize = 256_000;
const OUTPUT_LIMIT: usize = 32_000;

pub struct ToolContext<'a> {
    pub workspace: &'a Path,
    pub worktree: &'a Path,
    pub project: Option<&'a str>,
    pub brief: &'a str,
    pub base_sha: Option<&'a str>,
    pub allowed: &'a [ToolName],
}

pub fn execute(
    ctx: &ToolContext<'_>,
    name: ToolName,
    input: &serde_json::Value,
) -> FactoryResult<String> {
    if !ctx.allowed.contains(&name) {
        return Err(FactoryError::ToolDenied(format!(
            "{} is not allowed for this agent",
            name.as_str()
        )));
    }
    match name {
        ToolName::Graph => Ok(ctx.brief.to_string()),
        ToolName::Read => {
            let path = input_str(input, "path")?;
            let full = jail_existing(ctx.worktree, path)?;
            let text = std::fs::read_to_string(&full).map_err(|err| {
                FactoryError::ToolDenied(format!("read {}: {err}", full.display()))
            })?;
            Ok(truncate(&text, READ_LIMIT))
        }
        ToolName::Write => {
            let path = input_str(input, "path")?;
            let contents = input
                .get("contents")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| FactoryError::ToolDenied("write requires contents".into()))?;
            if contents.len() > WRITE_LIMIT {
                return Err(FactoryError::ToolDenied(format!(
                    "write exceeds {WRITE_LIMIT} bytes"
                )));
            }
            let full = jail_new(ctx.worktree, path)?;
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full, contents)?;
            Ok(format!("wrote {}", path))
        }
        ToolName::Diff => diff(ctx),
        ToolName::Run => {
            let task = input_str(input, "task")?;
            run_task(ctx.workspace, ctx.worktree, task, ctx.project)
        }
    }
}

pub fn run_task(
    workspace: &Path,
    worktree: &Path,
    task: &str,
    project: Option<&str>,
) -> FactoryResult<String> {
    if !task
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        || task.is_empty()
        || task.len() > 64
    {
        return Err(FactoryError::ToolDenied(format!(
            "refusing task name `{task}`"
        )));
    }
    let Some(command) = task_command(workspace, task)? else {
        return Ok(format!("no task `{task}` in kdo.toml"));
    };
    let cwd = task_cwd(worktree, project);
    run_shell(&cwd, &command)
}

/// The built-in tester. Missing `kdo.toml` or `test` task is a skip.
pub fn run_builtin_test(
    workspace: &Path,
    worktree: &Path,
    project: Option<&str>,
) -> FactoryResult<Option<String>> {
    let config_path = workspace.join("kdo.toml");
    if !config_path.exists() {
        return Ok(None);
    }
    let config =
        WorkspaceConfig::load(&config_path).map_err(|err| FactoryError::Plugin(err.to_string()))?;
    let name = config.resolve_alias("test");
    let Some(spec) = config.tasks.get(name) else {
        return Ok(None);
    };
    let Some(command) = spec.command() else {
        return Ok(None);
    };
    let cwd = task_cwd(worktree, project);
    run_shell(&cwd, command).map(Some)
}

fn task_command(workspace: &Path, task: &str) -> FactoryResult<Option<String>> {
    let config_path = workspace.join("kdo.toml");
    if !config_path.exists() {
        return Ok(None);
    }
    let config =
        WorkspaceConfig::load(&config_path).map_err(|err| FactoryError::Plugin(err.to_string()))?;
    let name = config.resolve_alias(task);
    Ok(config
        .tasks
        .get(name)
        .and_then(|spec| spec.command().map(str::to_string)))
}

fn task_cwd(worktree: &Path, project: Option<&str>) -> PathBuf {
    let Some(project) = project else {
        return worktree.to_path_buf();
    };
    let Ok(graph) = WorkspaceGraph::discover(worktree) else {
        return worktree.to_path_buf();
    };
    graph
        .get_project(project)
        .map(|found| worktree.join(&found.path))
        .unwrap_or_else(|_| worktree.to_path_buf())
}

fn run_shell(cwd: &Path, command: &str) -> FactoryResult<String> {
    let mut cmd = shell(command);
    cmd.current_dir(cwd);
    let output = cmd.output()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr);
    if !err.is_empty() {
        text.push('\n');
        text.push_str(&err);
    }
    let text = truncate(&text, OUTPUT_LIMIT);
    if output.status.success() {
        Ok(text)
    } else {
        Err(FactoryError::Worktree(format!(
            "task failed ({}): {text}",
            output.status
        )))
    }
}

fn shell(command: &str) -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

fn diff(ctx: &ToolContext<'_>) -> FactoryResult<String> {
    let text = if let Some(base) = ctx.base_sha {
        gitutil::ok_strs(ctx.worktree, &["diff", base])
    } else {
        gitutil::ok_strs(ctx.worktree, &["diff", "HEAD"])
    }
    .unwrap_or_else(|err| err.to_string());
    Ok(truncate(&text, OUTPUT_LIMIT))
}

fn input_str<'a>(input: &'a serde_json::Value, key: &str) -> FactoryResult<&'a str> {
    input
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| FactoryError::ToolDenied(format!("missing `{key}`")))
}

fn lexical_rel(rel: &str) -> FactoryResult<PathBuf> {
    if rel.is_empty() || rel.contains('\0') {
        return Err(FactoryError::ToolDenied("empty path".into()));
    }
    if rel.split(['/', '\\']).any(|part| part == ".git") {
        return Err(FactoryError::ToolDenied("refusing .git".into()));
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(FactoryError::ToolDenied("absolute path".into()));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(FactoryError::ToolDenied(format!(
            "parent segment in `{rel}`"
        )));
    }
    Ok(path.to_path_buf())
}

pub fn jail_existing(root: &Path, rel: &str) -> FactoryResult<PathBuf> {
    let rel_path = lexical_rel(rel)?;
    let root = root
        .canonicalize()
        .map_err(|err| FactoryError::ToolDenied(err.to_string()))?;
    let full = root.join(rel_path);
    let canon = full
        .canonicalize()
        .map_err(|_| FactoryError::ToolDenied(format!("not found: {rel}")))?;
    if !canon.starts_with(&root) {
        return Err(FactoryError::ToolDenied(format!("escapes worktree: {rel}")));
    }
    Ok(canon)
}

/// Resolve `rel` inside `root`, creating missing parent directories only
/// after every existing component has been canonicalized and checked.
pub fn jail_new(root: &Path, rel: &str) -> FactoryResult<PathBuf> {
    let rel_path = lexical_rel(rel)?;
    let root = root
        .canonicalize()
        .map_err(|err| FactoryError::ToolDenied(err.to_string()))?;
    let mut current = root.clone();
    let components: Vec<Component> = rel_path.components().collect();
    for (index, component) in components.iter().enumerate() {
        let next = current.join(component);
        if next.exists() {
            let canon = next
                .canonicalize()
                .map_err(|err| FactoryError::ToolDenied(err.to_string()))?;
            if !canon.starts_with(&root) {
                return Err(FactoryError::ToolDenied(format!("escapes worktree: {rel}")));
            }
            current = canon;
        } else {
            for component in &components[index..] {
                current = current.join(component);
            }
            if let Some(parent) = current.parent() {
                if parent != root && !parent.starts_with(&root) {
                    return Err(FactoryError::ToolDenied(format!("escapes worktree: {rel}")));
                }
            }
            return Ok(current);
        }
    }
    Ok(current)
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx<'a>(root: &'a Path, allowed: &'a [ToolName]) -> ToolContext<'a> {
        ToolContext {
            workspace: root,
            worktree: root,
            project: None,
            brief: "graph",
            base_sha: None,
            allowed,
        }
    }

    #[test]
    fn write_and_read_stay_inside() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let allowed = [ToolName::Write, ToolName::Read, ToolName::Graph];
        execute(
            &ctx(root, &allowed),
            ToolName::Write,
            &json!({"path": "a/b.txt", "contents": "hi"}),
        )
        .unwrap();
        let text = execute(
            &ctx(root, &allowed),
            ToolName::Read,
            &json!({"path": "a/b.txt"}),
        )
        .unwrap();
        assert_eq!(text, "hi");
        let err = execute(
            &ctx(root, &allowed),
            ToolName::Write,
            &json!({"path": "../nope.txt", "contents": "x"}),
        )
        .unwrap_err();
        assert!(matches!(err, FactoryError::ToolDenied(_)));
        assert!(!tmp.path().join("nope.txt").exists());
    }

    #[test]
    fn disallowed_tool_is_denied() {
        let tmp = tempfile::tempdir().unwrap();
        let err = execute(
            &ctx(tmp.path(), &[ToolName::Read]),
            ToolName::Write,
            &json!({"path": "a.txt", "contents": "x"}),
        )
        .unwrap_err();
        assert!(matches!(err, FactoryError::ToolDenied(_)));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_denied() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("wt");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let err = execute(
            &ctx(&root, &[ToolName::Write]),
            ToolName::Write,
            &json!({"path": "link/secret.txt", "contents": "nope"}),
        )
        .unwrap_err();
        assert!(matches!(err, FactoryError::ToolDenied(_)));
        assert!(!outside.join("secret.txt").exists());
    }

    #[test]
    fn run_refuses_shell_metacharacters() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run_task(tmp.path(), tmp.path(), "test; rm -rf /", None).unwrap_err();
        assert!(matches!(err, FactoryError::ToolDenied(_)));
    }
}
