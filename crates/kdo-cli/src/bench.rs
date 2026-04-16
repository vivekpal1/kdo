//! `kdo bench` — reproducible token-consumption benchmark.
//!
//! Two modes:
//!
//! 1. **proxy** (default): measures an apples-to-apples "bytes the agent must
//!    consume" for each scenario. `baseline` = sum of bytes across the files a
//!    filesystem-walking agent would read; `kdo` = actual bytes returned by
//!    calling the real kdo intelligence layer (`project_summaries` +
//!    `generate_bundle`) for the same scope. No mocking, no fabricated numbers.
//!
//! 2. **log** (`--from-log <path>`): parses a real agent session log (Claude
//!    Code JSONL) and reports observed token usage. Pair runs with/without
//!    kdo and this mode shows the real delta.
//!
//! Task definitions live at `.kdo/bench/tasks.toml`. If absent, a starter set
//! is scaffolded. Results are persisted as JSON under `.kdo/bench/results/`.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use kdo_context::ContextGenerator;
use kdo_core::estimate_tokens;
use kdo_graph::WorkspaceGraph;
use miette::{IntoDiagnostic, Result};
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};

// ─────────────────────────── Config shapes ───────────────────────────

/// Root document in `.kdo/bench/tasks.toml`.
#[derive(Debug, Deserialize)]
struct TasksDoc {
    #[serde(default)]
    task: Vec<TaskDef>,
}

/// One benchmark task.
#[derive(Debug, Deserialize)]
struct TaskDef {
    name: String,
    description: String,
    /// Projects a kdo-aware agent would pull context for.
    projects: Vec<String>,
    /// Glob patterns (relative to workspace root) representing the files a
    /// filesystem-walking agent would read in the naive baseline.
    baseline_files: Vec<String>,
}

/// One row in the results table.
#[derive(Debug, Serialize)]
struct TaskResult {
    name: String,
    description: String,
    baseline_bytes: usize,
    baseline_tokens: usize,
    kdo_bytes: usize,
    kdo_tokens: usize,
    reduction_pct: f32,
}

/// Full run output persisted to `.kdo/bench/results/<timestamp>.json`.
#[derive(Debug, Serialize)]
struct BenchRun {
    kdo_version: &'static str,
    timestamp: u64,
    mode: &'static str,
    tasks: Vec<TaskResult>,
    average_reduction_pct: f32,
}

// ─────────────────────────── Entry point ───────────────────────────

pub fn cmd_bench(
    task_filter: Option<&str>,
    iterations: usize,
    from_log: Option<&Path>,
) -> Result<()> {
    if let Some(log) = from_log {
        return log_mode(log);
    }

    let root = std::env::current_dir().into_diagnostic()?;
    let tasks_path = root.join(".kdo").join("bench").join("tasks.toml");

    if !tasks_path.exists() {
        scaffold_tasks(&tasks_path)?;
    }

    let doc: TasksDoc = {
        let raw = fs::read_to_string(&tasks_path).into_diagnostic()?;
        toml::from_str(&raw)
            .map_err(|e| miette::miette!("{} is not valid TOML: {e}", tasks_path.display()))?
    };

    let graph = WorkspaceGraph::discover(&root).map_err(|e| miette::miette!("{e}"))?;
    let ctx_gen = ContextGenerator::new();

    let mut results: Vec<TaskResult> = Vec::new();

    eprintln!(
        "{} {}",
        "kdo bench".cyan().bold(),
        format!(
            "{} tasks · {} iterations · proxy mode",
            doc.task.len(),
            iterations.max(1)
        )
        .dimmed()
    );
    eprintln!();

    for task in &doc.task {
        if let Some(f) = task_filter {
            if !task.name.contains(f) {
                continue;
            }
        }
        let mut baseline_samples = Vec::new();
        let mut kdo_samples = Vec::new();
        for _ in 0..iterations.max(1) {
            baseline_samples.push(measure_baseline(&root, &task.baseline_files)?);
            kdo_samples.push(measure_kdo(&graph, &ctx_gen, &task.projects)?);
        }
        let baseline_bytes = median(&baseline_samples);
        let kdo_bytes = median(&kdo_samples);
        let baseline_tokens = estimate_tokens(&"x".repeat(baseline_bytes));
        let kdo_tokens = estimate_tokens(&"x".repeat(kdo_bytes));
        let reduction_pct = if baseline_bytes == 0 {
            0.0
        } else {
            ((baseline_bytes - kdo_bytes.min(baseline_bytes)) as f32 / baseline_bytes as f32)
                * 100.0
        };
        results.push(TaskResult {
            name: task.name.clone(),
            description: task.description.clone(),
            baseline_bytes,
            baseline_tokens,
            kdo_bytes,
            kdo_tokens,
            reduction_pct,
        });
    }

    if results.is_empty() {
        miette::bail!("no tasks matched filter");
    }

    print_results(&results);
    persist_results(&root, "proxy", &results)?;
    Ok(())
}

