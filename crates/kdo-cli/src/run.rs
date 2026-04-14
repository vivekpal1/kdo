//! Task execution engine — `kdo run` and `kdo exec`.
//!
//! Runs tasks in topological order across workspace projects, expanding task
//! `depends_on` into a linear DAG-respecting plan. Supports env merging,
//! aliases, per-project overrides, persistent tasks, and pass-through args.

use kdo_core::{Project, TaskSpec, WorkspaceConfig};
use kdo_graph::WorkspaceGraph;
use owo_colors::OwoColorize;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::Stdio;

/// Colors for project name prefixes (cycle through these).
const PROJECT_COLORS: &[&str] = &["cyan", "green", "yellow", "magenta", "blue", "red"];

/// A single execution step in the run plan.
#[derive(Debug, Clone)]
struct Step {
    project: Project,
    task_name: String,
    command: String,
    env: BTreeMap<String, String>,
    persistent: bool,
}

/// Run a named task. Resolves aliases, expands `depends_on`, and executes the
/// resulting plan. `extra_args` is appended to every resolved command (for
/// `kdo run build -- --release` pass-through). When `dry_run` is true, prints
/// the plan without executing.
pub fn run_task(
    graph: &WorkspaceGraph,
    config: &WorkspaceConfig,
    task_name: &str,
    filter: Option<&str>,
    parallel: bool,
    dry_run: bool,
    extra_args: &[String],
) -> miette::Result<()> {
    let resolved_name = config.resolve_alias(task_name);
    let projects = get_target_projects(graph, filter);

    if projects.is_empty() {
        eprintln!("{}", "No projects matched filter.".yellow());
        return Ok(());
    }

    let workspace_env = merge_workspace_env(config);
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut plan: Vec<Step> = Vec::new();

    for project in &projects {
        plan_task(
            graph,
            config,
            &workspace_env,
            project,
            resolved_name,
            extra_args,
            &mut visited,
            &mut plan,
        )?;
    }

    if plan.is_empty() {
        miette::bail!("task '{task_name}' not found in any project or kdo.toml");
    }

    if dry_run {
        print_plan(&plan);
        return Ok(());
    }

    let failures = if parallel {
        run_parallel(&plan)?
    } else {
        run_sequential(&plan)?
    };

    if !failures.is_empty() {
        miette::bail!("{} task(s) failed: {}", failures.len(), failures.join(", "));
    }

    Ok(())
}

/// Print the resolved plan without executing — useful for validating pipelines.
fn print_plan(plan: &[Step]) {
    eprintln!(
        "{} {} steps",
        "plan:".cyan().bold(),
        plan.len().to_string().yellow().bold()
    );
    for (i, step) in plan.iter().enumerate() {
        let num = format!("{:>2}.", i + 1);
        eprintln!(
            "  {} {} {} {}",
            num.dimmed(),
            step.project.name.cyan().bold(),
            format!(":{}", step.task_name).yellow(),
            step.command.dimmed()
        );
        if !step.env.is_empty() {
            let envs: Vec<String> = step.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            eprintln!("     {} {}", "env".dimmed(), envs.join(" ").dimmed());
        }
        if step.persistent {
            eprintln!("     {}", "persistent".yellow());
        }
    }
}

/// Recursively plan a task for a project, expanding `depends_on` first.
#[allow(clippy::too_many_arguments)]
fn plan_task(
    graph: &WorkspaceGraph,
    config: &WorkspaceConfig,
    workspace_env: &BTreeMap<String, String>,
    project: &Project,
    task_name: &str,
    extra_args: &[String],
    visited: &mut HashSet<(String, String)>,
    plan: &mut Vec<Step>,
) -> miette::Result<()> {
    let key = (project.name.clone(), task_name.to_string());
    if visited.contains(&key) {
        return Ok(());
    }
    visited.insert(key);

    let resolved = resolve_task(config, project, task_name);

    // Expand dependencies before emitting this step.
    if let Some((Some(spec), _)) = &resolved {
        for dep in spec.depends_on() {
            if let Some(upstream_task) = dep.strip_prefix('^') {
                let upstream = graph
                    .dependency_closure(&project.name)
                    .map_err(|e| miette::miette!("{e}"))?;
                for dep_project in upstream {
                    plan_task(
                        graph,
                        config,
                        workspace_env,
                        dep_project,
                        upstream_task,
                        extra_args,
                        visited,
                        plan,
                    )?;
                }
            } else if let Some(workspace_task) = dep.strip_prefix("//") {
                for project_ref in graph.projects() {
                    plan_task(
                        graph,
                        config,
                        workspace_env,
                        project_ref,
                        workspace_task,
                        extra_args,
                        visited,
                        plan,
                    )?;
                }
            } else {
                plan_task(
                    graph,
                    config,
                    workspace_env,
                    project,
                    dep,
                    extra_args,
                    visited,
                    plan,
                )?;
            }
        }
    }

    let Some((spec_opt, mut command)) = resolved else {
        return Ok(());
    };

    // Composite tasks (depends_on only, no command) emit no step.
    if command.is_empty() {
        return Ok(());
    }

    if !extra_args.is_empty() {
        command.push(' ');
        command.push_str(&shell_quote_args(extra_args));
    }

    let mut env = workspace_env.clone();
    if let Some(project_cfg) = config.projects.get(&project.name) {
        for (k, v) in &project_cfg.env {
            env.insert(k.clone(), v.clone());
        }
    }
    let persistent = if let Some(spec) = &spec_opt {
        for (k, v) in spec.env() {
            env.insert(k.clone(), v.clone());
        }
        spec.persistent()
    } else {
        false
    };

    plan.push(Step {
        project: project.clone(),
        task_name: task_name.to_string(),
        command,
        env,
        persistent,
    });
    Ok(())
}

