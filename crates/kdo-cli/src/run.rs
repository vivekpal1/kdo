//! Task execution engine — `kdo run` and `kdo exec`.
//!
//! Runs tasks in topological order across workspace projects,
//! with colored output prefixes and optional parallel execution.

use kdo_core::WorkspaceConfig;
use kdo_graph::WorkspaceGraph;
use owo_colors::OwoColorize;
use std::path::Path;
use std::process::Stdio;

/// Colors for project name prefixes (cycle through these).
const PROJECT_COLORS: &[&str] = &["cyan", "green", "yellow", "magenta", "blue", "red"];

/// Run a named task from `kdo.toml` or project manifests.
///
/// Projects execute in topological order (dependencies first). Pass `parallel = true`
/// to run independent stages concurrently with rayon.
pub fn run_task(
    graph: &WorkspaceGraph,
    config: &WorkspaceConfig,
    task_name: &str,
    filter: Option<&str>,
    parallel: bool,
) -> miette::Result<()> {
    let projects = get_target_projects(graph, filter);

    if projects.is_empty() {
        eprintln!("{}", "No projects matched filter.".yellow());
        return Ok(());
    }

    // Check workspace-level task first
    let workspace_cmd = config.tasks.get(task_name).cloned();

    // Collect (project, command) pairs — skip projects without a matching task
    let work: Vec<(kdo_core::Project, String)> = projects
        .into_iter()
        .filter_map(|p| {
            let cmd = resolve_task_command(p, task_name).or_else(|| workspace_cmd.clone())?;
            Some((p.clone(), cmd))
        })
        .collect();

    if work.is_empty() {
        miette::bail!("task '{task_name}' not found in any project or kdo.toml");
    }

    let failures = if parallel {
        run_parallel(&work)?
    } else {
        run_sequential(&work)?
    };

    if !failures.is_empty() {
        miette::bail!("{} task(s) failed: {}", failures.len(), failures.join(", "));
    }

    Ok(())
}

/// Run an arbitrary command in each project directory.
pub fn exec_command(
    graph: &WorkspaceGraph,
    command: &str,
    filter: Option<&str>,
    parallel: bool,
) -> miette::Result<()> {
    let projects = get_target_projects(graph, filter);

    if projects.is_empty() {
        eprintln!("{}", "No projects matched filter.".yellow());
        return Ok(());
    }

    let work: Vec<(kdo_core::Project, String)> = projects
        .into_iter()
        .map(|p| (p.clone(), command.to_string()))
        .collect();

    let failures = if parallel {
        run_parallel(&work)?
    } else {
        run_sequential(&work)?
    };

    if !failures.is_empty() {
        miette::bail!(
            "{} command(s) failed: {}",
            failures.len(),
            failures.join(", ")
        );
    }

    Ok(())
}

/// Get target projects in topological order, optionally filtered by name.
fn get_target_projects<'a>(
    graph: &'a WorkspaceGraph,
    filter: Option<&str>,
) -> Vec<&'a kdo_core::Project> {
    let ordered = graph.topological_order();
    if let Some(filter_name) = filter {
        ordered
            .into_iter()
            .filter(|p| p.name == filter_name || p.name.contains(filter_name))
            .collect()
    } else {
        ordered
    }
}

/// Run tasks sequentially, printing prefixed output.
fn run_sequential(work: &[(kdo_core::Project, String)]) -> miette::Result<Vec<String>> {
    let mut failures = Vec::new();
    for (i, (project, cmd)) in work.iter().enumerate() {
        let prefix = format_prefix(&project.name, i % PROJECT_COLORS.len());
        eprintln!("{prefix} {}", cmd.dimmed());
        let success = execute_in_dir(&project.path, cmd, &prefix)?;
        if success {
            eprintln!("{prefix} {}", "done".green());
        } else {
            eprintln!("{prefix} {}", "FAILED".red().bold());
            failures.push(project.name.clone());
        }
    }
    Ok(failures)
}

