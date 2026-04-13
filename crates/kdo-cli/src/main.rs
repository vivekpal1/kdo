//! kdo CLI — context-native workspace manager for AI agents.

use clap::{Parser, Subcommand, ValueEnum};
use kdo_context::ContextGenerator;
use kdo_graph::WorkspaceGraph;
use miette::IntoDiagnostic;
use tabled::{Table, Tabled};
use tracing::info;

#[derive(Parser)]
#[command(name = "kdo", version, about = "Workspace manager for the agent era")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan workspace, create kdo.toml, generate CONTEXT.md per project.
    Init,

    /// List all projects in the workspace.
    List {
        /// Output format.
        #[arg(long, default_value = "table")]
        format: OutputFormat,
    },

    /// Show the dependency graph.
    Graph {
        /// Output format.
        #[arg(long, default_value = "text")]
        format: GraphFormat,
    },

    /// Generate a context bundle for a project within a token budget.
    Context {
        /// Project name.
        project: String,

        /// Token budget.
        #[arg(long, default_value = "4096")]
        budget: usize,

        /// Output format.
        #[arg(long, default_value = "table")]
        format: OutputFormat,
    },

    /// List projects affected by changes since a git ref.
    Affected {
        /// Git base ref.
        #[arg(long, default_value = "main")]
        base: String,

        /// Output format.
        #[arg(long, default_value = "table")]
        format: OutputFormat,
    },

    /// Start the MCP server.
    Serve {
        /// Transport type.
        #[arg(long, default_value = "stdio")]
        transport: String,
    },
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

#[derive(Clone, ValueEnum)]
enum GraphFormat {
    Text,
    Json,
    Dot,
}

#[derive(Tabled)]
struct ProjectRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Language")]
    language: String,
    #[tabled(rename = "Summary")]
    summary: String,
    #[tabled(rename = "Deps")]
    dep_count: usize,
}

#[derive(Tabled)]
struct AffectedRow {
    #[tabled(rename = "Project")]
    name: String,
}

fn main() -> miette::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => cmd_init()?,
        Commands::List { format } => cmd_list(format)?,
        Commands::Graph { format } => cmd_graph(format)?,
        Commands::Context {
            project,
            budget,
            format,
        } => cmd_context(&project, budget, format)?,
        Commands::Affected { base, format } => cmd_affected(&base, format)?,
        Commands::Serve { transport } => cmd_serve(&transport)?,
    }

    Ok(())
}

fn discover_graph() -> miette::Result<WorkspaceGraph> {
    let root = std::env::current_dir().into_diagnostic()?;
    let graph = WorkspaceGraph::discover(&root).map_err(|e| miette::miette!("{e}"))?;
    graph.detect_cycles().map_err(|e| miette::miette!("{e}"))?;
    Ok(graph)
}

fn cmd_init() -> miette::Result<()> {
    let root = std::env::current_dir().into_diagnostic()?;
    let graph = WorkspaceGraph::discover(&root).map_err(|e| miette::miette!("{e}"))?;

    // Create kdo.toml
    let kdo_toml = root.join("kdo.toml");
    let mut toml_content = String::from("[workspace]\nname = ");
    let workspace_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".into());
    toml_content.push_str(&format!("\"{workspace_name}\"\n\n"));
    toml_content.push_str("[projects]\n");
    for project in graph.projects() {
        toml_content.push_str(&format!("# {} ({})\n", project.name, project.language));
    }
    std::fs::write(&kdo_toml, &toml_content).into_diagnostic()?;
    info!(path = %kdo_toml.display(), "created kdo.toml");

    // Generate CONTEXT.md for each project
    let _ctx_gen = ContextGenerator::new();
    for project in graph.projects() {
        let bundle = kdo_context::generate_context(&graph, &project.name, 4096);
        if let Ok(bundle) = bundle {
            let md = kdo_context::render_context_md(&bundle);
            let context_path = project.path.join("CONTEXT.md");
            if let Err(e) = std::fs::write(&context_path, &md) {
                tracing::warn!(path = %context_path.display(), error = %e, "failed to write CONTEXT.md");
            } else {
                info!(project = %project.name, path = %context_path.display(), "generated CONTEXT.md");
            }
        }
    }

    let project_count = graph.projects().len();
    eprintln!("Initialized kdo workspace with {project_count} projects.");
    eprintln!("Created kdo.toml and CONTEXT.md files.");
    Ok(())
}

fn cmd_list(format: OutputFormat) -> miette::Result<()> {
    let graph = discover_graph()?;
    let summaries = graph.project_summaries();

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&summaries).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Table => {
            let rows: Vec<ProjectRow> = summaries
                .iter()
                .map(|s| ProjectRow {
                    name: s.name.clone(),
                    language: s.language.clone(),
                    summary: s
                        .summary
                        .as_deref()
                        .unwrap_or("-")
                        .chars()
                        .take(50)
                        .collect(),
                    dep_count: s.dep_count,
                })
                .collect();

            if rows.is_empty() {
                eprintln!("No projects found.");
            } else {
                println!("{}", Table::new(&rows));
            }
        }
    }

    Ok(())
}

fn cmd_graph(format: GraphFormat) -> miette::Result<()> {
    let graph = discover_graph()?;

    match format {
        GraphFormat::Text => {
            print!("{}", graph.to_text());
        }
        GraphFormat::Json => {
            let output = graph.to_graph_output();
            let json = serde_json::to_string_pretty(&output).into_diagnostic()?;
            println!("{json}");
        }
        GraphFormat::Dot => {
            print!("{}", graph.to_dot());
        }
    }

    Ok(())
}

fn cmd_context(project: &str, budget: usize, format: OutputFormat) -> miette::Result<()> {
    let graph = discover_graph()?;
    let bundle = kdo_context::generate_context(&graph, project, budget)
        .map_err(|e| miette::miette!("{e}"))?;

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&bundle).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Table => {
            let md = kdo_context::render_context_md(&bundle);
            print!("{md}");
        }
    }

    Ok(())
}

fn cmd_affected(base: &str, format: OutputFormat) -> miette::Result<()> {
    let graph = discover_graph()?;
    let affected = graph
        .affected_since_ref(base)
        .map_err(|e| miette::miette!("{e}"))?;

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&affected).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Table => {
            if affected.is_empty() {
                eprintln!("No projects affected since {base}.");
            } else {
                let rows: Vec<AffectedRow> = affected
                    .iter()
                    .map(|name| AffectedRow { name: name.clone() })
                    .collect();
                println!("{}", Table::new(&rows));
            }
        }
    }

    Ok(())
}

fn cmd_serve(transport: &str) -> miette::Result<()> {
    match transport {
        "stdio" => {
            let root = std::env::current_dir().into_diagnostic()?;
            let graph = WorkspaceGraph::discover(&root).map_err(|e| miette::miette!("{e}"))?;
            let ctx_gen = ContextGenerator::new();
            kdo_mcp::run_stdio(graph, ctx_gen).map_err(|e| miette::miette!("{e}"))?;
        }
        other => {
            miette::bail!("unsupported transport: {other}. Only 'stdio' is supported.");
        }
    }
    Ok(())
}
