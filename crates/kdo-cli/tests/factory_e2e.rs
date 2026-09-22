//! Binary-level factory loop. Uses the mock provider so CI never calls a model.

use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

fn kdo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kdo"))
        .args(args)
        .current_dir(dir)
        .env("KDO_FACTORY_MOCK", "1")
        .output()
        .expect("kdo")
}

fn init_repo(dir: &Path, test_command: &str) {
    git(dir, &["init"]);
    git(dir, &["config", "user.email", "kdo@example.com"]);
    git(dir, &["config", "user.name", "kdo"]);
    std::fs::write(
        dir.join("kdo.toml"),
        format!("[workspace]\nname = \"demo\"\n\n[tasks]\ntest = \"{test_command}\"\n"),
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), "hi\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "init"]);
}

fn assert_ok(output: &Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn mock_factory_run_succeeds_and_keys_do_not_leak() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_repo(dir, "true");
    std::fs::write(
        dir.join("spec.yaml"),
        "kind: Feature\nmetadata:\n  name: hello\nspec:\n  description: say hello\n",
    )
    .unwrap();
    git(dir, &["add", "spec.yaml"]);
    git(dir, &["commit", "-m", "spec"]);

    assert_ok(&kdo(dir, &["apply", "-f", "spec.yaml"]), "apply");

    let mut saw_succeeded = false;
    for _ in 0..8 {
        assert_ok(&kdo(dir, &["factory", "tick"]), "tick");
        let status = kdo(dir, &["factory", "status"]);
        assert_ok(&status, "status");
        if String::from_utf8_lossy(&status.stdout).contains("succeeded") {
            saw_succeeded = true;
            break;
        }
    }
    assert!(saw_succeeded, "run never succeeded");

    let keys = kdo(dir, &["keys", "status"]);
    assert_ok(&keys, "keys");
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&keys.stdout),
        String::from_utf8_lossy(&keys.stderr)
    );
    assert!(!rendered.contains("sk-"));
    assert!(rendered.contains("missing") || rendered.contains("set"));
}

#[test]
fn failing_test_task_does_not_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_repo(dir, "false");
    std::fs::write(
        dir.join("spec.yaml"),
        "kind: Feature\nmetadata:\n  name: boom\nspec:\n  description: should fail\n",
    )
    .unwrap();
    assert_ok(&kdo(dir, &["apply", "-f", "spec.yaml"]), "apply");

    let mut saw_failed = false;
    for _ in 0..8 {
        assert_ok(&kdo(dir, &["factory", "tick"]), "tick");
        let status = kdo(dir, &["factory", "status"]);
        assert_ok(&status, "status");
        let text = String::from_utf8_lossy(&status.stdout);
        if text.contains("failed") {
            saw_failed = true;
            assert!(!text.contains("succeeded"));
            break;
        }
    }
    assert!(saw_failed, "run never failed");

    let log = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "1");
}
