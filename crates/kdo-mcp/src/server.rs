//! MCP server for kdo — built on the rmcp 0.16 tool-router + ServerHandler
//! pattern.
//!
//! - Seven tools (`kdo_list_projects`, `kdo_get_context`, `kdo_read_symbol`,
//!   `kdo_dep_graph`, `kdo_affected`, `kdo_search_code`, `kdo_run_task`) all
//!   registered via `#[tool_router]` + `#[tool]`.
//! - Resources endpoint exposes `.kdo/context/<project>.md` as
//!   `kdo://context/<project>` URIs.
//! - Every `call_tool` is gated by a [`LoopGuard`] keyed to the active
//!   [`AgentProfile`]'s window — duplicate calls surface as structured errors
//!   instead of silently burning tokens.
//!
//! Transport: stdio only for now. SSE is on the roadmap.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::{
        AnnotateAble, CallToolRequestParams, CallToolResult, Content, Implementation,
        ListResourcesResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion, RawResource,
        ReadResourceRequestParams, ReadResourceResult, Resource, ResourceContents,
        ServerCapabilities, ServerInfo,
    },
    schemars,
    service::RequestContext,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::debug;

use kdo_context::ContextGenerator;
use kdo_graph::WorkspaceGraph;

use crate::guards::LoopGuard;
use crate::profile::AgentProfile;

