//! Parser for `package.json` manifests (Node.js / TypeScript projects).

use crate::ManifestParser;
use kdo_core::{DepKind, Dependency, KdoError, Language, Project};
use std::path::Path;
use tracing::debug;

/// Parses Node.js `package.json` manifests.
pub struct NodeParser;

impl ManifestParser for NodeParser {
    fn manifest_name(&self) -> &str {
        "package.json"
    }

    fn can_parse(&self, manifest_path: &Path) -> bool {
        manifest_path
            .file_name()
            .map(|f| f == "package.json")
            .unwrap_or(false)
    }

    fn parse(
        &self,
        manifest_path: &Path,
        workspace_root: &Path,
    ) -> Result<(Project, Vec<Dependency>), KdoError> {
        let content = std::fs::read_to_string(manifest_path)?;
        let doc: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| KdoError::ParseError {
                path: manifest_path.to_path_buf(),
                source: e.into(),
            })?;

        let name = doc
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KdoError::ParseError {
                path: manifest_path.to_path_buf(),
                source: anyhow::anyhow!("missing name field"),
            })?
            .to_string();

        let description = doc
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        let project_dir = manifest_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();

        // Detect TypeScript vs JavaScript
        let language = if project_dir.join("tsconfig.json").exists()
            || doc
                .get("devDependencies")
                .and_then(|v| v.as_object())
                .map(|d| d.contains_key("typescript"))
                .unwrap_or(false)
        {
            Language::TypeScript
        } else {
            Language::JavaScript
        };

        debug!(name = %name, language = %language, "parsed package.json");

        let mut deps = Vec::new();

        for (section, kind) in [
            ("dependencies", DepKind::Source),
            ("devDependencies", DepKind::Dev),
            ("peerDependencies", DepKind::Source),
        ] {
            if let Some(obj) = doc.get(section).and_then(|v| v.as_object()) {
                for (dep_name, dep_val) in obj {
                    let version_req = dep_val.as_str().unwrap_or("*").to_string();
                    let is_workspace = version_req.starts_with("workspace:");
                    let resolved_path = if is_workspace || version_req.starts_with("file:") {
                        let clean = version_req
                            .trim_start_matches("workspace:")
                            .trim_start_matches("file:");
                        if clean == "*" || clean == "^" {
                            // Workspace protocol — resolve by name in parent directories
                            None
                        } else {
                            Some(workspace_root.join(clean))
                        }
                    } else {
                        None
                    };

                    deps.push(Dependency {
                        name: dep_name.clone(),
                        version_req,
                        kind: kind.clone(),
                        is_workspace,
                        resolved_path,
                    });
                }
            }
        }

        let project = Project {
            name,
            path: project_dir,
            language,
            manifest_path: manifest_path.to_path_buf(),
            context_summary: description,
            public_api_files: Vec::new(),
            internal_files: Vec::new(),
            content_hash: [0u8; 32],
        };

        Ok((project, deps))
    }
}
