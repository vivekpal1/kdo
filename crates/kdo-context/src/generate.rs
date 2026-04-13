//! CONTEXT.md generation and token-budgeted context bundles.

use crate::extract::{extract_signatures, Signature, SignatureKind};
use kdo_core::{estimate_tokens, Language};
use kdo_graph::WorkspaceGraph;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::debug;

/// A token-budgeted context bundle for a project.
#[derive(Debug, Serialize, Deserialize)]
pub struct ContextBundle {
    /// Project name.
    pub project: String,
    /// One-line summary.
    pub summary: Option<String>,
    /// Public API signatures grouped by kind.
    pub signatures: Vec<Signature>,
    /// Dependency names.
    pub dependencies: Vec<String>,
    /// Total estimated tokens used.
    pub tokens_used: usize,
    /// Token budget applied.
    pub budget: usize,
    /// Whether output was truncated.
    pub truncated: bool,
    /// Number of signatures omitted due to budget.
    pub omitted_count: usize,
}

/// Generate a context bundle for a project within a token budget.
///
/// Tiers:
/// 1. Summary + dependency list (always included)
/// 2. Public API signatures (functions, structs, traits)
/// 3. Implementation details (truncated first)
pub fn generate_context(
    graph: &WorkspaceGraph,
    project_name: &str,
    budget: usize,
) -> Result<ContextBundle, kdo_core::KdoError> {
    let project = graph.get_project(project_name)?;
    let deps = graph.dependency_closure(project_name)?;
    let dep_names: Vec<String> = deps.iter().map(|d| d.name.clone()).collect();

    // Collect source files
    let source_files = collect_source_files(&project.path, &project.language);

    // Extract all signatures
    let mut all_sigs: Vec<Signature> = Vec::new();
    for file in &source_files {
        let sigs = extract_signatures(file, &project.language);
        all_sigs.extend(sigs);
    }

    // Build the bundle with token budget enforcement
    let mut tokens_used = 0;
    let mut included_sigs = Vec::new();
    let mut truncated = false;
    let mut omitted_count = 0;

    // Tier 1: Summary + deps (always included)
    let summary_text = project
        .context_summary
        .as_deref()
        .unwrap_or("No description");
    tokens_used += estimate_tokens(summary_text);
    let deps_text = dep_names.join(", ");
    tokens_used += estimate_tokens(&deps_text);
    tokens_used += 50; // Header overhead

    // Tier 2: Signatures by priority
    // Functions first, then structs/enums, then traits, then others
    let priority_order = [
        SignatureKind::Function,
        SignatureKind::Struct,
        SignatureKind::Enum,
        SignatureKind::Trait,
        SignatureKind::TypeAlias,
        SignatureKind::Impl,
        SignatureKind::Constant,
    ];

    let mut sorted_sigs = all_sigs.clone();
    sorted_sigs.sort_by_key(|sig| {
        priority_order
            .iter()
            .position(|k| k == &sig.kind)
            .unwrap_or(99)
    });

    for sig in &sorted_sigs {
        let sig_tokens = estimate_tokens(&sig.text) + 5; // formatting overhead
        if tokens_used + sig_tokens > budget {
            truncated = true;
            omitted_count += 1;
        } else {
            tokens_used += sig_tokens;
            included_sigs.push(sig.clone());
        }
    }

    debug!(
        project = project_name,
        total_sigs = all_sigs.len(),
        included = included_sigs.len(),
        tokens = tokens_used,
        budget = budget,
        "generated context bundle"
    );

    Ok(ContextBundle {
        project: project_name.to_string(),
        summary: project.context_summary.clone(),
        signatures: included_sigs,
        dependencies: dep_names,
        tokens_used,
        budget,
        truncated,
        omitted_count,
    })
}

/// Render a context bundle as CONTEXT.md markdown.
pub fn render_context_md(bundle: &ContextBundle) -> String {
    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", bundle.project));

    if let Some(summary) = &bundle.summary {
        md.push_str(&format!("> {summary}\n\n"));
    }

    md.push_str("## Public API\n\n");

    // Group by kind
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut traits = Vec::new();
    let mut type_aliases = Vec::new();
    let mut others = Vec::new();

    for sig in &bundle.signatures {
        match sig.kind {
            SignatureKind::Function => functions.push(sig),
            SignatureKind::Struct => structs.push(sig),
            SignatureKind::Enum => enums.push(sig),
            SignatureKind::Trait => traits.push(sig),
            SignatureKind::TypeAlias => type_aliases.push(sig),
            _ => others.push(sig),
        }
    }

    if !functions.is_empty() {
        md.push_str("### Functions\n\n");
        for sig in &functions {
            md.push_str(&format!("- `{}`\n", sig.text.replace('\n', " ")));
        }
        md.push('\n');
    }

    if !structs.is_empty() {
        md.push_str("### Structs\n\n");
        for sig in &structs {
            md.push_str(&format!("- `{}`\n", first_line(&sig.text)));
        }
        md.push('\n');
    }

    if !enums.is_empty() {
        md.push_str("### Enums\n\n");
        for sig in &enums {
            md.push_str(&format!("- `{}`\n", first_line(&sig.text)));
        }
        md.push('\n');
    }

    if !traits.is_empty() {
        md.push_str("### Traits\n\n");
        for sig in &traits {
            md.push_str(&format!("- `{}`\n", first_line(&sig.text)));
        }
        md.push('\n');
    }

    if !type_aliases.is_empty() {
        md.push_str("### Types\n\n");
        for sig in &type_aliases {
            md.push_str(&format!("- `{}`\n", sig.text.replace('\n', " ")));
        }
        md.push('\n');
    }

    if !others.is_empty() {
        md.push_str("### Other\n\n");
        for sig in &others {
            md.push_str(&format!("- `{}`\n", first_line(&sig.text)));
        }
        md.push('\n');
    }

    if bundle.truncated {
        md.push_str(&format!(
            "\n... [{} more signatures omitted, budget {}/{}]\n",
            bundle.omitted_count, bundle.tokens_used, bundle.budget
        ));
    }

    if !bundle.dependencies.is_empty() {
        md.push_str("## Dependencies\n\n");
        for dep in &bundle.dependencies {
            md.push_str(&format!("- `{dep}`\n"));
        }
        md.push('\n');
    }

    md
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or(s).to_string()
}

/// Collect source files for a project based on its language.
fn collect_source_files(project_path: &Path, language: &Language) -> Vec<std::path::PathBuf> {
    let extensions: &[&str] = match language {
        Language::Rust | Language::Anchor => &["rs"],
        Language::TypeScript => &["ts", "tsx"],
        Language::JavaScript => &["js", "jsx"],
        Language::Python => &["py"],
        Language::Go => &["go"],
    };

    let mut result = Vec::new();
    let walker = ignore::WalkBuilder::new(project_path)
        .hidden(true)
        .git_ignore(true)
        .add_custom_ignore_filename(".kdoignore")
        .build();

    for entry in walker.flatten() {
        let name = entry.file_name().to_string_lossy();
        if matches!(
            name.as_ref(),
            "node_modules" | "target" | ".git" | "dist" | "build" | "__pycache__"
        ) {
            continue;
        }
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
