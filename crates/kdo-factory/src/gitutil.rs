//! Small `git` wrappers. Every call is `git -C <repo> ...` with no shell.

use crate::error::{FactoryError, FactoryResult};
use std::path::Path;
use std::process::{Command, Output};

pub fn output(repo: &Path, args: &[String]) -> FactoryResult<Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                FactoryError::Worktree("git is not installed".into())
            } else {
                FactoryError::Io(err)
            }
        })
}

pub fn ok(repo: &Path, args: &[String]) -> FactoryResult<String> {
    let out = output(repo, args)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(FactoryError::Worktree(format!(
            "git {}: {}",
            args.join(" "),
            err.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn ok_strs(repo: &Path, args: &[&str]) -> FactoryResult<String> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    ok(repo, &owned)
}

/// `-c user.email` / `-c user.name` when the repo has no identity.
/// Flags must come before the git subcommand.
pub fn identity_prefix(repo: &Path) -> Vec<String> {
    let email = ok_strs(repo, &["config", "user.email"]).ok();
    let name = ok_strs(repo, &["config", "user.name"]).ok();
    let email_ok = email.as_deref().is_some_and(|s| !s.trim().is_empty());
    let name_ok = name.as_deref().is_some_and(|s| !s.trim().is_empty());
    if email_ok && name_ok {
        Vec::new()
    } else {
        vec![
            "-c".into(),
            "user.email=kdo@local".into(),
            "-c".into(),
            "user.name=kdo".into(),
        ]
    }
}
