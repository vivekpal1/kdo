//! Graph and context injected into every model prompt.

use kdo_context::ContextGenerator;
use kdo_graph::WorkspaceGraph;
use std::path::Path;

const BRIEF_CHARS: usize = 12_000;

pub fn workspace_brief(workspace: &Path, project: Option<&str>, token_budget: usize) -> String {
    let graph = match WorkspaceGraph::discover(workspace) {
        Ok(graph) => graph,
        Err(err) => return format!("graph unavailable: {err}"),
    };
    let mut out = graph.to_text();
    if let Some(name) = project {
        match ContextGenerator::new().generate_bundle(name, token_budget.clamp(256, 4096), &graph) {
            Ok(markdown) => {
                out.push_str("\n\n");
                out.push_str(&markdown);
            }
            Err(err) => {
                out.push_str(&format!(
                    "\n\nproject `{name}` context unavailable: {err}\n"
                ));
            }
        }
    }
    if out.len() > BRIEF_CHARS {
        let mut end = BRIEF_CHARS;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push('…');
    }
    out
}
