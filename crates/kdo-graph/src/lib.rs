//! Workspace dependency graph built on petgraph.
//!
//! Discovers projects via filesystem walk, parses manifests in parallel,
//! and provides graph queries: dependency closure, affected set, cycle detection.

mod discover;
mod hash;

pub use discover::WorkspaceGraph;