// ─────────────────────────── Proxy measurements ───────────────────────────

/// Sum the bytes of every file matching the baseline globs. This is what a
/// filesystem-walking agent would have to consume.
fn measure_baseline(root: &Path, patterns: &[String]) -> Result<usize> {
    use globset::{Glob, GlobSetBuilder};
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        if let Ok(g) = Glob::new(p) {
            builder.add(g);
        }
    }
    let set = builder.build().into_diagnostic()?;

    let mut total = 0usize;
    let mut builder_w = WalkBuilder::new(root);
    WalkBuilder::hidden(&mut builder_w, true);
    WalkBuilder::git_ignore(&mut builder_w, true);
    builder_w.add_custom_ignore_filename(".kdoignore");

    for entry in builder_w.build().flatten() {
        let entry: ignore::DirEntry = entry;
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        if set.is_match(rel) {
            if let Ok(meta) = fs::metadata(path) {
                total += meta.len() as usize;
            }
        }
    }
    Ok(total)
}

/// Actual bytes kdo would return: `kdo_list_projects` summary (always first
/// call in the recommended flow) + `kdo_get_context` for each in-scope project.
fn measure_kdo(
    graph: &WorkspaceGraph,
    ctx_gen: &ContextGenerator,
    projects: &[String],
) -> Result<usize> {
    let mut total = 0usize;

    // 1. kdo_list_projects output.
    let summaries = graph.project_summaries();
    let listed = serde_json::to_string(&summaries).unwrap_or_default();
    total += listed.len();

    // 2. kdo_get_context for each project in scope. Use Claude's default budget
    // (4096) so proxy numbers match what a Claude Code user would actually get.
    const DEFAULT_BUDGET: usize = 4096;
    for project in projects {
        match ctx_gen.generate_bundle(project, DEFAULT_BUDGET, graph) {
            Ok(bundle) => total += bundle.len(),
            Err(e) => {
                eprintln!("  {} {project}: {e}", "warn".yellow());
            }
        }
    }
    Ok(total)
}

// ─────────────────────────── Log mode ───────────────────────────

fn log_mode(log: &Path) -> Result<()> {
    let raw = fs::read_to_string(log).into_diagnostic()?;

    // Claude Code session logs are JSONL. We sum `usage.input_tokens +
    // usage.output_tokens + usage.cache_read_input_tokens` across turns when
    // present. Unknown event shapes are ignored silently.
    let mut total_input: u64 = 0;
    let mut total_output: u64 = 0;
    let mut total_cache_read: u64 = 0;
    let mut turns: u64 = 0;

    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let usage = match v.get("message").and_then(|m| m.get("usage")) {
            Some(u) => u,
            None => continue,
        };
        turns += 1;
        total_input += usage
            .get("input_tokens")
            .and_then(|n| n.as_u64())
            .unwrap_or(0);
        total_output += usage
            .get("output_tokens")
            .and_then(|n| n.as_u64())
            .unwrap_or(0);
        total_cache_read += usage
            .get("cache_read_input_tokens")
            .and_then(|n| n.as_u64())
            .unwrap_or(0);
    }

    eprintln!(
        "{} {}",
        "kdo bench".cyan().bold(),
        format!("log · {} turns", turns).dimmed()
    );
    eprintln!();
    eprintln!(
        "  {:<18} {:>10}",
        "input tokens".dimmed(),
        total_input.to_string().bold()
    );
    eprintln!(
        "  {:<18} {:>10}",
        "output tokens".dimmed(),
        total_output.to_string().bold()
    );
    eprintln!(
        "  {:<18} {:>10}",
        "cache reads".dimmed(),
        total_cache_read.to_string().bold()
    );
    eprintln!();
    eprintln!(
        "  {:<18} {:>10}",
        "total".green().bold(),
        (total_input + total_output + total_cache_read)
            .to_string()
            .green()
            .bold()
    );
    Ok(())
}

// ─────────────────────────── Output ───────────────────────────

