//! Workspace discovery and graph construction.

use crate::hash::content_hash_dir;
use indexmap::IndexMap;
use kdo_core::{DepKind, Dependency, KdoError, Project};
use kdo_resolver::{manifest_filenames, parse_manifest};
use petgraph::algo::is_cyclic_directed;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;
use petgraph::Direction;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// The workspace dependency graph.
///
/// Wraps a `petgraph::DiGraph` where nodes are [`Project`]s and edges are [`DepKind`]s.
#[derive(Debug)]
pub struct WorkspaceGraph {
    /// Root directory of the workspace.
    pub root: PathBuf,
    /// Directed graph of projects.
    pub graph: DiGraph<Project, DepKind>,
    /// Map from project name to node index.
    name_to_idx: IndexMap<String, NodeIndex>,
}

/// Serializable project summary for JSON output.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectSummary {
    /// Project name.
    pub name: String,
    /// Detected language.
    pub language: String,
    /// Context summary if available.
    pub summary: Option<String>,
    /// Number of direct dependencies.
    pub dep_count: usize,
}

/// Serializable graph output.
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphOutput {
    /// All projects in the workspace.
    pub projects: Vec<ProjectSummary>,
    /// Edges as (source_name, target_name, kind).
    pub edges: Vec<(String, String, String)>,
}

