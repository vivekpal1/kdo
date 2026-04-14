//! Core types and error definitions for the kdo workspace manager.
//!
//! This crate provides the foundational data structures used across all kdo crates:
//! [`Project`], [`Dependency`], [`Language`], and the unified [`KdoError`] type.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Programming language / framework detected for a project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Pure Rust (Cargo)
    Rust,
    /// TypeScript (package.json with TS)
    TypeScript,
    /// JavaScript (package.json)
    JavaScript,
    /// Python (pyproject.toml / setup.py)
    Python,
    /// Rust + Anchor framework (Solana)
    Anchor,
    /// Go (go.mod)
    Go,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rust => write!(f, "rust"),
            Self::TypeScript => write!(f, "typescript"),
            Self::JavaScript => write!(f, "javascript"),
            Self::Python => write!(f, "python"),
            Self::Anchor => write!(f, "anchor"),
            Self::Go => write!(f, "go"),
        }
    }
}

/// A discovered project within the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Human-readable project name (from manifest).
    pub name: String,
    /// Root directory of the project, relative to workspace root.
    pub path: PathBuf,
    /// Detected language / framework.
    pub language: Language,
    /// Path to the primary manifest file (Cargo.toml, package.json, etc.).
    pub manifest_path: PathBuf,
    /// One-line summary extracted from manifest or CONTEXT.md.
    pub context_summary: Option<String>,
    /// Files that constitute the public API surface.
    pub public_api_files: Vec<PathBuf>,
    /// Internal implementation files.
    pub internal_files: Vec<PathBuf>,
    /// Blake3 content hash of all project files (deterministic).
    pub content_hash: [u8; 32],
}

/// Dependency relationship kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    /// Normal source dependency.
    Source,
    /// Build-time dependency (build.rs, scripts).
    Build,
    /// Development / test dependency.
    Dev,
    /// Solana Cross-Program Invocation dependency.
    Cpi,
}

impl std::fmt::Display for DepKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source => write!(f, "source"),
            Self::Build => write!(f, "build"),
            Self::Dev => write!(f, "dev"),
            Self::Cpi => write!(f, "cpi"),
        }
    }
}

/// A single dependency edge between projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Dependency name (as declared in manifest).
    pub name: String,
    /// Version requirement string (e.g., "^1.0", "workspace:*").
    pub version_req: String,
    /// Kind of dependency.
    pub kind: DepKind,
    /// Whether this dependency uses workspace inheritance.
    pub is_workspace: bool,
    /// Resolved path to the dependency within the workspace (if local).
    pub resolved_path: Option<PathBuf>,
}

/// Unified error type for all kdo operations.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum KdoError {
    /// Workspace manifest not found at the expected path.
    #[error("workspace manifest not found at {0}")]
    ManifestNotFound(PathBuf),

    /// Failed to parse a manifest or source file.
    #[error("failed to parse {path}: {source}")]
    ParseError {
        /// Path to the file that failed to parse.
        path: PathBuf,
        /// Underlying parse error.
        source: anyhow::Error,
    },

    /// Referenced project does not exist in the workspace.
    #[error("project not found: {0}")]
    ProjectNotFound(String),

    /// Circular dependency detected in the workspace graph.
    #[error("circular dependency detected: {0}")]
    #[diagnostic(help("break the cycle by extracting shared code into a separate crate"))]
    CircularDependency(String),

    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Workspace configuration parsed from `kdo.toml`.
///
/// Supports a rich task pipeline model with dependencies, env, aliases, and per-project
/// overrides. Simple string tasks (`build = "cargo build"`) continue to work.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceConfig {
    /// Workspace metadata.
    pub workspace: WorkspaceMeta,

    /// Named tasks. Values can be a bare command string or a rich `[tasks.<name>]` table.
    #[serde(default)]
    pub tasks: std::collections::BTreeMap<String, TaskSpec>,

    /// Workspace-wide environment variables, applied to every task invocation.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,

    /// Paths to `.env`-style files loaded before task execution.
    /// Keys already in [`Self::env`] take precedence.
    #[serde(default)]
    pub env_files: Vec<String>,

    /// Short aliases for tasks, e.g. `b = "build"` so `kdo run b` resolves to `build`.
    #[serde(default)]
    pub aliases: std::collections::BTreeMap<String, String>,

    /// Per-project overrides keyed by project name.
    #[serde(default)]
    pub projects: std::collections::BTreeMap<String, ProjectConfig>,
}

/// Workspace metadata section of `kdo.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceMeta {
    /// Workspace name.
    #[serde(default)]
    pub name: String,

    /// Explicit glob patterns for project discovery. When set, only these paths
    /// are scanned for manifests. Empty = scan everything (default behavior).
    #[serde(default, rename = "projects")]
    pub project_globs: Vec<String>,

    /// Paths to exclude from project discovery.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// A single task definition. Can be declared either as a bare command string
