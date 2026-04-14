//! Workspace dependency graph built on petgraph.
//!
//! Discovers projects via filesystem walk, parses manifests in parallel,
//! and provides graph queries: dependency closure, affected set, cycle detection.

mod discover;
mod hash;

pub use discover::{parse_pnpm_workspace, parse_pnpm_workspace_str, WorkspaceGraph};
