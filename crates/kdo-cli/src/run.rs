//! Task execution engine — `kdo run` and `kdo exec`.
//!
//! Runs tasks in topological order across workspace projects,
//! with colored output prefixes and parallel execution of independent projects.

use kdo_core::WorkspaceConfig;
use kdo_graph::WorkspaceGraph;
use owo_colors::OwoColorize;
use std::path::Path;
use std::process::Stdio;

/// Colors for project name prefixes (cycle through these).
const PROJECT_COLORS: &[&str] = &["cyan", "green", "yellow", "magenta", "blue", "red"];

/// Run a named task from `kdo.toml` or project manifests.
pub fn run_task(
    graph: &WorkspaceGraph,
    config: &WorkspaceConfig,
    task_name: &str,
    filter: Option<&str>,
) -> miette::Result<()> {
    let projects = get_target_projects(graph, filter)?;

    if projects.is_empty() {
        eprintln!("{}", "No projects matched filter.".yellow());
        return Ok(());
    }

    // Check workspace-level task first
    let workspace_cmd = config.tasks.get(task_name).cloned();

    let mut any_ran = false;
    let mut any_failed = false;

    for (i, project) in projects.iter().enumerate() {
        let color_idx = i % PROJECT_COLORS.len();
        let prefix = format_prefix(&project.name, color_idx);

        // Resolve command: project-specific script > workspace task
        let cmd = resolve_task_command(project, task_name).or_else(|| workspace_cmd.clone());

        let cmd = match cmd {
            Some(c) => c,
            None => continue, // No task for this project
        };

        any_ran = true;
        eprintln!("{prefix} {}", cmd.dimmed());

        let success = execute_in_dir(&project.path, &cmd, &prefix)?;
        if !success {
            eprintln!("{prefix} {}", "FAILED".red().bold());
            any_failed = true;
        } else {
            eprintln!("{prefix} {}", "done".green());
        }
    }

    if !any_ran {
        miette::bail!("task '{task_name}' not found in any project or kdo.toml");
    }

    if any_failed {
        miette::bail!("some tasks failed");
    }

    Ok(())
}

/// Run an arbitrary command in each project directory.
pub fn exec_command(
    graph: &WorkspaceGraph,
    command: &str,
    filter: Option<&str>,
) -> miette::Result<()> {
    let projects = get_target_projects(graph, filter)?;

    if projects.is_empty() {
        eprintln!("{}", "No projects matched filter.".yellow());
        return Ok(());
    }

    let mut any_failed = false;

    for (i, project) in projects.iter().enumerate() {
        let color_idx = i % PROJECT_COLORS.len();
        let prefix = format_prefix(&project.name, color_idx);

        eprintln!("{prefix} {}", command.dimmed());

        let success = execute_in_dir(&project.path, command, &prefix)?;
        if !success {
            eprintln!("{prefix} {}", "FAILED".red().bold());
            any_failed = true;
        }
    }

    if any_failed {
        miette::bail!("some commands failed");
    }

    Ok(())
}

/// Get target projects, optionally filtered by name.
fn get_target_projects(
    graph: &WorkspaceGraph,
    filter: Option<&str>,
) -> miette::Result<Vec<kdo_core::Project>> {
    let all = graph.projects();

    if let Some(filter_name) = filter {
        let matched: Vec<_> = all
            .into_iter()
            .filter(|p| p.name == filter_name || p.name.contains(filter_name))
            .cloned()
            .collect();
        Ok(matched)
    } else {
        Ok(all.into_iter().cloned().collect())
    }
}

/// Try to resolve a task command from a project's manifest.
fn resolve_task_command(project: &kdo_core::Project, task_name: &str) -> Option<String> {
    match project.language {
        kdo_core::Language::Rust | kdo_core::Language::Anchor => {
            // Cargo built-in tasks
            match task_name {
                "build" => Some("cargo build".into()),
                "test" => Some("cargo test".into()),
                "check" => Some("cargo check".into()),
                "lint" => Some("cargo clippy".into()),
                "fmt" => Some("cargo fmt".into()),
                "clean" => Some("cargo clean".into()),
                _ => None,
            }
        }
        kdo_core::Language::TypeScript | kdo_core::Language::JavaScript => {
            // Try reading scripts from package.json
            let pkg_path = project.manifest_path.clone();
            if let Ok(content) = std::fs::read_to_string(&pkg_path) {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                    if pkg
                        .get("scripts")
                        .and_then(|s| s.get(task_name))
                        .and_then(|v| v.as_str())
                        .is_some()
                    {
                        // Detect package manager
                        let pm = detect_node_pm(&project.path);
                        return Some(format!("{pm} run {task_name}"));
                    }
                }
            }
            // Fallback built-in tasks
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
    }
}

/// Detect python binary (python3 preferred over python).
fn detect_python() -> &'static str {
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

/// Detect which Node package manager to use.
fn detect_node_pm(project_dir: &Path) -> &'static str {
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

/// Execute a shell command in a directory, streaming output with prefix.
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