// ─────────────────────────── Input schemas ───────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetContextArgs {
    /// Project name (as shown by `kdo_list_projects`).
    pub project: String,
    /// Token budget. Defaults to the agent profile's preferred budget.
    #[serde(default)]
    pub budget: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadSymbolArgs {
    /// Project containing the symbol.
    pub project: String,
    /// Symbol name (function, struct, trait, class, type).
    pub symbol: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DepGraphArgs {
    /// Project to query.
    pub project: String,
    /// "deps" (what this project depends on) or "dependents" (what depends on it).
    #[serde(default)]
    pub direction: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AffectedArgs {
    /// Git base ref. Defaults to "main".
    #[serde(default)]
    pub base_ref: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchCodeArgs {
    /// Substring pattern to search for.
    pub pattern: String,
    /// Limit search to this project (optional).
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunTaskArgs {
    /// Task name: build, test, lint, fmt, check, clean.
    pub task: String,
    /// Project to run the task in.
    pub project: String,
}

// ─────────────────────────── Server ───────────────────────────

/// kdo's MCP server state.
#[derive(Clone)]
pub struct KdoServer {
    graph: Arc<WorkspaceGraph>,
    ctx_gen: Arc<ContextGenerator>,
    root: Arc<PathBuf>,
    profile: AgentProfile,
    loop_guard: Arc<Mutex<LoopGuard>>,
    tool_router: ToolRouter<Self>,
}

impl KdoServer {
    pub fn new(
        graph: WorkspaceGraph,
        ctx_gen: ContextGenerator,
        root: PathBuf,
        profile: AgentProfile,
    ) -> Self {
        let loop_guard = LoopGuard::for_profile_window(profile.loop_detection_window());
        Self {
            graph: Arc::new(graph),
            ctx_gen: Arc::new(ctx_gen),
            root: Arc::new(root),
            profile,
            loop_guard: Arc::new(Mutex::new(loop_guard)),
            tool_router: Self::tool_router(),
        }
    }

    /// Truncate a tool response to the profile's `max_tool_output_tokens` by
    /// (very rough) character-to-token estimation: 4 chars ≈ 1 token.
    fn cap_output(&self, mut text: String) -> String {
        let max_chars = self.profile.max_tool_output_tokens().saturating_mul(4);
        if text.len() > max_chars {
            text.truncate(max_chars);
            text.push_str("\n\n[truncated by kdo — response exceeded agent profile budget]");
        }
        text
    }

    /// Map any error-like into a structured MCP tool-call error.
    fn params_err<E: std::fmt::Display>(err: E) -> McpError {
        McpError::invalid_params(err.to_string(), None)
    }

    fn internal_err<E: std::fmt::Display>(err: E) -> McpError {
        McpError::internal_error(err.to_string(), None)
    }
}

// ─────────────────────────── Tools ───────────────────────────

#[rmcp::tool_router]
impl KdoServer {
    #[rmcp::tool(
        description = "List all projects in the workspace with name, language, summary, and dependency count (~200 tokens total). Call this first to orient."
    )]
    async fn kdo_list_projects(&self) -> Result<CallToolResult, McpError> {
        let summaries = self.graph.project_summaries();
        let json = serde_json::to_string_pretty(&summaries).map_err(Self::internal_err)?;
        Ok(CallToolResult::success(vec![Content::text(
            self.cap_output(json),
        )]))
    }

    #[rmcp::tool(
        description = "Get agent-optimized context bundle for a project within a token budget. Returns summary, public API signatures (via tree-sitter), and dependency list."
    )]
    async fn kdo_get_context(
        &self,
        Parameters(args): Parameters<GetContextArgs>,
    ) -> Result<CallToolResult, McpError> {
        let budget = args
            .budget
            .unwrap_or_else(|| self.profile.default_context_budget());
        let bundle = self
            .ctx_gen
            .generate_bundle(&args.project, budget, &self.graph)
            .map_err(Self::params_err)?;
        Ok(CallToolResult::success(vec![Content::text(
            self.cap_output(bundle),
        )]))
    }

    #[rmcp::tool(
        description = "Read a specific symbol (function, struct, trait, class, type) body via tree-sitter. Use after kdo_get_context when you need the implementation, not just the signature."
    )]
    async fn kdo_read_symbol(
        &self,
        Parameters(args): Parameters<ReadSymbolArgs>,
    ) -> Result<CallToolResult, McpError> {
        let source = self
            .ctx_gen
            .read_symbol(&args.project, &args.symbol, &self.graph)
            .map_err(Self::params_err)?;
        Ok(CallToolResult::success(vec![Content::text(
            self.cap_output(source),
        )]))
    }

    #[rmcp::tool(
        description = "Query the dependency graph. direction='deps' (default) for what this project depends on, 'dependents' for what depends on it."
    )]
    async fn kdo_dep_graph(
        &self,
        Parameters(args): Parameters<DepGraphArgs>,
    ) -> Result<CallToolResult, McpError> {
        let direction = args.direction.as_deref().unwrap_or("deps");
        let json = match direction {
            "dependents" => self.graph.affected_set_json(&args.project),
            _ => self.graph.dependency_closure_json(&args.project),
        }
        .map_err(Self::params_err)?;
        Ok(CallToolResult::success(vec![Content::text(
            self.cap_output(json),
        )]))
    }

    #[rmcp::tool(
        description = "Projects affected by git changes since a base ref. Uses the dependency graph — touching a leaf marks every dependent."
    )]
    async fn kdo_affected(
        &self,
        Parameters(args): Parameters<AffectedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let base = args.base_ref.as_deref().unwrap_or("main");
        let projects = self
            .graph
            .affected_since_ref(base)
            .map_err(Self::internal_err)?;
        let json = serde_json::to_string_pretty(&projects).map_err(Self::internal_err)?;
        Ok(CallToolResult::success(vec![Content::text(
            self.cap_output(json),
        )]))
    }

    #[rmcp::tool(
        description = "Substring search across every workspace source file. Respects .gitignore and .kdoignore. Returns file:line:match hits."
    )]
    async fn kdo_search_code(
        &self,
        Parameters(args): Parameters<SearchCodeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let projects: Vec<&kdo_core::Project> = if let Some(name) = &args.project {
            match self.graph.get_project(name) {
                Ok(p) => vec![p],
                Err(e) => return Err(Self::params_err(e)),
            }
        } else {
            self.graph.projects()
        };

        let mut results = Vec::new();
        let max_results = 50usize;

        'outer: for project in &projects {
            let walker = ignore::WalkBuilder::new(&project.path)
                .hidden(true)
                .git_ignore(true)
                .add_custom_ignore_filename(".kdoignore")
                .build();

            for entry in walker.flatten() {
                if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    continue;
                }
                let path = entry.path();
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(
                    ext,
                    "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "toml" | "json"
                ) {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(path) {
                    for (i, line) in content.lines().enumerate() {
                        if line.contains(&args.pattern) {
                            let rel_path = path.strip_prefix(self.root.as_ref()).unwrap_or(path);
                            results.push(format!(
                                "{}:{}:{}",
                                rel_path.display(),
                                i + 1,
                                line.trim()
                            ));
                            if results.len() >= max_results {
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        let text = if results.is_empty() {
            format!("no matches for '{}'", args.pattern)
        } else {
            format!(
                "{} matches for '{}':\n\n{}",
                results.len(),
                args.pattern,
                results.join("\n")
            )
        };
        Ok(CallToolResult::success(vec![Content::text(
            self.cap_output(text),
        )]))
    }

    #[rmcp::tool(
        description = "Execute a build/test/lint task in a workspace project via the same resolution as `kdo run`. Returns stdout, stderr, and exit status."
    )]
    async fn kdo_run_task(
        &self,
        Parameters(args): Parameters<RunTaskArgs>,
    ) -> Result<CallToolResult, McpError> {
        let project = self
            .graph
            .get_project(&args.project)
            .map_err(Self::params_err)?;

        let cmd = resolve_default_task(&project.language, &args.task).ok_or_else(|| {
            Self::params_err(format!("no '{}' task for {}", args.task, args.project))
        })?;

        debug!(project = %args.project, task = %args.task, cmd = %cmd, "running task");

        let output = tokio::process::Command::new("sh")
            .args(["-c", &cmd])
            .current_dir(&project.path)
            .output()
            .await
            .map_err(|e| Self::internal_err(format!("failed to spawn process: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);
        let success = output.status.success();

        let text = format!(
            "project: {project}\ntask: {task}\ncommand: {cmd}\nexit_code: {exit_code}\nsuccess: {success}\n\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
            project = args.project,
            task = args.task,
        );

        Ok(CallToolResult::success(vec![Content::text(
            self.cap_output(text),
        )]))
    }
}

// ─────────────────────────── ServerHandler ───────────────────────────

impl ServerHandler for KdoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            instructions: Some(self.profile.instructions().into()),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            server_info: Implementation {
                name: "kdo".to_string(),
                title: Some("kdo — workspace manager".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some(
                    "Context-native workspace manager for AI coding agents.".to_string(),
                ),
                icons: None,
                website_url: Some("https://github.com/vivekpal1/kdo".to_string()),
            },
        }
    }

    // Auto-derived from tool_router.
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    // Dispatch through the router, but first gate on the loop-detection guard.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let args_value = match &request.arguments {
            Some(obj) => serde_json::Value::Object(obj.clone()),
            None => serde_json::Value::Null,
        };

        // Gate: return the loop error to the agent instead of silently
        // re-running the same call and burning more tokens.
        {
            let mut guard = self.loop_guard.lock().await;
            if let Err(loop_err) = guard.record(&request.name, &args_value) {
                return Err(McpError::invalid_params(loop_err.to_string(), None));
            }
        }

        let ctx = ToolCallContext::new(self, request, context);
        self.tool_router.call(ctx).await
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let context_dir = self.root.join(".kdo").join("context");
        let mut resources: Vec<Resource> = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&context_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let raw = RawResource {
                    uri: format!("kdo://context/{stem}"),
                    name: format!("{stem} context"),
                    description: Some(format!(
                        "Pre-generated context bundle for project `{stem}`."
                    )),
                    mime_type: Some("text/markdown".into()),
                    ..RawResource::new(format!("kdo://context/{stem}"), format!("{stem} context"))
                };
                resources.push(raw.no_annotation());
            }
        }

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let project = request.uri.strip_prefix("kdo://context/").ok_or_else(|| {
            McpError::invalid_params(format!("unsupported uri: {}", request.uri), None)
        })?;

        // Defense-in-depth: reject path traversal / absolute paths.
        if project.is_empty() || project.contains('/') || project.contains("..") {
            return Err(McpError::invalid_params("invalid project name", None));
        }

        let path = self
            .root
            .join(".kdo")
            .join("context")
            .join(format!("{project}.md"));
        let text = std::fs::read_to_string(&path).map_err(|e| {
            McpError::internal_error(format!("failed to read {}: {e}", path.display()), None)
        })?;

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::text(text, request.uri)],
        })
    }
}