impl WorkspaceGraph {
    /// Discover all projects under `root` by walking the filesystem.
    ///
    /// Uses the `ignore` crate to respect `.gitignore` and `.kdoignore`.
    /// Parses manifests in parallel with rayon.
    pub fn discover(root: &Path) -> Result<Self, KdoError> {
        let root = root
            .canonicalize()
            .map_err(|_| KdoError::ManifestNotFound(root.to_path_buf()))?;

        info!(root = %root.display(), "discovering workspace");

        let manifest_names = manifest_filenames();
        let walker = ignore::WalkBuilder::new(&root)
            .hidden(true)
            .git_ignore(true)
            .add_custom_ignore_filename(".kdoignore")
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                // Skip node_modules, target, .git, etc.
                !matches!(
                    name.as_ref(),
                    "node_modules" | "target" | ".git" | ".kdo" | "dist" | "build" | "__pycache__"
                )
            })
            .build();

        // Collect manifest paths
        let manifest_paths: Vec<PathBuf> = walker
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                    && entry
                        .file_name()
                        .to_str()
                        .map(|name| manifest_names.contains(&name))
                        .unwrap_or(false)
            })
            .map(|entry| entry.into_path())
            .collect();

        debug!(count = manifest_paths.len(), "found manifest files");

        // Filter out manifests in directories that have a more specific manifest.
        // E.g., if a dir has both Anchor.toml and Cargo.toml, Anchor takes priority.
        let filtered = filter_manifests(&manifest_paths);

        // Parse manifests in parallel
        let results: Vec<Result<(Project, Vec<Dependency>), KdoError>> = filtered
            .par_iter()
            .map(|path| parse_manifest(path, &root))
            .collect();

        let mut graph = DiGraph::new();
        let mut name_to_idx = IndexMap::new();
        let mut all_deps: Vec<(String, Vec<Dependency>)> = Vec::new();

        for result in results {
            match result {
                Ok((mut project, deps)) => {
                    // Compute content hash
                    project.content_hash = content_hash_dir(&project.path);

                    let name = project.name.clone();
                    let idx = graph.add_node(project);
                    name_to_idx.insert(name.clone(), idx);
                    all_deps.push((name, deps));
                }
                Err(KdoError::ManifestNotFound(_)) => {
                    // Skip workspace-root manifests
                    continue;
                }
                Err(e) => {
                    warn!(error = %e, "skipping unparseable manifest");
                }
            }
        }

        // Wire up edges for local/workspace dependencies
        for (source_name, deps) in &all_deps {
            if let Some(&source_idx) = name_to_idx.get(source_name) {
                for dep in deps {
                    if let Some(&target_idx) = name_to_idx.get(&dep.name) {
                        graph.add_edge(source_idx, target_idx, dep.kind.clone());
                    }
                }
            }
        }

        info!(
            projects = name_to_idx.len(),
            edges = graph.edge_count(),
            "workspace graph built"
        );

        Ok(Self {
            root,
            graph,
            name_to_idx,
        })
    }

    /// Get a project by name.
    pub fn get_project(&self, name: &str) -> Result<&Project, KdoError> {
        let idx = self
            .name_to_idx
            .get(name)
            .ok_or_else(|| KdoError::ProjectNotFound(name.to_string()))?;
        Ok(&self.graph[*idx])
    }

    /// Get all projects.
    pub fn projects(&self) -> Vec<&Project> {
        self.graph.node_weights().collect()
    }

    /// Dependency closure: all transitive dependencies of a project (DFS on outgoing edges).
    pub fn dependency_closure(&self, name: &str) -> Result<Vec<&Project>, KdoError> {
        let start = self
            .name_to_idx
            .get(name)
            .ok_or_else(|| KdoError::ProjectNotFound(name.to_string()))?;

        let mut visited = Vec::new();
        let mut stack = vec![*start];
        let mut seen = std::collections::HashSet::new();
        seen.insert(*start);

        while let Some(node) = stack.pop() {
            // Skip the start node itself
            if node != *start {
                visited.push(&self.graph[node]);
            }
            for neighbor in self.graph.neighbors_directed(node, Direction::Outgoing) {
                if seen.insert(neighbor) {
                    stack.push(neighbor);
                }
            }
        }

        Ok(visited)
    }

    /// Affected set: all projects that transitively depend on the given project.
    /// Uses BFS on the reversed graph (incoming edges).
    pub fn affected_set(&self, name: &str) -> Result<Vec<&Project>, KdoError> {
        let start = self
            .name_to_idx
            .get(name)
            .ok_or_else(|| KdoError::ProjectNotFound(name.to_string()))?;

        // BFS on reversed edges
        let reversed = petgraph::visit::Reversed(&self.graph);
        let mut bfs = Bfs::new(&reversed, *start);
        let mut affected = Vec::new();

        while let Some(node) = bfs.next(&reversed) {
            if node != *start {
                affected.push(&self.graph[node]);
            }
        }

        Ok(affected)
    }

    /// Detect cycles in the dependency graph.
    pub fn detect_cycles(&self) -> Result<(), KdoError> {
        if is_cyclic_directed(&self.graph) {
            // Find a cycle for the error message
            let cycle_desc = find_cycle_description(&self.graph);
            return Err(KdoError::CircularDependency(cycle_desc));
        }
        Ok(())
    }

    /// Get a serializable summary of all projects.
    pub fn project_summaries(&self) -> Vec<ProjectSummary> {
        self.graph
            .node_indices()
            .map(|idx| {
                let project = &self.graph[idx];
                let dep_count = self
                    .graph
                    .neighbors_directed(idx, Direction::Outgoing)
                    .count();
                ProjectSummary {
                    name: project.name.clone(),
                    language: project.language.to_string(),
                    summary: project.context_summary.clone(),
                    dep_count,
                }
            })
            .collect()
    }

    /// Get the full graph as a serializable structure.
    pub fn to_graph_output(&self) -> GraphOutput {
        let projects = self.project_summaries();
        let edges = self
            .graph
            .edge_indices()
            .filter_map(|edge_idx| {
                let (source, target) = self.graph.edge_endpoints(edge_idx)?;
                let kind = &self.graph[edge_idx];
                Some((
                    self.graph[source].name.clone(),
                    self.graph[target].name.clone(),
                    kind.to_string(),
                ))
            })
            .collect();

        GraphOutput { projects, edges }
    }

    /// Generate a DOT representation of the graph.
    pub fn to_dot(&self) -> String {
        let mut dot = String::from("digraph workspace {\n  rankdir=LR;\n");
        for idx in self.graph.node_indices() {
            let project = &self.graph[idx];
            dot.push_str(&format!(
                "  \"{}\" [label=\"{}\\n({})\"];\n",
                project.name, project.name, project.language
            ));
        }
        for edge_idx in self.graph.edge_indices() {
            if let Some((source, target)) = self.graph.edge_endpoints(edge_idx) {
                let kind = &self.graph[edge_idx];
                dot.push_str(&format!(
                    "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                    self.graph[source].name, self.graph[target].name, kind
                ));
            }
        }
        dot.push_str("}\n");
        dot
    }

    /// Generate a text representation of the graph.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for idx in self.graph.node_indices() {
            let project = &self.graph[idx];
            out.push_str(&format!("{} ({})\n", project.name, project.language));
            for neighbor in self.graph.neighbors_directed(idx, Direction::Outgoing) {
                let dep = &self.graph[neighbor];
                out.push_str(&format!("  -> {}\n", dep.name));
            }
        }
        out
    }

    /// Dependency closure as JSON string.
    pub fn dependency_closure_json(&self, project: &str) -> Result<String, KdoError> {
        let deps = self.dependency_closure(project)?;
        let names: Vec<&str> = deps.iter().map(|p| p.name.as_str()).collect();
        serde_json::to_string_pretty(&names).map_err(|e| KdoError::ParseError {
            path: std::path::PathBuf::from("<json>"),
            source: e.into(),
        })
    }

    /// Affected set as JSON string.
    pub fn affected_set_json(&self, project: &str) -> Result<String, KdoError> {
        let affected = self.affected_set(project)?;
        let names: Vec<&str> = affected.iter().map(|p| p.name.as_str()).collect();
        serde_json::to_string_pretty(&names).map_err(|e| KdoError::ParseError {
            path: std::path::PathBuf::from("<json>"),
            source: e.into(),
        })
    }

    /// Return all projects in topological order (dependencies before dependents).
    ///
    /// Falls back to insertion order if the graph has cycles (which `detect_cycles` should
    /// have caught earlier).
    pub fn topological_order(&self) -> Vec<&Project> {
        match petgraph::algo::toposort(&self.graph, None) {
            Ok(indices) => indices.iter().map(|idx| &self.graph[*idx]).collect(),
            Err(_) => self.projects(),
        }
    }

    /// Find projects affected by changes since a git ref.
    ///
    /// Shells out to `git diff --name-only` to find changed files,
    /// then maps them to owning projects.
    pub fn affected_since_ref(&self, base_ref: &str) -> Result<Vec<String>, KdoError> {
        let output = std::process::Command::new("git")
            .args(["diff", "--name-only", &format!("{base_ref}...HEAD")])
            .current_dir(&self.root)
            .output()?;

        if !output.status.success() {
            // Fallback: try without ...HEAD (for uncommitted changes)
            let output = std::process::Command::new("git")
                .args(["diff", "--name-only", base_ref])
                .current_dir(&self.root)
                .output()?;

            if !output.status.success() {
                return Ok(Vec::new());
            }
            return self.map_paths_to_projects(&String::from_utf8_lossy(&output.stdout));
        }

        self.map_paths_to_projects(&String::from_utf8_lossy(&output.stdout))
    }

    /// Map changed file paths to their owning project names.
    fn map_paths_to_projects(&self, diff_output: &str) -> Result<Vec<String>, KdoError> {
        let mut affected = std::collections::HashSet::new();
        for line in diff_output.lines() {
            let changed_path = self.root.join(line.trim());
            for project in self.projects() {
                if changed_path.starts_with(&project.path) {
                    affected.insert(project.name.clone());
                }
            }
        }
        let mut result: Vec<String> = affected.into_iter().collect();
        result.sort();
        Ok(result)
    }
}

