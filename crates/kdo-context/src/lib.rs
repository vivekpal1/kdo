//! Context generation for kdo projects.
//!
//! Uses tree-sitter to extract public API signatures (no bodies) and generates
//! structured CONTEXT.md files with token budget enforcement.

mod extract;
mod generate;

pub use extract::{extract_signatures, Signature, SignatureKind};
pub use generate::{generate_context, render_context_md, ContextBundle};

use kdo_core::KdoError;
use kdo_graph::WorkspaceGraph;

/// Context generator — thin wrapper providing the MCP-facing API.
#[derive(Debug)]
pub struct ContextGenerator;

impl ContextGenerator {
    /// Create a new context generator.
    pub fn new() -> Self {
        Self
    }

    /// Generate a token-budgeted context bundle and render it as markdown.
    pub fn generate_bundle(
        &self,
        project: &str,
        token_budget: usize,
        graph: &WorkspaceGraph,
    ) -> Result<String, KdoError> {
        let bundle = generate_context(graph, project, token_budget)?;
        Ok(render_context_md(&bundle))
    }

    /// Read a specific symbol's source from a project.
    ///
    /// Searches all extracted signatures for a matching name and returns the text.
    pub fn read_symbol(
        &self,
        project_name: &str,
        symbol: &str,
        graph: &WorkspaceGraph,
    ) -> Result<String, KdoError> {
        let project = graph.get_project(project_name)?;
        let source_files = collect_source_files(&project.path, &project.language);

        for file in &source_files {
            let sigs = extract_signatures(file, &project.language);
            for sig in &sigs {
                if sig.text.contains(symbol) {
                    return Ok(format!("// {}:{}\n{}", sig.file, sig.line, sig.text));
                }
            }
        }

        // If not found via signatures, try searching file contents directly
        for file in &source_files {
            if let Ok(content) = std::fs::read_to_string(file) {
                if content.contains(symbol) {
                    // Find the relevant block
                    for (i, line) in content.lines().enumerate() {
                        if line.contains(symbol) {
                            let start = i.saturating_sub(2);
                            let end = (i + 20).min(content.lines().count());
                            let snippet: String = content
                                .lines()
                                .skip(start)
                                .take(end - start)
                                .collect::<Vec<_>>()
                                .join("\n");
                            return Ok(format!("// {}:{}\n{}", file.display(), start + 1, snippet));
                        }
                    }
                }
            }
        }

        Err(KdoError::ProjectNotFound(format!(
            "symbol '{symbol}' not found in project '{project_name}'"
        )))
    }
}

impl Default for ContextGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Collect source files for a project based on its language.
fn collect_source_files(
    project_path: &std::path::Path,
    language: &kdo_core::Language,
) -> Vec<std::path::PathBuf> {
    let extensions: &[&str] = match language {
        kdo_core::Language::Rust | kdo_core::Language::Anchor => &["rs"],
        kdo_core::Language::TypeScript => &["ts", "tsx"],
        kdo_core::Language::JavaScript => &["js", "jsx"],
        kdo_core::Language::Python => &["py"],
        kdo_core::Language::Go => &["go"],
    };

    let walker = ignore::WalkBuilder::new(project_path)
        .hidden(true)
        .git_ignore(true)
        .add_custom_ignore_filename(".kdoignore")
        .build();

    let mut result = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        let matches_ext = entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| extensions.contains(&ext))
            .unwrap_or(false);
        if matches_ext {
            result.push(entry.into_path());
        }
    }
    result
}
