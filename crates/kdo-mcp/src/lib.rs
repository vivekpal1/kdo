//! MCP server for kdo workspace manager.
//!
//! Implements the Model Context Protocol (JSON-RPC 2.0 over stdio)
//! to expose workspace intelligence to AI agents.

pub mod server;

pub use server::run_stdio;