fn print_results(results: &[TaskResult]) {
    let name_width = results
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(8)
        .max(8);
    let header_name = format!("{:width$}", "Task", width = name_width);

    eprintln!(
        "  {}  {:>12}  {:>12}  {:>10}",
        header_name.bold(),
        "baseline".bold(),
        "with kdo".bold(),
        "reduction".bold()
    );
    eprintln!(
        "  {}  {:>12}  {:>12}  {:>10}",
        "─".repeat(name_width).dimmed(),
        "─".repeat(12).dimmed(),
        "─".repeat(12).dimmed(),
        "─".repeat(10).dimmed()
    );

    let mut total_baseline = 0usize;
    let mut total_kdo = 0usize;

    for r in results {
        total_baseline += r.baseline_tokens;
        total_kdo += r.kdo_tokens;
        let reduction = format!("{:.1}%", r.reduction_pct);
        eprintln!(
            "  {:width$}  {:>12}  {:>12}  {:>10}",
            r.name.cyan(),
            format_tokens(r.baseline_tokens).dimmed(),
            format_tokens(r.kdo_tokens),
            reduction.green().bold(),
            width = name_width
        );
    }

    eprintln!(
        "  {}  {:>12}  {:>12}  {:>10}",
        "─".repeat(name_width).dimmed(),
        "─".repeat(12).dimmed(),
        "─".repeat(12).dimmed(),
        "─".repeat(10).dimmed()
    );
    let avg_reduction = if total_baseline == 0 {
        0.0
    } else {
        ((total_baseline - total_kdo.min(total_baseline)) as f32 / total_baseline as f32) * 100.0
    };
    let avg_label = format!("{:.1}%", avg_reduction);
    eprintln!(
        "  {:width$}  {:>12}  {:>12}  {:>10}",
        "AVERAGE".bold(),
        format_tokens(total_baseline).bold(),
        format_tokens(total_kdo).bold(),
        avg_label.green().bold(),
        width = name_width
    );
}

fn format_tokens(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k tok", n as f32 / 1000.0)
    } else {
        format!("{n} tok")
    }
}

fn persist_results(root: &Path, mode: &'static str, results: &[TaskResult]) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .into_diagnostic()?
        .as_secs();

    let total_baseline: usize = results.iter().map(|r| r.baseline_tokens).sum();
    let total_kdo: usize = results.iter().map(|r| r.kdo_tokens).sum();
    let avg_reduction = if total_baseline == 0 {
        0.0
    } else {
        ((total_baseline - total_kdo.min(total_baseline)) as f32 / total_baseline as f32) * 100.0
    };

    let run = BenchRun {
        kdo_version: env!("CARGO_PKG_VERSION"),
        timestamp: now,
        mode,
        tasks: results.iter().map(clone_result).collect(),
        average_reduction_pct: avg_reduction,
    };

    let dir = root.join(".kdo").join("bench").join("results");
    fs::create_dir_all(&dir).into_diagnostic()?;
    let path = dir.join(format!("{now}.json"));
    let serialized = serde_json::to_string_pretty(&run).into_diagnostic()?;
    fs::write(&path, serialized).into_diagnostic()?;
    eprintln!();
    eprintln!(
        "  {} {}",
        "saved".dimmed(),
        path.strip_prefix(root).unwrap_or(&path).display()
    );
    Ok(())
}

fn clone_result(r: &TaskResult) -> TaskResult {
    TaskResult {
        name: r.name.clone(),
        description: r.description.clone(),
        baseline_bytes: r.baseline_bytes,
        baseline_tokens: r.baseline_tokens,
        kdo_bytes: r.kdo_bytes,
        kdo_tokens: r.kdo_tokens,
        reduction_pct: r.reduction_pct,
    }
}

fn scaffold_tasks(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    fs::write(path, include_str!("../templates/bench_tasks.toml")).into_diagnostic()?;
    eprintln!(
        "  {} {}  {}",
        "scaffolded".green(),
        path.display(),
        "edit it to describe your own tasks".dimmed()
    );
    Ok(())
}

fn median(samples: &[usize]) -> usize {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

// ─────────────────────────── Tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn median_is_middle() {
        assert_eq!(median(&[100, 200, 300, 400, 500]), 300);
        assert_eq!(median(&[10]), 10);
        assert_eq!(median(&[]), 0);
    }

    #[test]
    fn format_tokens_formats_k() {
        assert_eq!(format_tokens(999), "999 tok");
        assert_eq!(format_tokens(1000), "1.0k tok");
        // f32 {:.1} rounds half-to-even, so pick values that don't straddle 0.5.
        assert_eq!(format_tokens(12600), "12.6k tok");
        assert_eq!(format_tokens(12400), "12.4k tok");
    }

    #[test]
    fn baseline_sums_matching_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "hello").unwrap();
        fs::write(dir.path().join("b.rs"), "world!!").unwrap();
        fs::write(dir.path().join("c.ts"), "should not count").unwrap();

        let total = measure_baseline(dir.path(), &["*.rs".into()]).unwrap();
        assert_eq!(total, "hello".len() + "world!!".len());
    }
}
