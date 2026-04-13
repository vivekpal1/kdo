//! MCP server implementation using raw JSON-RPC 2.0 over stdio.
//!
//! Protocol: <https://modelcontextprotocol.io/specification/2025-11-25>

use kdo_context::ContextGenerator;
use kdo_graph::WorkspaceGraph;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tracing::{debug, error};

/// Tool definition for MCP tools/list response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolDef {
    name: String,
    description: String,
    input_schema: Value,
}

/// MCP server state.
struct McpServer {
    graph: Arc<WorkspaceGraph>,
    ctx_gen: Arc<ContextGenerator>,
}

impl McpServer {
    fn new(graph: WorkspaceGraph, ctx_gen: ContextGenerator) -> Self {
        Self {
            graph: Arc::new(graph),
            ctx_gen: Arc::new(ctx_gen),
        }
    }

    /// Handle a JSON-RPC request and return a response.
    fn handle_request(&self, method: &str, params: &Value, id: &Value) -> Value {
        let result = match method {
            "initialize" => self.handle_initialize(),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(params),
            "ping" => Ok(serde_json::json!({})),
            _ => Err(jsonrpc_error(
                -32601,
                &format!("method not found: {method}"),
            )),
        };

        match result {
            Ok(result) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(err) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": err,
            }),
        }
    }

    fn handle_initialize(&self) -> Result<Value, Value> {
        Ok(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "kdo",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Context-native workspace manager. Use kdo_list_projects first to orient, then kdo_get_context for a specific project within a token budget. Use kdo_read_symbol only when you need a specific function body."
        }))
    }

    fn handle_tools_list(&self) -> Result<Value, Value> {
        let tools = vec![
            ToolDef {
                name: "kdo_list_projects".into(),
                description:
                    "List all projects in the workspace with summaries (~200 tokens total).".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDef {
                name: "kdo_get_context".into(),
                description:
                    "Get agent-optimized context bundle for a project within a token budget.".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "Project name" },
                        "budget": { "type": "integer", "description": "Token budget (default 4096)" }
                    },
                    "required": ["project"]
                }),
            },
            ToolDef {
                name: "kdo_read_symbol".into(),
                description: "Read a specific symbol (function/struct/trait) body via tree-sitter."
                    .into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "Project name" },
                        "symbol": { "type": "string", "description": "Symbol name (function, struct, trait)" }
                    },
                    "required": ["project", "symbol"]
                }),
            },
            ToolDef {
                name: "kdo_dep_graph".into(),
                description:
                    "Query the dependency graph for a project. direction=deps or dependents.".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "Project name" },
                        "direction": { "type": "string", "description": "Direction: 'deps' or 'dependents' (default deps)" }
                    },
                    "required": ["project"]
                }),
            },
            ToolDef {
                name: "kdo_affected".into(),
                description: "List projects affected by git changes since base ref.".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "base_ref": { "type": "string", "description": "Git base ref (default 'main')" }
                    },
                    "required": []
                }),
            },
            ToolDef {
                name: "kdo_search_code".into(),
                description: "Search for a pattern across all workspace source files. Returns matching lines with file:line context.".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Search pattern (substring match)" },
                        "project": { "type": "string", "description": "Limit search to this project (optional)" }
                    },
                    "required": ["pattern"]
                }),
            },
        ];

        Ok(serde_json::json!({ "tools": tools }))
    }

    fn handle_tools_call(&self, params: &Value) -> Result<Value, Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| jsonrpc_error(-32602, "missing tool name"))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        debug!(tool = name, "calling tool");

        match name {
            "kdo_list_projects" => self.tool_list_projects(),
            "kdo_get_context" => self.tool_get_context(&arguments),
            "kdo_read_symbol" => self.tool_read_symbol(&arguments),
            "kdo_dep_graph" => self.tool_dep_graph(&arguments),
            "kdo_affected" => self.tool_affected(&arguments),
            "kdo_search_code" => self.tool_search_code(&arguments),
            _ => Err(jsonrpc_error(-32602, &format!("unknown tool: {name}"))),
        }
    }

    fn tool_list_projects(&self) -> Result<Value, Value> {
        let summaries = self.graph.project_summaries();
        let json = serde_json::to_string_pretty(&summaries)
            .map_err(|e| jsonrpc_error(-32603, &e.to_string()))?;
        Ok(tool_result_text(&json))
    }

    fn tool_get_context(&self, args: &Value) -> Result<Value, Value> {
        let project = args
            .get("project")
            .and_then(|v| v.as_str())
            .ok_or_else(|| jsonrpc_error(-32602, "missing 'project' argument"))?;
        let budget = args.get("budget").and_then(|v| v.as_u64()).unwrap_or(4096) as usize;

        let bundle = self
            .ctx_gen
            .generate_bundle(project, budget, &self.graph)
            .map_err(|e| jsonrpc_error(-32602, &e.to_string()))?;
        Ok(tool_result_text(&bundle))
    }

    fn tool_read_symbol(&self, args: &Value) -> Result<Value, Value> {
        let project = args
            .get("project")
            .and_then(|v| v.as_str())
            .ok_or_else(|| jsonrpc_error(-32602, "missing 'project' argument"))?;
        let symbol = args
            .get("symbol")
            .and_then(|v| v.as_str())
            .ok_or_else(|| jsonrpc_error(-32602, "missing 'symbol' argument"))?;

        let source = self
            .ctx_gen
            .read_symbol(project, symbol, &self.graph)
            .map_err(|e| jsonrpc_error(-32602, &e.to_string()))?;
        Ok(tool_result_text(&source))
    }

    fn tool_dep_graph(&self, args: &Value) -> Result<Value, Value> {
        let project = args
            .get("project")
            .and_then(|v| v.as_str())
            .ok_or_else(|| jsonrpc_error(-32602, "missing 'project' argument"))?;
        let direction = args
            .get("direction")
            .and_then(|v| v.as_str())
            .unwrap_or("deps");

        let result = match direction {
            "dependents" => self.graph.affected_set_json(project),
            _ => self.graph.dependency_closure_json(project),
        }
        .map_err(|e| jsonrpc_error(-32602, &e.to_string()))?;
        Ok(tool_result_text(&result))
    }

    fn tool_affected(&self, args: &Value) -> Result<Value, Value> {
        let base = args
            .get("base_ref")
            .and_then(|v| v.as_str())
            .unwrap_or("main");

        let projects = self
            .graph
            .affected_since_ref(base)
            .map_err(|e| jsonrpc_error(-32603, &e.to_string()))?;
        let json = serde_json::to_string_pretty(&projects)
            .map_err(|e| jsonrpc_error(-32603, &e.to_string()))?;
        Ok(tool_result_text(&json))
    }

    fn tool_search_code(&self, args: &Value) -> Result<Value, Value> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| jsonrpc_error(-32602, "missing 'pattern' argument"))?;
        let project_filter = args.get("project").and_then(|v| v.as_str());

        let projects: Vec<&kdo_core::Project> = if let Some(name) = project_filter {
            match self.graph.get_project(name) {
                Ok(p) => vec![p],
                Err(e) => return Err(jsonrpc_error(-32602, &e.to_string())),
            }
        } else {
            self.graph.projects()
        };

        let mut results = Vec::new();
        let max_results = 50;

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
                    "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "toml" | "json"
                ) {
                    continue;
                }

                if let Ok(content) = std::fs::read_to_string(path) {
                    for (i, line) in content.lines().enumerate() {
                        if line.contains(pattern) {
                            let rel_path = path
                                .strip_prefix(&self.graph.root)
                                .unwrap_or(path)
                                .display();
                            results.push(format!("{}:{}:{}", rel_path, i + 1, line.trim()));
                            if results.len() >= max_results {
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok(tool_result_text(&format!("no matches for '{pattern}'")))
        } else {
            let header = format!("{} matches for '{pattern}':\n\n", results.len());
            Ok(tool_result_text(&format!("{header}{}", results.join("\n"))))
        }
    }
}

/// Format a text result in MCP tool call response format.
fn tool_result_text(text: &str) -> Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": text }]
    })
}

/// Create a JSON-RPC error object.
fn jsonrpc_error(code: i64, message: &str) -> Value {
    serde_json::json!({
        "code": code,
        "message": message
    })
}

/// JSON-RPC request structure.
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

/// Run the MCP server on stdio. Reads JSON-RPC messages line-by-line.
pub fn run_stdio(graph: WorkspaceGraph, ctx_gen: ContextGenerator) -> anyhow::Result<()> {
    let server = McpServer::new(graph, ctx_gen);
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                error!(error = %e, "failed to parse JSON-RPC request");
                let err_resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": jsonrpc_error(-32700, &format!("parse error: {e}"))
                });
                writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        // Notifications (no id) don't get responses
        if request.id.is_none() {
            debug!(method = %request.method, "received notification");
            continue;
        }

        let params = request.params.unwrap_or(serde_json::json!({}));
        let id = request.id.unwrap_or(Value::Null);
        let response = server.handle_request(&request.method, &params, &id);

        let response_str = serde_json::to_string(&response)?;
        debug!(method = %request.method, "sending response");
        writeln!(stdout, "{response_str}")?;
        stdout.flush()?;
    }

    Ok(())
}
