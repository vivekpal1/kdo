//! Property tests for the `pnpm-workspace.yaml` parser.
//!
//! Focus on invariants rather than specific outputs:
//! - never panics on arbitrary bytes
//! - includes never contain a leading `!` (those go to excludes)
//! - round-trips the known-good forms

use kdo_graph::parse_pnpm_workspace_str;
use proptest::prelude::*;

proptest! {
    /// The parser must never panic, regardless of input.
    #[test]
    fn never_panics_on_arbitrary_input(s in ".*") {
        let _ = parse_pnpm_workspace_str(&s);
    }

    /// The parser must never panic on multi-line random garbage either.
    #[test]
    fn never_panics_on_multiline_garbage(lines in prop::collection::vec(".*", 0..20)) {
        let s = lines.join("\n");
        let _ = parse_pnpm_workspace_str(&s);
    }

    /// Any pattern emitted as an `include` must not start with `!`.
    /// Excludes get the `!` stripped in the parser.
    #[test]
    fn includes_never_carry_bang(patterns in prop::collection::vec("[a-zA-Z0-9_/*?\\-]{1,20}", 1..10)) {
        let body = patterns
            .iter()
            .map(|p| format!("  - \"{p}\""))
            .collect::<Vec<_>>()
            .join("\n");
        let yaml = format!("packages:\n{body}\n");
        let (includes, _excludes) = parse_pnpm_workspace_str(&yaml).unwrap_or_default();
        for inc in &includes {
            prop_assert!(!inc.starts_with('!'), "include pattern has `!` prefix: {inc}");
        }
    }
}

/// Regression: the documented happy-path form parses to the expected vectors.
#[test]
fn parses_pnpm_documented_example() {
    let yaml = r#"
packages:
  - "apps/*"
  - "packages/*"
  - "!**/test/**"
  - docs
"#;
    let (inc, exc) = parse_pnpm_workspace_str(yaml).expect("parses");
    assert_eq!(inc, vec!["apps/*", "packages/*", "docs"]);
    assert_eq!(exc, vec!["**/test/**"]);
}

/// Entries outside the `packages:` block must be ignored.
#[test]
fn ignores_entries_outside_packages_block() {
    let yaml = r#"
shared-workspace-lockfile: true
packages:
  - apps/*
auto-install-peers: true
  - ignored-because-wrong-block
"#;
    let (inc, exc) = parse_pnpm_workspace_str(yaml).expect("parses");
    assert_eq!(inc, vec!["apps/*"]);
    assert!(exc.is_empty());
}

/// Missing or unrelated files return `None` (verified via empty input).
#[test]
fn empty_input_returns_none() {
    assert!(parse_pnpm_workspace_str("").is_none());
    assert!(parse_pnpm_workspace_str("something: else\n").is_none());
}
