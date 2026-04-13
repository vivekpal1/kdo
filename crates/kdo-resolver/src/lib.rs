//! Manifest parsers for polyglot workspaces.
//!
//! Implements [`ManifestParser`] for Cargo, Node (package.json),
//! Python (pyproject.toml), and Anchor workspaces.

mod anchor;
mod cargo;
mod node;
mod python;

pub use anchor::AnchorParser;
pub use cargo::CargoParser;
pub use node::NodeParser;
pub use python::PythonParser;

use kdo_core::{Dependency, KdoError, Project};
use std::path::Path;

/// Trait implemented by each language-specific manifest parser.
pub trait ManifestParser {
    /// Returns the canonical manifest filename (e.g., `Cargo.toml`).
    fn manifest_name(&self) -> &str;

    /// Check whether this parser can handle the given manifest path.
    fn can_parse(&self, manifest_path: &Path) -> bool;

    /// Parse a manifest file, returning a project and its declared dependencies.
    ///
    /// `workspace_root` is used to resolve relative path dependencies.
    fn parse(
        &self,
        manifest_path: &Path,
        workspace_root: &Path,
    ) -> Result<(Project, Vec<Dependency>), KdoError>;
}

/// Detects the appropriate parser for a manifest file and parses it.
///
/// Tries parsers in order: Anchor, Cargo, Node, Python.
pub fn parse_manifest(
    manifest_path: &Path,
    workspace_root: &Path,
) -> Result<(Project, Vec<Dependency>), KdoError> {
    let parsers: Vec<Box<dyn ManifestParser>> = vec![
        Box::new(AnchorParser),
        Box::new(CargoParser),
        Box::new(NodeParser),
        Box::new(PythonParser),
    ];

    for parser in &parsers {
        if parser.can_parse(manifest_path) {
            return parser.parse(manifest_path, workspace_root);
        }
    }

    Err(KdoError::ManifestNotFound(manifest_path.to_path_buf()))
}

/// Returns the list of manifest filenames to scan for.
pub fn manifest_filenames() -> &'static [&'static str] {
    &[
        "Anchor.toml",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
    ]
}
