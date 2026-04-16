//! MCP server for kdo workspace manager.
//!
//! Implements the Model Context Protocol via the rmcp SDK (JSON-RPC over
//! stdio) and exposes workspace intelligence as a catalog of tools + resources
//! to AI coding agents.

pub mod guards;
pub mod profile;
pub mod server;

pub use guards::{LoopError, LoopGuard};
pub use profile::{AgentProfile, UnknownAgent};
pub use server::{run_stdio, KdoServer};
