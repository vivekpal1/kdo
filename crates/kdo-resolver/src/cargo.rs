//! Parser for `Cargo.toml` manifests (Rust projects).

use crate::ManifestParser;
use kdo_core::{DepKind, Dependency, KdoError, Language, Project};
use std::path::Path;
use tracing::debug;

/// Parses Rust `Cargo.toml` manifests.
pub struct CargoParser;

impl ManifestParser for CargoParser {
    fn manifest_name(&self) -> &str {
        "Cargo.toml"
    }

    fn can_parse(&self, manifest_path: &Path) -> bool {
        manifest_path
            .file_name()
            .map(|f| f == "Cargo.toml")
            .unwrap_or(false)
    }

    fn parse(
        &self,
        manifest_path: &Path,
        workspace_root: &Path,
    ) -> Result<(Project, Vec<Dependency>), KdoError> {
        let content = std::fs::read_to_string(manifest_path)?;
        let doc: toml::Value = toml::from_str(&content).map_err(|e| KdoError::ParseError {
            path: manifest_path.to_path_buf(),
            source: e.into(),
        })?;

        // Skip workspace-root Cargo.toml (has [workspace] but no [package])
        if doc.get("workspace").is_some() && doc.get("package").is_none() {
            return Err(KdoError::ManifestNotFound(manifest_path.to_path_buf()));
        }

        let package = doc.get("package").ok_or_else(|| KdoError::ParseError {
            path: manifest_path.to_path_buf(),
            source: anyhow::anyhow!("missing [package] table"),
        })?;

        let name = package
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KdoError::ParseError {
                path: manifest_path.to_path_buf(),
                source: anyhow::anyhow!("missing package.name"),
            })?
            .to_string();

        let description = package
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        let project_dir = manifest_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();

        debug!(name = %name, path = %project_dir.display(), "parsed Cargo.toml");

        let mut deps = Vec::new();

        // Parse each dependency section
        for (section, kind) in [
            ("dependencies", DepKind::Source),
            ("dev-dependencies", DepKind::Dev),
            ("build-dependencies", DepKind::Build),
        ] {
            if let Some(table) = doc.get(section).and_then(|v| v.as_table()) {
                for (dep_name, dep_val) in table {
                    let dep = parse_cargo_dep(dep_name, dep_val, &kind, workspace_root);
                    deps.push(dep);
                }
            }
        }

        let project = Project {
            name,
            path: project_dir,
            language: Language::Rust,
            manifest_path: manifest_path.to_path_buf(),
            context_summary: description,
            public_api_files: Vec::new(),
            internal_files: Vec::new(),
            content_hash: [0u8; 32],
        };

        Ok((project, deps))
    }
}

fn parse_cargo_dep(
    name: &str,
    value: &toml::Value,
    kind: &DepKind,
    workspace_root: &Path,
) -> Dependency {
    let mut version_req = String::new();
    let mut is_workspace = false;
    let mut resolved_path = None;

    match value {
        toml::Value::String(v) => {
            version_req = v.clone();
        }
        toml::Value::Table(t) => {
            if let Some(v) = t.get("version").and_then(|v| v.as_str()) {
                version_req = v.to_string();
            }
            if let Some(true) = t.get("workspace").and_then(|v| v.as_bool()) {
                is_workspace = true;
                version_req = "workspace".to_string();
            }
            if let Some(p) = t.get("path").and_then(|v| v.as_str()) {
                resolved_path = Some(workspace_root.join(p));
            }
        }
        _ => {}
    }

    Dependency {
        name: name.to_string(),
        version_req,
        kind: kind.clone(),
        is_workspace,
        resolved_path,
    }
}
