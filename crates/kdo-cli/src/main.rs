//! kdo CLI — context-native workspace manager for AI agents.

mod bench;
mod run;
mod setup;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use indicatif::{ProgressBar, ProgressStyle};
use kdo_context::ContextGenerator;
use kdo_core::WorkspaceConfig;
use kdo_graph::WorkspaceGraph;
use miette::IntoDiagnostic;
use owo_colors::OwoColorize;
use std::io::{self, Write};
use std::path::Path;
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
    /// Initialize a kdo workspace. Scaffolds template if empty, adopts existing repo otherwise.
    Init,

    /// Create a new project in the workspace with interactive scaffolding.
    New {
        /// Project name.
        name: String,
    },

    /// Run a named task across workspace projects.
    Run {
        /// Task name (e.g., build, test, lint, dev).
        task: String,

        /// Only run in this project (name or substring match).
        #[arg(long)]
        filter: Option<String>,

        /// Run independent projects in parallel.
        #[arg(long)]
        parallel: bool,

        /// Print the resolved pipeline without executing.
        #[arg(long)]
        dry_run: bool,

        /// Extra args appended to the resolved command (use after `--`).
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Run an arbitrary command in each project directory.
    Exec {
        /// Command to execute (quoted).
        command: String,

        /// Only run in this project (name or substring match).
        #[arg(long)]
        filter: Option<String>,

        /// Run in all projects in parallel.
        #[arg(long)]
        parallel: bool,
    },

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

    /// Validate workspace health.
    Doctor,

    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },

    /// Start the MCP server.
    Serve {
        /// Transport type.
        #[arg(long, default_value = "stdio")]
        transport: String,

        /// Agent profile: `claude`, `openclaw`, or `generic`. Tunes context budget,
        /// loop-detection window, and tool description length for the target agent.
        #[arg(long, default_value = "generic")]
        agent: String,
    },

    /// Find projects structurally similar to the given one.
    Similar {
        /// Project name.
        project: String,

        /// Number of results to return.
        #[arg(long, default_value = "5")]
        limit: usize,

        /// Output format.
        #[arg(long, default_value = "table")]
        format: OutputFormat,
    },

    /// Look up a symbol's definition across the workspace.
    Source {
        /// Symbol name (function, struct, class, type).
        symbol: String,

        /// Only search this project.
        #[arg(long)]
        filter: Option<String>,
    },

    /// Benchmark token consumption: baseline (filesystem walk) vs kdo (MCP).
    Bench {
        /// Only run tasks whose name contains this substring.
        #[arg(long)]
        task: Option<String>,

        /// Repeat each measurement this many times and report the median.
        #[arg(long, default_value = "1")]
        iterations: usize,

        /// Instead of proxy measurement, parse a real agent session log
        /// (Claude Code JSONL) and report observed token usage.
        #[arg(long, value_name = "PATH")]
        from_log: Option<std::path::PathBuf>,
    },

    /// Wire kdo into a coding agent's config (Claude Code or OpenClaw).
    Setup {
        /// Agent to set up: `claude` or `openclaw`.
        agent: String,

        /// Write to user-level config instead of workspace-level.
        #[arg(long)]
        global: bool,

        /// Print every file + command that would change, without touching disk.
        #[arg(long)]
        dry_run: bool,
    },

    /// Upgrade kdo to the latest release (or a specific version).
    Upgrade {
        /// Install a specific version (e.g. `0.1.0-alpha.3`). Default: latest release.
        #[arg(long)]
        version: Option<String>,

        /// Show what would happen without changing the binary.
        #[arg(long)]
        dry_run: bool,
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
        .with_writer(io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => cmd_init()?,
        Commands::New { name } => cmd_new(&name)?,
        Commands::Run {
            task,
            filter,
            parallel,
            dry_run,
            args,
        } => cmd_run(&task, filter.as_deref(), parallel, dry_run, &args)?,
        Commands::Exec {
            command,
            filter,
            parallel,
        } => cmd_exec(&command, filter.as_deref(), parallel)?,
        Commands::List { format } => cmd_list(format)?,
        Commands::Graph { format } => cmd_graph(format)?,
        Commands::Context {
            project,
            budget,
            format,
        } => cmd_context(&project, budget, format)?,
        Commands::Affected { base, format } => cmd_affected(&base, format)?,
        Commands::Doctor => cmd_doctor()?,
        Commands::Completions { shell } => cmd_completions(shell)?,
        Commands::Serve { transport, agent } => cmd_serve(&transport, &agent)?,
        Commands::Similar {
            project,
            limit,
            format,
        } => cmd_similar(&project, limit, format)?,
        Commands::Source { symbol, filter } => cmd_source(&symbol, filter.as_deref())?,
        Commands::Bench {
            task,
            iterations,
            from_log,
        } => bench::cmd_bench(task.as_deref(), iterations, from_log.as_deref())?,
        Commands::Setup {
            agent,
            global,
            dry_run,
        } => setup::cmd_setup(&agent, global, dry_run)?,
        Commands::Upgrade { version, dry_run } => cmd_upgrade(version.as_deref(), dry_run)?,
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// .kdo/ and kdo.toml management
// ---------------------------------------------------------------------------

const KDO_TOML: &str = "kdo.toml";
const KDO_DIR: &str = ".kdo";
const KDO_CONTEXT_DIR: &str = ".kdo/context";
const KDO_CACHE_DIR: &str = ".kdo/cache";
const KDO_GRAPH_CACHE: &str = ".kdo/graph.json";
const KDOIGNORE_FILE: &str = ".kdoignore";

/// Create the `.kdo/` cache directory structure.
fn create_kdo_dir(root: &Path) -> miette::Result<()> {
    std::fs::create_dir_all(root.join(KDO_CONTEXT_DIR)).into_diagnostic()?;
    std::fs::create_dir_all(root.join(KDO_CACHE_DIR)).into_diagnostic()?;
    Ok(())
}

/// Write `kdo.toml` at workspace root.
///
/// Generates a richly-commented template whose tasks match the languages detected
/// in the workspace. The resulting file both runs out of the box and teaches the
/// reader the full schema (env, aliases, depends_on, per-project overrides).
fn write_kdo_toml(
    root: &Path,
    workspace_name: &str,
    projects: &[String],
    languages: &std::collections::HashSet<kdo_core::Language>,
) -> miette::Result<()> {
    let path = root.join(KDO_TOML);
    if path.exists() {
        info!(path = %path.display(), "kdo.toml already exists, leaving it alone");
        return Ok(());
    }

    let (build_cmd, test_cmd, lint_cmd, fmt_cmd, dev_cmd) = detect_default_tasks(languages);

    let projects_line = if projects.is_empty() {
        "# (no projects yet — run `kdo new <name>` to scaffold one)".to_string()
    } else {
        format!("# Projects: {}", projects.join(", "))
    };

    let content = format!(
        r#"# kdo workspace configuration
# https://github.com/vivekpal1/kdo
#
{projects_line}

[workspace]
name = "{workspace_name}"
# Restrict project discovery to specific globs (optional — default scans everything):
# projects = ["apps/*", "packages/*", "crates/*"]
# exclude  = ["legacy/**", "archive/**"]

# Short aliases: `kdo run b` → `kdo run build`.
[aliases]
b = "build"
t = "test"
l = "lint"

# Workspace-wide environment (merged into every task invocation).
# Loaded before `[env]`; keys here win over env_files.
# [env]
# RUST_BACKTRACE = "1"
# env_files = [".env", ".env.local"]

# ─────────────────────────── TASKS ───────────────────────────
# Tasks can be declared two ways:
#
#   1. Bare command:
#        build = "cargo build"
#
#   2. Full spec with pipeline semantics:
#        [tasks.build]
#        command     = "cargo build"
#        depends_on  = ["^build"]          # "^task" = run `task` in every
#                                          #          upstream dep project first
#                                          # "task"  = same project, earlier step
#                                          # "//task"= workspace-wide task first
#        inputs      = ["src/**", "Cargo.toml"]
#        outputs     = ["target/debug/"]
#        cache       = true                # reserved for future cache backend
#        persistent  = false               # long-running (dev server) — don't block
#        env         = {{ RUST_LOG = "info" }}

[tasks]
build = "{build}"
test  = "{test}"
lint  = "{lint}"
fmt   = "{fmt}"
dev   = "{dev}"

# Example pipeline (uncomment to use):
# [tasks.ci]
# depends_on = ["lint", "test", "build"]

# ────────────────────── PER-PROJECT OVERRIDES ─────────────────
# Override tasks or env for a specific project:
# [projects.my-service]
# env = {{ DATABASE_URL = "postgres://localhost/myservice_dev" }}
#
# [projects.my-service.tasks]
# build = "cargo build --release --features prod"
"#,
        build = build_cmd,
        test = test_cmd,
        lint = lint_cmd,
        fmt = fmt_cmd,
        dev = dev_cmd,
    );

    std::fs::write(&path, content).into_diagnostic()?;
    info!(path = %path.display(), "wrote kdo.toml");
    Ok(())
}

/// Pick sensible default commands based on languages present in the workspace.
fn detect_default_tasks(
    languages: &std::collections::HashSet<kdo_core::Language>,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    use kdo_core::Language;
    let has = |l: &Language| languages.contains(l);

    if has(&Language::Rust) || has(&Language::Anchor) {
        (
            "cargo build",
            "cargo test",
            "cargo clippy --all-targets -- -D warnings",
            "cargo fmt --all",
            "cargo run",
        )
    } else if has(&Language::TypeScript) || has(&Language::JavaScript) {
        (
            "npm run build",
            "npm test",
            "npm run lint",
            "npm run format",
            "npm run dev",
        )
    } else if has(&Language::Python) {
        (
            "python -m build",
            "python -m pytest",
            "ruff check .",
            "ruff format .",
            "python -m app",
        )
    } else if has(&Language::Go) {
        (
            "go build ./...",
            "go test ./...",
            "golangci-lint run",
            "gofmt -w .",
            "go run .",
        )
    } else {
        (
            "echo 'configure build in kdo.toml'",
            "echo 'configure test in kdo.toml'",
            "echo 'configure lint in kdo.toml'",
            "echo 'configure fmt in kdo.toml'",
            "echo 'configure dev in kdo.toml'",
        )
    }
}

/// Write a `.kdoignore` file with sensible defaults.
fn write_kdoignore(root: &Path) -> miette::Result<()> {
    let ignore_path = root.join(KDOIGNORE_FILE);
    if ignore_path.exists() {
        return Ok(());
    }
    let content = "\
node_modules/
target/
dist/
build/
__pycache__/
.git/
.kdo/
*.lock
";
    std::fs::write(&ignore_path, content).into_diagnostic()?;
    info!(path = %ignore_path.display(), "created .kdoignore");
    Ok(())
}

/// Ensure `.gitignore` has kdo entries and language-specific patterns.
fn ensure_gitignore(
    root: &Path,
    languages: &std::collections::HashSet<kdo_core::Language>,
) -> miette::Result<()> {
    let gitignore_path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();

    let mut additions = String::new();

    // kdo entries
    if !existing.contains(".kdo") {
        additions.push_str("\n# kdo\n.kdo/\nTODO.md\n");
    }

    // Rust / Anchor
    if (languages.contains(&kdo_core::Language::Rust)
        || languages.contains(&kdo_core::Language::Anchor))
        && !existing.contains("target/")
    {
        additions.push_str("\n# Rust\ntarget/\n");
    }

    // Node / TypeScript / JavaScript
    if (languages.contains(&kdo_core::Language::TypeScript)
        || languages.contains(&kdo_core::Language::JavaScript))
        && !existing.contains("node_modules")
    {
        additions.push_str("\n# Node\nnode_modules/\ndist/\n.next/\n");
    }

    // Python
    if languages.contains(&kdo_core::Language::Python) && !existing.contains("__pycache__") {
        additions.push_str("\n# Python\n__pycache__/\n*.pyc\n.venv/\n");
    }

    // Go
    if languages.contains(&kdo_core::Language::Go) && !existing.contains("vendor/") {
        additions.push_str("\n# Go\nvendor/\n*.test\n");
    }

    // Common
    if !existing.contains(".DS_Store") {
        additions.push_str("\n# OS\n.DS_Store\n");
    }

    if !additions.is_empty() {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&gitignore_path)
            .into_diagnostic()?;
        file.write_all(additions.as_bytes()).into_diagnostic()?;
    }
    Ok(())
}

/// Generate context files into `.kdo/context/`.
fn generate_all_context(root: &Path, graph: &WorkspaceGraph) -> miette::Result<usize> {
    let context_dir = root.join(KDO_CONTEXT_DIR);
    std::fs::create_dir_all(&context_dir).into_diagnostic()?;

    let projects = graph.projects();
    let pb = ProgressBar::new(projects.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "  {spinner:.cyan} context {bar:30.cyan/blue} {pos}/{len} {msg}",
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));

    let mut count = 0;
    for project in &projects {
        pb.set_message(project.name.clone());
        let bundle = kdo_context::generate_context(graph, &project.name, 4096);
        if let Ok(bundle) = bundle {
            let md = kdo_context::render_context_md(&bundle);
            let context_path = context_dir.join(format!("{}.md", project.name));
            if std::fs::write(&context_path, &md).is_ok() {
                count += 1;
            }
        }
        pb.inc(1);
    }
    pb.finish_and_clear();

    // Cache graph snapshot
    let graph_output = graph.to_graph_output();
    if let Ok(json) = serde_json::to_string_pretty(&graph_output) {
        let _ = std::fs::write(root.join(KDO_GRAPH_CACHE), json);
    }

    Ok(count)
}

