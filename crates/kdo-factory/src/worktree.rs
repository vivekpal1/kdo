//! Per-run git worktrees. Agents never touch the checkout they were launched from.

use crate::error::{FactoryError, FactoryResult};
use crate::gitutil;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub branch: String,
    pub path: PathBuf,
    pub base_sha: String,
}

pub fn create(workspace: &Path, run_id: &str) -> FactoryResult<Worktree> {
    gitutil::ok_strs(workspace, &["rev-parse", "--is-inside-work-tree"])?;
    let base_sha = gitutil::ok_strs(workspace, &["rev-parse", "HEAD"])?;
    let short = short_id(run_id);
    let branch = format!("kdo/run/{short}");
    let path = workspace.join(".kdo").join("worktrees").join(&short);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let path_arg = path.to_string_lossy().into_owned();
    gitutil::ok_strs(
        workspace,
        &["worktree", "add", "-b", &branch, &path_arg, "HEAD"],
    )?;
    Ok(Worktree {
        branch,
        path,
        base_sha,
    })
}

pub fn commit_all(worktree: &Path, message: &str) -> FactoryResult<bool> {
    gitutil::ok_strs(worktree, &["add", "-A"])?;
    let status = gitutil::ok_strs(worktree, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(false);
    }
    let mut args = gitutil::identity_prefix(worktree);
    args.push("commit".into());
    args.push("-m".into());
    args.push(message.to_string());
    gitutil::ok(worktree, &args)?;
    Ok(true)
}

pub fn remove(workspace: &Path, worktree: &Path) -> FactoryResult<()> {
    let args = vec![
        "worktree".to_string(),
        "remove".into(),
        "--force".into(),
        worktree.to_string_lossy().into_owned(),
    ];
    let out = gitutil::output(workspace, &args)?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    if err.contains("not a working tree") || err.contains("No such file") {
        return Ok(());
    }
    Err(FactoryError::Worktree(err.trim().to_string()))
}

pub fn delete_branch(workspace: &Path, branch: &str) -> FactoryResult<()> {
    gitutil::ok_strs(workspace, &["branch", "-d", branch])?;
    Ok(())
}

fn short_id(run_id: &str) -> String {
    let mut short: String = run_id
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .take(12)
        .collect();
    if short.is_empty() {
        short = "run".into();
    }
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_init(dir: &std::path::Path) {
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        };
        run(&["init"]);
        run(&["config", "user.email", "kdo@example.com"]);
        run(&["config", "user.name", "kdo"]);
        std::fs::write(dir.join("README.md"), "hi\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-m", "init"]);
    }

    #[test]
    fn creates_commits_and_removes() {
        let tmp = tempfile::tempdir().unwrap();
        git_init(tmp.path());
        let wt = create(tmp.path(), "abcdef1234567890").unwrap();
        assert!(wt.path.join("README.md").exists());
        std::fs::write(wt.path.join("hello.txt"), "hello\n").unwrap();
        assert!(commit_all(&wt.path, "kdo: hello").unwrap());
        remove(tmp.path(), &wt.path).unwrap();
        assert!(!wt.path.exists());
    }
}