/// Filter manifests to prefer more specific ones (Anchor.toml > Cargo.toml in same dir).
fn filter_manifests(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut by_dir: IndexMap<PathBuf, Vec<PathBuf>> = IndexMap::new();
    for path in paths {
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        by_dir.entry(dir).or_default().push(path.clone());
    }

    let mut filtered = Vec::new();
    for (_dir, manifests) in &by_dir {
        // Priority: Anchor.toml > Cargo.toml > package.json > pyproject.toml
        let has_anchor = manifests
            .iter()
            .any(|p| p.file_name().map(|f| f == "Anchor.toml").unwrap_or(false));
        for manifest in manifests {
            let filename = manifest
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            // If Anchor.toml exists, skip Cargo.toml in the same dir
            if has_anchor && filename == "Cargo.toml" {
                continue;
            }
            filtered.push(manifest.clone());
        }
    }

    filtered
}

/// Find a cycle and return a human-readable description.
fn find_cycle_description(graph: &DiGraph<Project, DepKind>) -> String {
    // Use petgraph's toposort which returns the node involved in a cycle on error
    match petgraph::algo::toposort(graph, None) {
        Ok(_) => "unknown cycle".to_string(),
        Err(cycle) => {
            let node = &graph[cycle.node_id()];
            format!("cycle involving '{}'", node.name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_manifests_anchor_priority() {
        let paths = vec![
            PathBuf::from("/project/Anchor.toml"),
            PathBuf::from("/project/Cargo.toml"),
            PathBuf::from("/project/sub/Cargo.toml"),
        ];
        let filtered = filter_manifests(&paths);
        assert!(filtered.contains(&PathBuf::from("/project/Anchor.toml")));
        assert!(!filtered.contains(&PathBuf::from("/project/Cargo.toml")));
        assert!(filtered.contains(&PathBuf::from("/project/sub/Cargo.toml")));
    }
}