/// (`build = "cargo build"`) or as a full spec (`[tasks.build] command = "..."`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TaskSpec {
    /// Bare command form: `build = "cargo build"`.
    Command(String),
    /// Full spec form with dependencies, inputs, env, and caching hints.
    Full(TaskDef),
}

/// Full task definition with pipeline semantics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskDef {
    /// Shell command to execute. Optional when the task is purely composite (only `depends_on`).
    #[serde(default)]
    pub command: Option<String>,

    /// Task dependencies.
    ///
    /// - `"build"` — run this project's `build` task first (same project).
    /// - `"^build"` — run `build` in every upstream dependency project first (topological).
    /// - `"//lint"` — run the workspace-level `lint` task first.
    #[serde(default)]
    pub depends_on: Vec<String>,

    /// Input glob patterns. Used for cache key computation (future: content-addressable cache).
    #[serde(default)]
    pub inputs: Vec<String>,

    /// Output glob patterns. Files/dirs produced by the task.
    #[serde(default)]
    pub outputs: Vec<String>,

    /// Whether this task's output is cacheable. Default: true.
    #[serde(default = "default_true")]
    pub cache: bool,

    /// Long-running / persistent task (e.g. dev server). Won't block downstream tasks.
    #[serde(default)]
    pub persistent: bool,

    /// Task-specific environment variables (merged on top of workspace env).
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

/// Per-project overrides declared under `[projects.<name>]` in `kdo.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    /// Task overrides for this project.
    #[serde(default)]
    pub tasks: std::collections::BTreeMap<String, TaskSpec>,

    /// Project-specific env (merged on top of workspace env, below task env).
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

impl TaskSpec {
    /// Borrow the command if this task has one.
    pub fn command(&self) -> Option<&str> {
        match self {
            Self::Command(c) => Some(c.as_str()),
            Self::Full(def) => def.command.as_deref(),
        }
    }

    /// Task dependencies (possibly empty).
    pub fn depends_on(&self) -> &[String] {
        match self {
            Self::Command(_) => &[],
            Self::Full(def) => &def.depends_on,
        }
    }

    /// Borrow task-level env vars.
    pub fn env(&self) -> &std::collections::BTreeMap<String, String> {
        static EMPTY: std::sync::OnceLock<std::collections::BTreeMap<String, String>> =
            std::sync::OnceLock::new();
        match self {
            Self::Command(_) => EMPTY.get_or_init(std::collections::BTreeMap::new),
            Self::Full(def) => &def.env,
        }
    }

    /// Whether the task should not block downstream execution.
    pub fn persistent(&self) -> bool {
        matches!(self, Self::Full(def) if def.persistent)
    }
}

impl WorkspaceConfig {
    /// Load workspace config from a `kdo.toml` file.
    pub fn load(path: &std::path::Path) -> Result<Self, KdoError> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| KdoError::ParseError {
            path: path.to_path_buf(),
            source: e.into(),
        })
    }

    /// Write workspace config to a `kdo.toml` file.
    pub fn save(&self, path: &std::path::Path) -> Result<(), KdoError> {
        let content = toml::to_string_pretty(self).map_err(|e| KdoError::ParseError {
            path: path.to_path_buf(),
            source: e.into(),
        })?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Resolve an alias to its real task name. Returns the input unchanged if not aliased.
    pub fn resolve_alias<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases.get(name).map(String::as_str).unwrap_or(name)
    }
}

/// Rough token estimator: ~4 characters per token for English/code.
///
/// # Examples
///
/// ```
/// use kdo_core::estimate_tokens;
/// assert_eq!(estimate_tokens("hello world!"), 3); // 12 chars / 4
/// ```
pub fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("ab"), 0);
        assert_eq!(estimate_tokens("hello world!"), 3);
    }

    #[test]
    fn test_language_display() {
        assert_eq!(Language::Rust.to_string(), "rust");
        assert_eq!(Language::Anchor.to_string(), "anchor");
    }

    #[test]
    fn test_language_serde_roundtrip() {
        let lang = Language::TypeScript;
        let json = serde_json::to_string(&lang).unwrap();
        assert_eq!(json, "\"typescript\"");
        let back: Language = serde_json::from_str(&json).unwrap();
        assert_eq!(back, lang);
    }

    #[test]
    fn test_dep_kind_display() {
        assert_eq!(DepKind::Cpi.to_string(), "cpi");
        assert_eq!(DepKind::Source.to_string(), "source");
    }
}
