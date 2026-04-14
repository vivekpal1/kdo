//! Integration tests for workspace graph discovery.
//!
//! Uses the `fixtures/sample-monorepo` directory which contains a realistic
//! polyglot workspace with Anchor, Rust, TypeScript, and Python projects.

use kdo_graph::WorkspaceGraph;
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sample-monorepo")
        .canonicalize()
        .expect("fixtures/sample-monorepo must exist")
}

/// Helper for glob-filter tests: build a tempdir tree with fake package.json files.
fn make_scratch_workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    for (rel, pkg_name) in files {
        let dir = tmp.path().join(rel);
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(
            dir.join("package.json"),
            format!(r#"{{"name":"{pkg_name}","version":"1.0.0"}}"#),
        )
        .expect("write package.json");
    }
    tmp
}

#[test]
fn discovers_expected_project_count() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    let projects = graph.projects();
    // Fixture has: vault-program (Anchor), common-lib (Rust), vault-sdk (TS), vault-tool (Python)
    assert!(
        projects.len() >= 3,
        "expected at least 3 projects, got {} — projects: {:?}",
        projects.len(),
        projects.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

#[test]
fn detects_anchor_project() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    let has_anchor = graph
        .projects()
        .iter()
        .any(|p| p.language == kdo_core::Language::Anchor);
    assert!(has_anchor, "expected an Anchor project in fixture");
}

#[test]
fn detects_typescript_project() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    let has_ts = graph
        .projects()
        .iter()
        .any(|p| p.language == kdo_core::Language::TypeScript);
    assert!(has_ts, "expected a TypeScript project in fixture");
}

#[test]
fn detects_python_project() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    let has_python = graph
        .projects()
        .iter()
        .any(|p| p.language == kdo_core::Language::Python);
    assert!(has_python, "expected a Python project in fixture");
}

#[test]
fn no_cycles_in_fixture() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    graph.detect_cycles().expect("fixture must not have cycles");
}

#[test]
fn topological_order_has_all_projects() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    let all = graph.projects();
    let ordered = graph.topological_order();
    assert_eq!(
        all.len(),
        ordered.len(),
        "topological_order must include every project"
    );
}

#[test]
fn get_project_by_name() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    // Find the first project and look it up by name
    let first = graph
        .projects()
        .into_iter()
        .next()
        .expect("at least one project");
    let found = graph
        .get_project(&first.name)
        .expect("get_project must work");
    assert_eq!(first.name, found.name);
}

#[test]
fn graph_output_serialises() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    let output = graph.to_graph_output();
    let json = serde_json::to_string_pretty(&output).expect("serialisation must succeed");
    assert!(json.contains("projects"));
}

#[test]
fn dot_output_is_valid() {
    let root = fixture_root();
    let graph = WorkspaceGraph::discover(&root).expect("discovery must succeed");
    let dot = graph.to_dot();
    assert!(dot.starts_with("digraph workspace {"));
    assert!(dot.ends_with("}\n"));
}

#[test]
fn pnpm_workspace_packages_star_is_single_depth() {
    // Regression: globset's default `*` crosses `/`. We set `literal_separator(true)`
    // so `packages/*` matches exactly one level — matching pnpm/turbo semantics.
    let tmp = make_scratch_workspace(&[
        ("apps/web", "web"),
        ("apps/api", "api"),
        ("packages/ui", "ui"),
        ("packages/nested/deep", "deep"),
    ]);
    std::fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - \"apps/*\"\n  - \"packages/*\"\n",
    )
    .unwrap();

    let graph = WorkspaceGraph::discover(tmp.path()).expect("discovery must succeed");
    let mut names: Vec<String> = graph.projects().iter().map(|p| p.name.clone()).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["api", "ui", "web"],
        "nested package `deep` must NOT be included by single-depth `packages/*`"
    );
}

#[test]
fn pnpm_workspace_exclude_filters_test_dirs() {
    let tmp = make_scratch_workspace(&[
        ("apps/web", "web"),
        ("apps/web/__tests__/fixture", "fixture"),
    ]);
    std::fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - \"apps/**\"\n  - \"!**/__tests__/**\"\n",
    )
    .unwrap();

    let graph = WorkspaceGraph::discover(tmp.path()).expect("discovery must succeed");
    let names: Vec<String> = graph.projects().iter().map(|p| p.name.clone()).collect();
    assert!(
        names.contains(&"web".to_string()),
        "expected `web` to be included; got {names:?}"
    );
    assert!(
        !names.contains(&"fixture".to_string()),
        "expected test fixture to be excluded; got {names:?}"
    );
}