/// Load workspace config, or return default.
fn load_config(root: &Path) -> WorkspaceConfig {
    let path = root.join(KDO_TOML);
    WorkspaceConfig::load(&path).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn discover_graph() -> miette::Result<(WorkspaceGraph, std::path::PathBuf)> {
    let root = std::env::current_dir().into_diagnostic()?;
    let graph = WorkspaceGraph::discover(&root).map_err(|e| miette::miette!("{e}"))?;
    graph.detect_cycles().map_err(|e| miette::miette!("{e}"))?;
    Ok((graph, root))
}

fn cmd_init() -> miette::Result<()> {
    let root = std::env::current_dir().into_diagnostic()?;
    let workspace_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".into());

    let has_manifests = has_any_manifest(&root);

    // Create .kdo/ cache structure
    create_kdo_dir(&root)?;
    write_kdoignore(&root)?;

    if has_manifests {
        // Existing repo — discover and adopt
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::with_template("  {spinner:.cyan} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        spinner.enable_steady_tick(std::time::Duration::from_millis(80));
        spinner.set_message("discovering workspace…");

        let graph = WorkspaceGraph::discover(&root).map_err(|e| {
            spinner.finish_and_clear();
            miette::miette!("{e}")
        })?;
        spinner.finish_and_clear();
        let project_names: Vec<String> = graph.projects().iter().map(|p| p.name.clone()).collect();
        let project_count = project_names.len();

        // Collect detected languages for gitignore generation
        let languages: std::collections::HashSet<kdo_core::Language> = graph
            .projects()
            .iter()
            .map(|p| p.language.clone())
            .collect();
        ensure_gitignore(&root, &languages)?;

        write_kdo_toml(&root, &workspace_name, &project_names, &languages)?;
        let ctx_count = generate_all_context(&root, &graph)?;

        eprintln!(
            "{} Initialized workspace with {} projects.",
            "kdo".cyan().bold(),
            project_count.to_string().green().bold()
        );
        eprintln!("  {} kdo.toml         workspace config", "create".green());
        eprintln!(
            "  {} .kdo/context/    {} context files",
            "create".green(),
            ctx_count
        );
        eprintln!("  {} .kdoignore       ignore rules", "create".green());
        eprintln!("  {} .gitignore       updated", "create".green());
    } else {
        // Empty directory — scaffold template
        let empty = std::collections::HashSet::new();
        ensure_gitignore(&root, &empty)?;
        write_kdo_toml(&root, &workspace_name, &[], &empty)?;

        eprintln!("{} Initialized empty workspace.", "kdo".cyan().bold());
        eprintln!("  {} kdo.toml         workspace config", "create".green());
        eprintln!("  {} .kdo/            cache directory", "create".green());
        eprintln!("  {} .kdoignore       ignore rules", "create".green());
        eprintln!();
        eprintln!(
            "  Run {} to create your first project.",
            "kdo new <name>".yellow().bold()
        );
    }

    Ok(())
}

/// Check if any manifest files exist under root.
fn has_any_manifest(root: &Path) -> bool {
    let manifest_names = [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "Anchor.toml",
    ];
    for name in &manifest_names {
        if root.join(name).exists() {
            return true;
        }
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let dir = entry.path();
                let dir_name = dir.file_name().unwrap_or_default().to_string_lossy();
                if matches!(
                    dir_name.as_ref(),
                    "node_modules" | "target" | ".git" | ".kdo" | "dist"
                ) {
                    continue;
                }
                for name in &manifest_names {
                    if dir.join(name).exists() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn cmd_new(name: &str) -> miette::Result<()> {
    let root = std::env::current_dir().into_diagnostic()?;
    let project_dir = root.join(name);

    if project_dir.exists() {
        miette::bail!("directory '{}' already exists", name);
    }

    let language = prompt_select(
        "Language",
        &["rust", "typescript", "python", "anchor", "go"],
    )?;
    let project_type = prompt_select("Type", &["library", "binary"])?;

    let framework = match language.as_str() {
        "typescript" => prompt_select("Framework", &["none", "react", "next"])?,
        "python" => prompt_select("Framework", &["none", "fastapi", "cli"])?,
        "go" => prompt_select("Framework", &["none", "http", "cli"])?,
        "anchor" => "anchor".to_string(),
        _ => "none".to_string(),
    };

    scaffold_project(&project_dir, name, &language, &project_type, &framework)?;

    // Re-discover and update context
    if root.join(KDO_DIR).exists() {
        if let Ok(graph) = WorkspaceGraph::discover(&root) {
            let _ = generate_all_context(&root, &graph);
        }
    }

    eprintln!(
        "\n{} Created {} ({}{})",
        "kdo".cyan().bold(),
        name.green().bold(),
        language,
        if framework != "none" {
            format!("/{framework}")
        } else {
            String::new()
        }
    );
    eprintln!("  path: {}", project_dir.display().to_string().dimmed());

    Ok(())
}

fn cmd_run(
    task: &str,
    filter: Option<&str>,
    parallel: bool,
    dry_run: bool,
    extra_args: &[String],
) -> miette::Result<()> {
    let (graph, root) = discover_graph()?;
    let config = load_config(&root);

    let mode = if dry_run {
        "dry-run".magenta().to_string()
    } else if parallel {
        "parallel".dimmed().to_string()
    } else {
        "sequential".dimmed().to_string()
    };
    eprintln!(
        "{} {} {} {}",
        "kdo".cyan().bold(),
        "run".bold(),
        task.yellow().bold(),
        mode
    );

    run::run_task(&graph, &config, task, filter, parallel, dry_run, extra_args)
}

fn cmd_exec(command: &str, filter: Option<&str>, parallel: bool) -> miette::Result<()> {
    let (graph, _root) = discover_graph()?;

    eprintln!(
        "{} {} {}",
        "kdo".cyan().bold(),
        "exec".bold(),
        command.dimmed()
    );

    run::exec_command(&graph, command, filter, parallel)
}

fn cmd_list(format: OutputFormat) -> miette::Result<()> {
    let (graph, _root) = discover_graph()?;
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
                eprintln!("{}", "No projects found.".yellow());
            } else {
                eprintln!(
                    "{} {} projects\n",
                    "kdo".cyan().bold(),
                    rows.len().to_string().green().bold()
                );
                println!("{}", Table::new(&rows));
            }
        }
    }

    Ok(())
}

fn cmd_graph(format: GraphFormat) -> miette::Result<()> {
    let (graph, _root) = discover_graph()?;

    match format {
        GraphFormat::Text => print!("{}", graph.to_text()),
        GraphFormat::Json => {
            let output = graph.to_graph_output();
            let json = serde_json::to_string_pretty(&output).into_diagnostic()?;
            println!("{json}");
        }
        GraphFormat::Dot => print!("{}", graph.to_dot()),
    }

    Ok(())
}

fn cmd_context(project: &str, budget: usize, format: OutputFormat) -> miette::Result<()> {
    let (graph, root) = discover_graph()?;
    let bundle = kdo_context::generate_context(&graph, project, budget)
        .map_err(|e| miette::miette!("{e}"))?;

    // Cache to .kdo/context/
    let kdo_context_dir = root.join(KDO_CONTEXT_DIR);
    if kdo_context_dir.exists() {
        let md = kdo_context::render_context_md(&bundle);
        let context_path = kdo_context_dir.join(format!("{project}.md"));
        let _ = std::fs::write(context_path, &md);
    }

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
    let (graph, _root) = discover_graph()?;
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
                eprintln!(
                    "{} No projects affected since {}.",
                    "kdo".cyan().bold(),
                    base.yellow()
                );
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

fn cmd_doctor() -> miette::Result<()> {
    let root = std::env::current_dir().into_diagnostic()?;
    let mut issues = 0;
    let mut warnings = 0;

    eprintln!("{}", "kdo doctor".cyan().bold());
    eprintln!();

    // Check kdo.toml
    let kdo_toml = root.join(KDO_TOML);
    if kdo_toml.exists() {
        match WorkspaceConfig::load(&kdo_toml) {
            Ok(config) => {
                eprintln!(
                    "  {} kdo.toml (workspace: {})",
                    "ok".green(),
                    config.workspace.name
                );
            }
            Err(e) => {
                eprintln!("  {} kdo.toml: {}", "err".red(), e);
                issues += 1;
            }
        }
    } else {
        eprintln!("  {} kdo.toml not found. Run `kdo init`.", "warn".yellow());
        warnings += 1;
    }

    // Check .kdo/ directory
    if root.join(KDO_DIR).exists() {
        eprintln!("  {} .kdo/ cache directory", "ok".green());
    } else {
        eprintln!("  {} .kdo/ not found. Run `kdo init`.", "warn".yellow());
        warnings += 1;
    }

    // Check .kdoignore
    if root.join(KDOIGNORE_FILE).exists() {
        eprintln!("  {} .kdoignore", "ok".green());
    } else {
        eprintln!("  {} .kdoignore not found.", "warn".yellow());
        warnings += 1;
    }

    // Check .gitignore includes .kdo/
    let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    if gitignore.contains(".kdo") {
        eprintln!("  {} .gitignore includes .kdo/", "ok".green());
    } else {
        eprintln!(
            "  {} .kdo/ not in .gitignore (cache may be committed)",
            "warn".yellow()
        );
        warnings += 1;
    }

    // Discover and check graph
    match WorkspaceGraph::discover(&root) {
        Ok(graph) => {
            let projects = graph.projects();
            eprintln!("  {} {} projects discovered", "ok".green(), projects.len());

            match graph.detect_cycles() {
                Ok(()) => eprintln!("  {} no circular dependencies", "ok".green()),
                Err(e) => {
                    eprintln!("  {} {}", "err".red(), e);
                    issues += 1;
                }
            }

            // Check context freshness
            let context_dir = root.join(KDO_CONTEXT_DIR);
            if context_dir.exists() {
                let mut stale = 0;
                for project in &projects {
                    let ctx_path = context_dir.join(format!("{}.md", project.name));
                    if !ctx_path.exists() {
                        stale += 1;
                    }
                }
                if stale > 0 {
                    eprintln!(
                        "  {} {} projects missing context files. Run `kdo init` to regenerate.",
                        "warn".yellow(),
                        stale
                    );
                    warnings += 1;
                } else {
                    eprintln!("  {} all context files present", "ok".green());
                }
            }

            // Check git status
            let git_check = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&root)
                .output();
            match git_check {
                Ok(output) if output.status.success() => {
                    let changes = String::from_utf8_lossy(&output.stdout);
                    let change_count = changes.lines().count();
                    if change_count > 0 {
                        eprintln!("  {} {} uncommitted changes", "info".blue(), change_count);
                    } else {
                        eprintln!("  {} git working tree clean", "ok".green());
                    }
                }
                _ => {
                    eprintln!("  {} not a git repository", "info".blue());
                }
            }
        }
        Err(e) => {
            eprintln!("  {} workspace discovery failed: {}", "err".red(), e);
            issues += 1;
        }
    }

    eprintln!();
    if issues > 0 {
        eprintln!(
            "  {} {} issues, {} warnings",
            "FAIL".red().bold(),
            issues,
            warnings
        );
        miette::bail!("{issues} issues found");
    } else if warnings > 0 {
        eprintln!("  {} {} warnings", "WARN".yellow().bold(), warnings);
    } else {
        eprintln!("  {} workspace is healthy", "PASS".green().bold());
    }

    Ok(())
}

fn cmd_completions(shell: Shell) -> miette::Result<()> {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "kdo", &mut io::stdout());
    Ok(())
}

fn cmd_serve(transport: &str, agent: &str) -> miette::Result<()> {
    let profile: kdo_mcp::AgentProfile = agent
        .parse()
        .map_err(|e: kdo_mcp::UnknownAgent| miette::miette!("{e}"))?;

    match transport {
        "stdio" => {
            let root = std::env::current_dir().into_diagnostic()?;
            let graph = WorkspaceGraph::discover(&root).map_err(|e| miette::miette!("{e}"))?;
            let ctx_gen = ContextGenerator::new();

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .into_diagnostic()?;
            runtime
                .block_on(kdo_mcp::run_stdio(graph, ctx_gen, root, profile))
                .map_err(|e| miette::miette!("{e}"))?;
        }
        other => {
            miette::bail!("unsupported transport: {other}. Only 'stdio' is supported.");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// similar / source / upgrade
// ---------------------------------------------------------------------------

#[derive(Tabled)]
struct SimilarRow {
    #[tabled(rename = "Project")]
    name: String,
    #[tabled(rename = "Language")]
    language: String,
    #[tabled(rename = "Score")]
    score: String,
    #[tabled(rename = "Shared deps")]
    shared: String,
}

/// Find projects structurally similar to `project_name`.
/// Similarity = (same language bonus) + Jaccard(dependency sets).
fn cmd_similar(project_name: &str, limit: usize, format: OutputFormat) -> miette::Result<()> {
    let (graph, _root) = discover_graph()?;
    let target = graph
        .get_project(project_name)
        .map_err(|e| miette::miette!("{e}"))?;
    let target_deps = graph
        .dependency_closure(project_name)
        .map_err(|e| miette::miette!("{e}"))?;
    let target_dep_names: std::collections::HashSet<String> =
        target_deps.iter().map(|p| p.name.clone()).collect();

    let mut scored: Vec<(f32, &kdo_core::Project, Vec<String>)> = Vec::new();
    for candidate in graph.projects() {
        if candidate.name == target.name {
            continue;
        }
        let cand_deps = graph
            .dependency_closure(&candidate.name)
            .map_err(|e| miette::miette!("{e}"))?;
        let cand_dep_names: std::collections::HashSet<String> =
            cand_deps.iter().map(|p| p.name.clone()).collect();

        let shared: Vec<String> = target_dep_names
            .intersection(&cand_dep_names)
            .cloned()
            .collect();
        let union = target_dep_names.union(&cand_dep_names).count().max(1);
        let jaccard = shared.len() as f32 / union as f32;
        let lang_bonus = if candidate.language == target.language {
            0.5
        } else {
            0.0
        };
        let score = jaccard + lang_bonus;
        if score > 0.0 {
            scored.push((score, candidate, shared));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    match format {
        OutputFormat::Json => {
            let json: Vec<serde_json::Value> = scored
                .iter()
                .map(|(score, p, shared)| {
                    serde_json::json!({
                        "name": p.name,
                        "language": p.language.to_string(),
                        "score": score,
                        "shared_deps": shared,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json).into_diagnostic()?);
        }
        OutputFormat::Table => {
            if scored.is_empty() {
                eprintln!("{}", "No similar projects found.".yellow());
                return Ok(());
            }
            let rows: Vec<SimilarRow> = scored
                .iter()
                .map(|(score, p, shared)| SimilarRow {
                    name: p.name.clone(),
                    language: p.language.to_string(),
                    score: format!("{score:.2}"),
                    shared: if shared.is_empty() {
                        "—".into()
                    } else {
                        shared.join(", ")
                    },
                })
                .collect();
            eprintln!(
                "{} projects similar to {}",
                "kdo".cyan().bold(),
                project_name.yellow().bold()
            );
            println!("{}", Table::new(rows));
        }
    }
    Ok(())
}

/// Look up a symbol's definition across the workspace by grepping source files.
fn cmd_source(symbol: &str, filter: Option<&str>) -> miette::Result<()> {
    let (graph, _root) = discover_graph()?;
    let projects: Vec<&kdo_core::Project> = graph
        .projects()
        .into_iter()
        .filter(|p| match filter {
            Some(f) => p.name == f || p.name.contains(f),
            None => true,
        })
        .collect();

    if projects.is_empty() {
        miette::bail!("no projects matched filter");
    }

    let patterns = build_symbol_patterns(symbol);
    let mut hits: Vec<SourceHit> = Vec::new();

    for project in &projects {
        for abs in walk_source_files(&project.path) {
            let Ok(content) = std::fs::read_to_string(&abs) else {
                continue;
            };
            let rel = abs.strip_prefix(&project.path).unwrap_or(&abs);
            for (line_no, line) in content.lines().enumerate() {
                if patterns.iter().any(|p| line.contains(p)) {
                    hits.push(SourceHit {
                        project: project.name.clone(),
                        file: rel.display().to_string(),
                        line: line_no + 1,
                        snippet: line.trim().to_string(),
                    });
                }
            }
        }
    }

    if hits.is_empty() {
        eprintln!(
            "{} No definition of {} found.",
            "kdo".cyan().bold(),
            symbol.yellow().bold()
        );
        return Ok(());
    }

    eprintln!(
        "{} {} hits for {}",
        "kdo".cyan().bold(),
        hits.len().to_string().green().bold(),
        symbol.yellow().bold()
    );
    for hit in &hits {
        println!(
            "  {}:{} {}",
            format!("{}/{}", hit.project, hit.file).cyan(),
            hit.line.to_string().yellow(),
            hit.snippet.dimmed()
        );
    }
    Ok(())
}

struct SourceHit {
    project: String,
    file: String,
    line: usize,
    snippet: String,
}

/// Walk a project directory for source files we care about, honoring `.gitignore`
/// / `.kdoignore` via the `ignore` crate.
fn walk_source_files(project_path: &Path) -> Vec<std::path::PathBuf> {
    const SOURCE_EXTS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go"];
    let mut builder = ignore::WalkBuilder::new(project_path);
    ignore::WalkBuilder::hidden(&mut builder, true);
    ignore::WalkBuilder::git_ignore(&mut builder, true);
    builder.add_custom_ignore_filename(".kdoignore");
    builder.filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            "node_modules" | "target" | ".git" | ".kdo" | "dist" | "build" | "__pycache__"
        )
    });

    builder
        .build()
        .filter_map(|e| e.ok())
        .filter(|e: &ignore::DirEntry| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.into_path())
        .filter(|p: &std::path::PathBuf| {
            p.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| SOURCE_EXTS.contains(&ext))
        })
        .collect()
}

/// Build language-agnostic definition-ish patterns for a symbol.
fn build_symbol_patterns(symbol: &str) -> Vec<String> {
    vec![
        format!("fn {symbol}"),
        format!("fn {symbol}("),
        format!("pub fn {symbol}"),
        format!("struct {symbol}"),
        format!("enum {symbol}"),
        format!("trait {symbol}"),
        format!("type {symbol}"),
        format!("class {symbol}"),
        format!("interface {symbol}"),
        format!("def {symbol}("),
        format!("function {symbol}"),
        format!("export function {symbol}"),
        format!("export class {symbol}"),
        format!("export const {symbol}"),
        format!("export type {symbol}"),
        format!("const {symbol} ="),
        format!("func {symbol}("),
    ]
}

/// Upgrade kdo in place by downloading the latest release binary.
fn cmd_upgrade(version: Option<&str>, dry_run: bool) -> miette::Result<()> {
    let current_exe = std::env::current_exe().into_diagnostic()?;
    let platform = detect_platform()?;
    let target_version = match version {
        Some(v) => format!("v{}", v.trim_start_matches('v')),
        None => fetch_latest_tag()?,
    };

    let current = env!("CARGO_PKG_VERSION");
    let stripped = target_version.trim_start_matches('v');
    eprintln!(
        "{} {} → {}",
        "kdo upgrade".cyan().bold(),
        current.dimmed(),
        stripped.yellow().bold()
    );

    if stripped == current {
        eprintln!("  {} already at {current}.", "ok".green());
        return Ok(());
    }

    let archive = format!("kdo-{target_version}-{platform}.tar.gz");
    let url =
        format!("https://github.com/vivekpal1/kdo/releases/download/{target_version}/{archive}");
    eprintln!("  {} {url}", "url".dimmed());
    eprintln!("  {} {}", "target".dimmed(), current_exe.display());

    if dry_run {
        eprintln!("  {} no changes made.", "dry-run".magenta());
        return Ok(());
    }

    let tmp_dir = std::env::temp_dir().join(format!("kdo-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).into_diagnostic()?;
    let archive_path = tmp_dir.join(&archive);

    eprintln!("  {} downloading…", "»".bold());
    download_to_file(&url, &archive_path)?;

    eprintln!("  {} extracting…", "»".bold());
    let status = std::process::Command::new("tar")
        .arg("xzf")
        .arg(&archive_path)
        .arg("-C")
        .arg(&tmp_dir)
        .status()
        .into_diagnostic()?;
    if !status.success() {
        miette::bail!("failed to extract archive");
    }

    let new_binary = tmp_dir.join("kdo");
    if !new_binary.exists() {
        miette::bail!("extracted archive did not contain a `kdo` binary");
    }

    // Atomic replace: write to temp file next to current, then rename.
    let backup = current_exe.with_extension("old");
    std::fs::rename(&current_exe, &backup).into_diagnostic()?;
    if let Err(e) = std::fs::copy(&new_binary, &current_exe) {
        // Roll back on failure.
        let _ = std::fs::rename(&backup, &current_exe);
        return Err(miette::miette!("failed to install new binary: {e}"));
    }
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_dir_all(&tmp_dir);

    eprintln!(
        "  {} installed kdo {}.",
        "ok".green(),
        stripped.yellow().bold()
    );
    Ok(())
}

/// Match the install-script naming convention.
fn detect_platform() -> miette::Result<&'static str> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => miette::bail!(
            "no prebuilt binary for {os}/{arch}. Install from source with `cargo install kdo`."
        ),
    }
}

/// Fetch the most recent release tag (including prereleases) from the GitHub API.
///
/// `/releases/latest` skips prereleases, which is wrong for an alpha project.
/// We hit `/releases` (list, newest first) and take the first `tag_name`.
fn fetch_latest_tag() -> miette::Result<String> {
    let url = "https://api.github.com/repos/vivekpal1/kdo/releases?per_page=1";
    let curl = std::process::Command::new("curl")
        .args(["-fsSL", "-H", "User-Agent: kdo-upgrade", url])
        .output()
        .into_diagnostic()?;
    if !curl.status.success() {
        miette::bail!("failed to query GitHub releases API — does the repo have any releases yet?");
    }
    let body = String::from_utf8_lossy(&curl.stdout);
    let needle = "\"tag_name\":";
    let start = body.find(needle).ok_or_else(|| {
        miette::miette!("no releases found — install a specific version with --version")
    })?;
    let after = &body[start + needle.len()..];
    let q1 = after
        .find('"')
        .ok_or_else(|| miette::miette!("malformed release response"))?;
    let tail = &after[q1 + 1..];
    let q2 = tail
        .find('"')
        .ok_or_else(|| miette::miette!("malformed release response"))?;
    Ok(tail[..q2].to_string())
}

/// Download via curl (dependency-light; curl ships on macOS/Linux by default).
fn download_to_file(url: &str, dest: &Path) -> miette::Result<()> {
    let status = std::process::Command::new("curl")
        .args(["-fsSL", url, "-o"])
        .arg(dest)
        .status()
        .into_diagnostic()?;
    if !status.success() {
        miette::bail!("download failed for {url}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive prompts
// ---------------------------------------------------------------------------

fn prompt_select(label: &str, options: &[&str]) -> miette::Result<String> {
    eprint!("  {} ", label.bold());
    for (i, opt) in options.iter().enumerate() {
        if i == 0 {
            eprint!("[{}]", opt.green());
        } else {
            eprint!(" / {opt}");
        }
    }
    eprint!(": ");
    io::stderr().flush().into_diagnostic()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).into_diagnostic()?;
    let input = input.trim();

    if input.is_empty() {
        return Ok(options[0].to_string());
    }

    for opt in options {
        if opt.starts_with(input) {
            return Ok(opt.to_string());
        }
    }

    Ok(input.to_string())
}

// ---------------------------------------------------------------------------
// Project scaffolding
// ---------------------------------------------------------------------------

fn scaffold_project(
    dir: &Path,
    name: &str,
    language: &str,
    project_type: &str,
    framework: &str,
) -> miette::Result<()> {
    let src_dir = dir.join("src");
    std::fs::create_dir_all(&src_dir).into_diagnostic()?;

    match language {
        "rust" => scaffold_rust(dir, &src_dir, name, project_type)?,
        "typescript" => scaffold_typescript(dir, &src_dir, name, framework)?,
        "python" => scaffold_python(dir, &src_dir, name, framework)?,
        "anchor" => scaffold_anchor(dir, &src_dir, name)?,
        "go" => scaffold_go(dir, name, framework)?,
        _ => scaffold_rust(dir, &src_dir, name, project_type)?,
    }

    Ok(())
}

fn scaffold_rust(dir: &Path, src_dir: &Path, name: &str, project_type: &str) -> miette::Result<()> {
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
description = ""

[dependencies]
"#
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml).into_diagnostic()?;

    let (filename, content) = if project_type == "binary" {
        (
            "main.rs",
            format!(
                "//! {name} binary.\n\nfn main() {{\n    println!(\"hello from {name}\");\n}}\n"
            ),
        )
    } else {
        (
            "lib.rs",
            format!(
                "//! {name} library.\n\npub fn hello() -> &'static str {{\n    \"{name}\"\n}}\n\n#[cfg(test)]\nmod tests {{\n    use super::*;\n\n    #[test]\n    fn it_works() {{\n        assert_eq!(hello(), \"{name}\");\n    }}\n}}\n"
            ),
        )
    };
    std::fs::write(src_dir.join(filename), content).into_diagnostic()?;
    Ok(())
}

fn scaffold_typescript(
    dir: &Path,
    src_dir: &Path,
    name: &str,
    framework: &str,
) -> miette::Result<()> {
    let mut deps = serde_json::json!({});
    let mut dev_deps = serde_json::json!({ "typescript": "^5.0.0" });
    let mut scripts =
        serde_json::json!({ "build": "tsc", "dev": "tsc --watch", "test": "echo 'no tests'" });

    match framework {
        "react" => {
            deps = serde_json::json!({ "react": "^18.0.0", "react-dom": "^18.0.0" });
            dev_deps = serde_json::json!({ "typescript": "^5.0.0", "@types/react": "^18.0.0", "@types/react-dom": "^18.0.0" });
        }
        "next" => {
            deps = serde_json::json!({ "next": "^14.0.0", "react": "^18.0.0", "react-dom": "^18.0.0" });
            dev_deps = serde_json::json!({ "typescript": "^5.0.0", "@types/react": "^18.0.0" });
            scripts = serde_json::json!({ "dev": "next dev", "build": "next build", "start": "next start", "test": "echo 'no tests'" });
        }
        _ => {}
    }

    let package_json = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "description": "",
        "main": "src/index.ts",
        "scripts": scripts,
        "dependencies": deps,
        "devDependencies": dev_deps
    });

    std::fs::write(
        dir.join("package.json"),
        serde_json::to_string_pretty(&package_json).into_diagnostic()?,
    )
    .into_diagnostic()?;

    let tsconfig = serde_json::json!({
        "compilerOptions": {
            "target": "ES2020",
            "module": "commonjs",
            "strict": true,
            "outDir": "./dist",
            "declaration": true
        },
        "include": ["src/**/*"]
    });
    std::fs::write(
        dir.join("tsconfig.json"),
        serde_json::to_string_pretty(&tsconfig).into_diagnostic()?,
    )
    .into_diagnostic()?;

    let index_content = format!(
        "/**\n * {name}\n */\n\nexport function hello(): string {{\n  return \"{name}\";\n}}\n"
    );
    std::fs::write(src_dir.join("index.ts"), index_content).into_diagnostic()?;
    Ok(())
}

fn scaffold_python(dir: &Path, src_dir: &Path, name: &str, framework: &str) -> miette::Result<()> {
    let snake_name = name.replace('-', "_");

    let mut deps = vec![];
    match framework {
        "fastapi" => {
            deps.push("\"fastapi>=0.100\"".to_string());
            deps.push("\"uvicorn>=0.23\"".to_string());
        }
        "cli" => {
            deps.push("\"click>=8.0\"".to_string());
        }
        _ => {}
    }

    let deps_str = deps.join(",\n    ");
    let pyproject = format!(
        r#"[project]
name = "{name}"
version = "0.1.0"
description = ""
dependencies = [
    {deps_str}
]

[project.optional-dependencies]
dev = [
    "pytest>=7.0",
    "ruff>=0.1",
]
"#
    );

    std::fs::write(dir.join("pyproject.toml"), pyproject).into_diagnostic()?;

    // Remove src/ for Python — use flat layout
    let _ = std::fs::remove_dir(src_dir);

    let py_content = match framework {
        "fastapi" => format!(
            "\"\"\"{name} — FastAPI application.\"\"\"\n\nfrom fastapi import FastAPI\n\napp = FastAPI(title=\"{name}\")\n\n\n@app.get(\"/\")\ndef root():\n    return {{\"message\": \"hello from {name}\"}}\n"
        ),
        "cli" => format!(
            "\"\"\"{name} — CLI application.\"\"\"\n\nimport click\n\n\n@click.group()\ndef cli():\n    \"\"\"{name} CLI.\"\"\"\n\n\n@cli.command()\ndef hello():\n    \"\"\"Say hello.\"\"\"\n    click.echo(\"hello from {name}\")\n\n\nif __name__ == \"__main__\":\n    cli()\n"
        ),
        _ => format!(
            "\"\"\"{name} library.\"\"\"\n\n\ndef hello() -> str:\n    \"\"\"Return greeting.\"\"\"\n    return \"{name}\"\n"
        ),
    };

    std::fs::write(dir.join(format!("{snake_name}.py")), py_content).into_diagnostic()?;
    Ok(())
}

fn scaffold_go(dir: &Path, name: &str, framework: &str) -> miette::Result<()> {
    let module_path = format!("github.com/user/{name}");

    let go_mod = format!("module {module_path}\n\ngo 1.21\n");
    std::fs::write(dir.join("go.mod"), go_mod).into_diagnostic()?;

    let main_content = match framework {
        "http" => format!(
            "package main\n\nimport (\n\t\"fmt\"\n\t\"net/http\"\n)\n\nfunc main() {{\n\thttp.HandleFunc(\"/\", func(w http.ResponseWriter, r *http.Request) {{\n\t\tfmt.Fprintf(w, \"hello from {name}\")\n\t}})\n\thttp.ListenAndServe(\":8080\", nil)\n}}\n"
        ),
        "cli" => format!(
            "package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n\nfunc main() {{\n\tif len(os.Args) > 1 {{\n\t\tfmt.Println(\"hello,\", os.Args[1])\n\t\treturn\n\t}}\n\tfmt.Println(\"hello from {name}\")\n}}\n"
        ),
        _ => format!(
            "package main\n\nimport \"fmt\"\n\n// Hello returns a greeting from {name}.\nfunc Hello() string {{\n\treturn \"hello from {name}\"\n}}\n\nfunc main() {{\n\tfmt.Println(Hello())\n}}\n"
        ),
    };

    std::fs::write(dir.join("main.go"), main_content).into_diagnostic()?;

    // Simple test file
    let test_content =
        "package main\n\nimport \"testing\"\n\nfunc TestHello(t *testing.T) {\n\tif got := Hello(); got == \"\" {\n\t\tt.Error(\"Hello() returned empty string\")\n\t}\n}\n";
    std::fs::write(dir.join("main_test.go"), test_content).into_diagnostic()?;

    Ok(())
}

fn scaffold_anchor(dir: &Path, src_dir: &Path, name: &str) -> miette::Result<()> {
    let snake_name = name.replace('-', "_");

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
description = "Solana program built with Anchor"

[dependencies]
"#
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml).into_diagnostic()?;

    let lib_content = format!(
        r#"//! {name} — Solana program.

/// Program state account.
pub struct State {{
    pub authority: [u8; 32],
    pub data: u64,
}}

/// Initialize the program state.
pub fn initialize(authority: [u8; 32]) -> Result<(), ()> {{
    let _ = authority;
    Ok(())
}}
"#
    );
    std::fs::write(src_dir.join("lib.rs"), lib_content).into_diagnostic()?;

    let anchor_toml = format!(
        r#"[features]
seeds = false

[programs.localnet]
{snake_name} = "11111111111111111111111111111111"

[provider]
cluster = "Localnet"
wallet = "~/.config/solana/id.json"
"#
    );
    std::fs::write(dir.join("Anchor.toml"), anchor_toml).into_diagnostic()?;
    Ok(())
}