// ─────────────────────────── Transport ───────────────────────────

/// Run the MCP server on stdio. Blocks until the client disconnects.
pub async fn run_stdio(
    graph: WorkspaceGraph,
    ctx_gen: ContextGenerator,
    root: PathBuf,
    profile: AgentProfile,
) -> anyhow::Result<()> {
    let server = KdoServer::new(graph, ctx_gen, root, profile);
    let transport = rmcp::transport::stdio();
    let running = server.serve(transport).await?;
    running.waiting().await?;
    Ok(())
}

// ─────────────────────────── Helpers ───────────────────────────

/// Language-aware default task resolver. Mirrors `kdo-cli::run::resolve_task_command`
/// but intentionally re-implemented here to avoid a circular crate dep.
fn resolve_default_task(language: &kdo_core::Language, task_name: &str) -> Option<String> {
    match language {
        kdo_core::Language::Rust | kdo_core::Language::Anchor => match task_name {
            "build" => Some("cargo build".into()),
            "test" => Some("cargo test".into()),
            "lint" => Some("cargo clippy".into()),
            "fmt" => Some("cargo fmt".into()),
            "check" => Some("cargo check".into()),
            "clean" => Some("cargo clean".into()),
            _ => None,
        },
        kdo_core::Language::TypeScript | kdo_core::Language::JavaScript => match task_name {
            "build" => Some("npm run build".into()),
            "test" => Some("npm test".into()),
            "lint" => Some("npm run lint".into()),
            _ => None,
        },
        kdo_core::Language::Python => match task_name {
            "test" => Some("python3 -m pytest".into()),
            "lint" => Some("ruff check .".into()),
            "fmt" => Some("ruff format .".into()),
            _ => None,
        },
        kdo_core::Language::Go => match task_name {
            "build" => Some("go build ./...".into()),
            "test" => Some("go test ./...".into()),
            "lint" => Some("golangci-lint run".into()),
            "fmt" => Some("gofmt -w .".into()),
            _ => None,
        },
    }
}
