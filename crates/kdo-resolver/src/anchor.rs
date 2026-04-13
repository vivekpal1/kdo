//! Parser for `Anchor.toml` manifests (Solana Anchor framework).

use crate::ManifestParser;
use kdo_core::{DepKind, Dependency, KdoError, Language, Project};
use std::path::Path;
use tracing::debug;

/// Parses Anchor workspace `Anchor.toml` manifests.
pub struct AnchorParser;

impl ManifestParser for AnchorParser {
    fn manifest_name(&self) -> &str {
        "Anchor.toml"
    }

    fn can_parse(&self, manifest_path: &Path) -> bool {
        manifest_path
            .file_name()
            .map(|f| f == "Anchor.toml")
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

        let project_dir = manifest_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();

        // Anchor.toml has [programs.<network>] with program_name = "address"
        // and [workspace] with members = ["programs/*"]
        let programs = doc
            .get("programs")
            .and_then(|p| p.as_table())
            .and_then(|t| {
                // Get the first network's programs (usually "localnet")
                t.values().next().and_then(|v| v.as_table())
            });

        let program_names: Vec<String> = programs
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();

        // Use first program name or directory name
        let name = program_names.first().cloned().unwrap_or_else(|| {
            project_dir
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "anchor-project".to_string())
        });

        debug!(name = %name, programs = ?program_names, "parsed Anchor.toml");

        // Scan for sub-program Cargo.toml files to find CPI dependencies
        let mut deps = Vec::new();
        if let Some(workspace) = doc.get("workspace") {
            if let Some(members) = workspace.get("members").and_then(|m| m.as_array()) {
                for member in members {
                    if let Some(member_str) = member.as_str() {
                        // Members can be globs like "programs/*"
                        let member_path = workspace_root.join(member_str);
                        if member_path.is_dir() {
                            deps.push(Dependency {
                                name: member_path
                                    .file_name()
                                    .map(|f| f.to_string_lossy().to_string())
                                    .unwrap_or_default(),
                                version_req: "path".to_string(),
                                kind: DepKind::Source,
                                is_workspace: true,
                                resolved_path: Some(member_path),
                            });
                        }
                    }
                }
            }
        }

        let project = Project {
            name,
            path: project_dir,
            language: Language::Anchor,
            manifest_path: manifest_path.to_path_buf(),
            context_summary: None,
            public_api_files: Vec::new(),
            internal_files: Vec::new(),
            content_hash: [0u8; 32],
        };

        Ok((project, deps))
    }
}
