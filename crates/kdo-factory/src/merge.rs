//! Land a run branch onto the current checkout.
//!
//! This is not a model call. A dirty tree (anything outside `.kdo/`) refuses
//! the merge. Conflicts abort and leave the worktree in place.

use crate::error::{FactoryError, FactoryResult};
use crate::gitutil;
use std::path::Path;

pub fn try_merge(workspace: &Path, branch: &str) -> FactoryResult<()> {
    let porcelain = gitutil::ok_strs(workspace, &["status", "--porcelain"])?;
    if user_dirty(&porcelain) {
        return Err(FactoryError::DirtyWorktree);
    }
    let mut args = gitutil::identity_prefix(workspace);
    args.push("merge".into());
    args.push("--no-ff".into());
    args.push("--no-edit".into());
    args.push(branch.to_string());
    let out = gitutil::output(workspace, &args)?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let _ = gitutil::ok_strs(workspace, &["merge", "--abort"]);
    Err(FactoryError::MergeConflict(err))
}

/// `.kdo/` is our own cache and worktree bookkeeping. It must not block a merge.
fn user_dirty(porcelain: &str) -> bool {
    porcelain.lines().any(|line| {
        let path = porcelain_path(line);
        path != ".kdo" && !path.starts_with(".kdo/")
    })
}

fn porcelain_path(line: &str) -> &str {
    let rest = line.get(3..).unwrap_or(line).trim();
    let path = rest.rsplit(" -> ").next().unwrap_or(rest).trim();
    path.trim_matches('"')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree;

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
    fn merges_a_clean_tree_and_refuses_a_dirty_one() {
        let tmp = tempfile::tempdir().unwrap();
        git_init(tmp.path());
        let wt = worktree::create(tmp.path(), "abc123def456").unwrap();
        std::fs::write(wt.path.join("hello.txt"), "hello from kdo\n").unwrap();
        assert!(worktree::commit_all(&wt.path, "kdo: hello").unwrap());

        std::fs::write(tmp.path().join("DIRTY"), "x\n").unwrap();
        let err = try_merge(tmp.path(), &wt.branch).unwrap_err();
        assert!(matches!(err, FactoryError::DirtyWorktree));
        assert!(!tmp.path().join("hello.txt").exists());

        std::fs::remove_file(tmp.path().join("DIRTY")).unwrap();
        try_merge(tmp.path(), &wt.branch).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("hello.txt")).unwrap(),
            "hello from kdo\n"
        );
        worktree::remove(tmp.path(), &wt.path).unwrap();
    }

    #[test]
    fn kdo_cache_does_not_count_as_dirty() {
        let tmp = tempfile::tempdir().unwrap();
        git_init(tmp.path());
        let wt = worktree::create(tmp.path(), "fff123abc456").unwrap();
        std::fs::create_dir_all(tmp.path().join(".kdo")).unwrap();
        std::fs::write(tmp.path().join(".kdo/factory.db"), "db").unwrap();
        try_merge(tmp.path(), &wt.branch).unwrap();
        worktree::remove(tmp.path(), &wt.path).unwrap();
    }

    #[test]
    fn porcelain_ignores_kdo_paths() {
        assert!(!user_dirty("?? .kdo/factory.db\n"));
        assert!(user_dirty("?? src/lib.rs\n"));
        assert!(user_dirty(" M README.md\n?? .kdo/factory.db\n"));
    }
}