/// Resolve a task for a project. Returns `(task_spec_if_rich, command_string)`.
/// Precedence: per-project override > workspace task > manifest script > language default.
fn resolve_task(
    config: &WorkspaceConfig,
    project: &Project,
    task_name: &str,
) -> Option<(Option<TaskSpec>, String)> {
    if let Some(project_cfg) = config.projects.get(&project.name) {
        if let Some(spec) = project_cfg.tasks.get(task_name) {
            return Some((Some(spec.clone()), spec.command().unwrap_or("").to_string()));
        }
    }
    if let Some(spec) = config.tasks.get(task_name) {
        return Some((Some(spec.clone()), spec.command().unwrap_or("").to_string()));
    }
    if let Some(cmd) = resolve_task_command(project, task_name) {
        return Some((None, cmd));
    }
    None
}

/// Merge workspace-level env: env_files first, then `[env]` keys on top.
fn merge_workspace_env(config: &WorkspaceConfig) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for path in &config.env_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    env.insert(k.trim().to_string(), v.to_string());
                }
            }
        }
    }
    for (k, v) in &config.env {
        env.insert(k.clone(), v.clone());
    }
    env
}

/// POSIX shell-quote arguments so they survive `sh -c`.
fn shell_quote_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.chars()
                .all(|c| c.is_alphanumeric() || "-_./:=".contains(c))
            {
                a.clone()
            } else {
                let escaped = a.replace('\'', "'\\''");
                format!("'{escaped}'")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    let plan: Vec<Step> = projects
        .into_iter()
        .map(|p| Step {
            project: p.clone(),
            task_name: "exec".to_string(),
            command: command.to_string(),
            env: BTreeMap::new(),
            persistent: false,
        })
        .collect();

    let failures = if parallel {
        run_parallel(&plan)?
    } else {
        run_sequential(&plan)?
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
fn get_target_projects<'a>(graph: &'a WorkspaceGraph, filter: Option<&str>) -> Vec<&'a Project> {
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

/// Run steps sequentially, printing prefixed output.
fn run_sequential(plan: &[Step]) -> miette::Result<Vec<String>> {
    let mut failures = Vec::new();
    for (i, step) in plan.iter().enumerate() {
        let prefix = format_prefix(
            &step.project.name,
            &step.task_name,
            i % PROJECT_COLORS.len(),
        );
        eprintln!("{prefix} {}", step.command.dimmed());
        let success = execute_step(step)?;
        if success {
            eprintln!("{prefix} {}", "done".green());
        } else if step.persistent {
            eprintln!("{prefix} {}", "persistent task exited".yellow());
        } else {
            eprintln!("{prefix} {}", "FAILED".red().bold());
            failures.push(step.project.name.clone());
        }
    }
    Ok(failures)
}

/// Run steps in parallel using rayon, collecting failures.
fn run_parallel(plan: &[Step]) -> miette::Result<Vec<String>> {
    use rayon::prelude::*;
    use std::sync::Mutex;

    let failures = Mutex::new(Vec::new());

    plan.par_iter().enumerate().for_each(|(i, step)| {
        let prefix = format_prefix(
            &step.project.name,
            &step.task_name,
            i % PROJECT_COLORS.len(),
        );
        eprintln!("{prefix} {}", step.command.dimmed());
        match execute_step(step) {
            Ok(true) => eprintln!("{prefix} {}", "done".green()),
            Ok(false) if step.persistent => {
                eprintln!("{prefix} {}", "persistent task exited".yellow());
            }
            Ok(false) => {
                eprintln!("{prefix} {}", "FAILED".red().bold());
                failures.lock().unwrap().push(step.project.name.clone());
            }
            Err(e) => {
                eprintln!("{prefix} {} {e}", "ERROR".red().bold());
                failures.lock().unwrap().push(step.project.name.clone());
            }
        }
    });

    Ok(failures.into_inner().unwrap())
}

/// Try to resolve a task command from a project's manifest or language defaults.
pub fn resolve_task_command(project: &Project, task_name: &str) -> Option<String> {
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

/// Execute a single plan step in its project directory with merged env.
fn execute_step(step: &Step) -> miette::Result<bool> {
    use miette::IntoDiagnostic;

    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", &step.command])
        .current_dir(&step.project.path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for (k, v) in &step.env {
        cmd.env(k, v);
    }
    let status = cmd.status().into_diagnostic()?;
    Ok(status.success())
}

/// Format a colored `[project:task]` prefix.
fn format_prefix(project: &str, task: &str, color_idx: usize) -> String {
    let label = format!("{project}:{task}");
    let colored = match PROJECT_COLORS[color_idx] {
        "cyan" => label.cyan().bold().to_string(),
        "green" => label.green().bold().to_string(),
        "yellow" => label.yellow().bold().to_string(),
        "magenta" => label.magenta().bold().to_string(),
        "blue" => label.blue().bold().to_string(),
        "red" => label.red().bold().to_string(),
        _ => label.bold().to_string(),
    };
    format!("[{colored}]")
}