/// Run tasks in parallel using rayon, collecting failures.
fn run_parallel(work: &[(kdo_core::Project, String)]) -> miette::Result<Vec<String>> {
    use rayon::prelude::*;
    use std::sync::Mutex;

    let failures = Mutex::new(Vec::new());

    work.par_iter().enumerate().for_each(|(i, (project, cmd))| {
        let prefix = format_prefix(&project.name, i % PROJECT_COLORS.len());
        eprintln!("{prefix} {}", cmd.dimmed());
        match execute_in_dir(&project.path, cmd, &prefix) {
            Ok(true) => eprintln!("{prefix} {}", "done".green()),
            Ok(false) => {
                eprintln!("{prefix} {}", "FAILED".red().bold());
                failures.lock().unwrap().push(project.name.clone());
            }
            Err(e) => {
                eprintln!("{prefix} {} {e}", "ERROR".red().bold());
                failures.lock().unwrap().push(project.name.clone());
            }
        }
    });

    Ok(failures.into_inner().unwrap())
}

/// Try to resolve a task command from a project's manifest or language defaults.
pub fn resolve_task_command(project: &kdo_core::Project, task_name: &str) -> Option<String> {
    match project.language {
        kdo_core::Language::Rust | kdo_core::Language::Anchor => match task_name {
            "build" => Some("cargo build".into()),
            "test" => Some("cargo test".into()),
            "check" => Some("cargo check".into()),
            "lint" => Some("cargo clippy".into()),
            "fmt" => Some("cargo fmt".into()),
            "clean" => Some("cargo clean".into()),
            _ => None,
        },
        kdo_core::Language::TypeScript | kdo_core::Language::JavaScript => {
            let pkg_path = project.manifest_path.clone();
            if let Ok(content) = std::fs::read_to_string(&pkg_path) {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                    if pkg
                        .get("scripts")
                        .and_then(|s| s.get(task_name))
                        .and_then(|v| v.as_str())
                        .is_some()
                    {
                        let pm = detect_node_pm(&project.path);
                        return Some(format!("{pm} run {task_name}"));
                    }
                }
            }
            match task_name {
                "build" => Some("npm run build".into()),
                "test" => Some("npm test".into()),
                "lint" => Some("npm run lint".into()),
                "dev" => Some("npm run dev".into()),
                _ => None,
            }
        }
        kdo_core::Language::Python => {
            let py = detect_python();
            match task_name {
                "test" => Some(format!("{py} -m pytest")),
                "lint" => Some("ruff check .".into()),
                "fmt" => Some("ruff format .".into()),
                "build" => Some(format!("{py} -c \"print('no build step for Python')\"")),
                _ => None,
            }
        }
        kdo_core::Language::Go => match task_name {
            "build" => Some("go build ./...".into()),
            "test" => Some("go test ./...".into()),
            "lint" => Some("golangci-lint run".into()),
            "fmt" => Some("gofmt -w .".into()),
            "check" => Some("go vet ./...".into()),
            _ => None,
        },
    }
}

/// Detect python binary (python3 preferred over python).
pub fn detect_python() -> &'static str {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "python3"
    } else {
        "python"
    }
}

/// Detect which Node package manager to use based on lockfile presence.
pub fn detect_node_pm(project_dir: &Path) -> &'static str {
    if project_dir.join("bun.lockb").exists() || project_dir.join("bun.lock").exists() {
        "bun"
    } else if project_dir.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if project_dir.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    }
}

/// Execute a shell command in a directory, streaming output.
fn execute_in_dir(dir: &Path, command: &str, _prefix: &str) -> miette::Result<bool> {
    use miette::IntoDiagnostic;

    let status = std::process::Command::new("sh")
        .args(["-c", command])
        .current_dir(dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .into_diagnostic()?;

    Ok(status.success())
}

/// Format a colored project name prefix.
fn format_prefix(name: &str, color_idx: usize) -> String {
    let colored_name = match PROJECT_COLORS[color_idx] {
        "cyan" => name.cyan().bold().to_string(),
        "green" => name.green().bold().to_string(),
        "yellow" => name.yellow().bold().to_string(),
        "magenta" => name.magenta().bold().to_string(),
        "blue" => name.blue().bold().to_string(),
        "red" => name.red().bold().to_string(),
        _ => name.bold().to_string(),
    };
    format!("[{colored_name}]")
}
