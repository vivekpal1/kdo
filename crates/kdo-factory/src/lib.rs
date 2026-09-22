//! kdo-factory — embedded control plane for kdo.
//!
//! A spec → run → task → agent reconciliation loop running in a single
//! tokio process, backed by SQLite. Designed to be embedded by both the
//! `kdo` CLI (dev-local at `.kdo/factory.db`) and the `kdow` Axum
//! backend (multi-tenant, scoped by workspace).
//!
//! Agents edit a git worktree. A passing review merges that branch onto
//! the current checkout only when the checkout is clean.

mod brief;
pub mod connector;
pub mod error;
mod gitutil;
pub mod keys;
pub mod merge;
mod migrations;
pub mod openai;
pub mod plugin;
pub mod reconciler;
pub mod resources;
pub mod router;
pub mod spec;
pub mod store;
pub mod tools;
mod turn;
pub mod worktree;

pub use connector::{AnthropicConnector, Completion, Connector, MockConnector};
pub use error::{FactoryError, FactoryResult};
pub use keys::{KeyStatus, KeyStore};
pub use merge::try_merge;
pub use openai::OpenAiConnector;
pub use plugin::{AgentPlugin, PluginSet, ProviderPlugin, ToolName};
pub use reconciler::{Reconciler, ReconcilerHandle};
pub use resources::{Event, Run, RunStatus, Spec, SpecStatus, Task, TaskRole, TaskStatus};
pub use router::build_connector;
pub use spec::{parse_spec, SpecDocument};
pub use store::Store;
pub use worktree::Worktree;
