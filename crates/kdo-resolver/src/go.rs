//! Parser for Go modules (`go.mod`).

use crate::ManifestParser;
use kdo_core::{DepKind, Dependency, KdoError, Language, Project};
use std::path::Path;

/// Parser for Go `go.mod` manifest files.
pub struct GoParser;

impl ManifestParser for GoParser {
    fn manifest_name(&self) -> &str {
        "go.mod"
    }

    fn can_parse(&self, manifest_path: &Path) -> bool {
        manifest_path
            .file_name()
            .map(|f| f == "go.mod")
            .unwrap_or(false)
    }

    fn parse(
        &self,
        manifest_path: &Path,
        workspace_root: &Path,
    ) -> Result<(Project, Vec<Dependency>), KdoError> {
        let content = std::fs::read_to_string(manifest_path)?;

        let name = parse_module_name(&content).unwrap_or_else(|| {
            manifest_path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        });

        let project_path = manifest_path
            .parent()
            .unwrap_or(manifest_path)
            .to_path_buf();

        // Parse `require` block for dependencies
        let deps = parse_requires(&content);

        // Determine if this is a workspace-local dependency
        let workspace_deps = deps
            .into_iter()
            .filter_map(|(dep_name, version)| {
                // Only keep deps that resolve to local paths via `replace` directives
                let local_path = find_replace_path(&content, &dep_name)?;
                let resolved = workspace_root.join(&local_path);
                if resolved.exists() {
                    Some(Dependency {
                        name: dep_name
                            .split('/')
                            .next_back()
                            .unwrap_or(&dep_name)
                            .to_string(),
                        version_req: version,
                        kind: DepKind::Source,
                        is_workspace: true,
                        resolved_path: Some(resolved),
                    })
                } else {
                    None
                }
            })
            .collect();

        let project = Project {
            name: short_name(&name),
            path: project_path,
            language: Language::Go,
            manifest_path: manifest_path.to_path_buf(),
            context_summary: None,
            public_api_files: Vec::new(),
            internal_files: Vec::new(),
            content_hash: [0u8; 32],
        };

        Ok((project, workspace_deps))
    }
}

/// Extract the module name from `module <name>` line.
fn parse_module_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("module ") {
            let name = rest.trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Parse `require` blocks: returns (module_path, version) pairs.
fn parse_requires(content: &str) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    let mut in_require_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "require (" {
            in_require_block = true;
            continue;
        }
        if in_require_block && trimmed == ")" {
            in_require_block = false;
            continue;
        }

        if in_require_block {
            // Line format: <module> <version> [// indirect]
            let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let module = parts[0].trim().to_string();
                let version = parts[1].split("//").next().unwrap_or("").trim().to_string();
                if !module.is_empty() && !version.is_empty() {
                    deps.push((module, version));
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("require ") {
            // Single-line require
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                deps.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
            }
        }
    }

    deps
}

/// Look for a `replace` directive for the given module path.
fn find_replace_path(content: &str, module_path: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        // replace <module> => <path>
        if let Some(rest) = trimmed.strip_prefix("replace ") {
            if rest.contains(module_path) {
                if let Some(arrow_pos) = rest.find("=>") {
                    let path = rest[arrow_pos + 2..].trim().to_string();
                    // If it starts with ./ or ../ it's a local path
                    if path.starts_with("./") || path.starts_with("../") {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

/// Get a short human-readable name from a Go module path (last component).
fn short_name(module_path: &str) -> String {
    module_path
        .split('/')
        .next_back()
        .unwrap_or(module_path)
        .to_string()
}
