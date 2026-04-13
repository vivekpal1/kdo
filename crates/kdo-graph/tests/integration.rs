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
