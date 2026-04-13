//! Parser for `pyproject.toml` manifests (Python projects).

use crate::ManifestParser;
use kdo_core::{DepKind, Dependency, KdoError, Language, Project};
use std::path::Path;
use tracing::debug;

/// Parses Python `pyproject.toml` manifests.
pub struct PythonParser;

impl ManifestParser for PythonParser {
    fn manifest_name(&self) -> &str {
        "pyproject.toml"
    }

    fn can_parse(&self, manifest_path: &Path) -> bool {
        manifest_path
            .file_name()
            .map(|f| f == "pyproject.toml")
            .unwrap_or(false)
    }

    fn parse(
        &self,
        manifest_path: &Path,
        _workspace_root: &Path,
    ) -> Result<(Project, Vec<Dependency>), KdoError> {
        let content = std::fs::read_to_string(manifest_path)?;
        let doc: toml::Value = toml::from_str(&content).map_err(|e| KdoError::ParseError {
            path: manifest_path.to_path_buf(),
            source: e.into(),
        })?;

        // Try [project] table first (PEP 621), then [tool.poetry]
        let (name, description) = if let Some(project) = doc.get("project") {
            let name = project
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let desc = project
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from);
            (name, desc)
        } else if let Some(poetry) = doc.get("tool").and_then(|t| t.get("poetry")) {
            let name = poetry
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let desc = poetry
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from);
            (name, desc)
        } else {
            return Err(KdoError::ParseError {
                path: manifest_path.to_path_buf(),
                source: anyhow::anyhow!("no [project] or [tool.poetry] table found"),
            });
        };

        let project_dir = manifest_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();

        debug!(name = %name, "parsed pyproject.toml");

        let mut deps = Vec::new();

        // PEP 621 dependencies
        if let Some(dep_list) = doc
            .get("project")
            .and_then(|p| p.get("dependencies"))
            .and_then(|d| d.as_array())
        {
            for dep_val in dep_list {
                if let Some(dep_str) = dep_val.as_str() {
                    let (dep_name, version_req) = parse_pep508(dep_str);
                    deps.push(Dependency {
                        name: dep_name,
                        version_req,
                        kind: DepKind::Source,
                        is_workspace: false,
                        resolved_path: None,
                    });
                }
            }
        }

        // Dev dependencies from optional-dependencies.dev
        if let Some(dev_list) = doc
            .get("project")
            .and_then(|p| p.get("optional-dependencies"))
            .and_then(|o| o.get("dev"))
            .and_then(|d| d.as_array())
        {
            for dep_val in dev_list {
                if let Some(dep_str) = dep_val.as_str() {
                    let (dep_name, version_req) = parse_pep508(dep_str);
                    deps.push(Dependency {
                        name: dep_name,
                        version_req,
                        kind: DepKind::Dev,
                        is_workspace: false,
                        resolved_path: None,
                    });
                }
            }
        }

        let project = Project {
            name,
            path: project_dir,
            language: Language::Python,
            manifest_path: manifest_path.to_path_buf(),
            context_summary: description,
            public_api_files: Vec::new(),
            internal_files: Vec::new(),
            content_hash: [0u8; 32],
        };

        Ok((project, deps))
    }
}

/// Rough PEP 508 parser: splits `"requests>=2.28"` into `("requests", ">=2.28")`.
fn parse_pep508(spec: &str) -> (String, String) {
    let spec = spec.trim();
    // Find first version specifier char
    let split_pos = spec
        .find(['>', '<', '=', '!', '~', '[', ';'])
        .unwrap_or(spec.len());
    let name = spec[..split_pos].trim().to_string();
    let version = spec[split_pos..].trim().to_string();
    (name, version)
}
