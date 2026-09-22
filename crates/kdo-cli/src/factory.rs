//! `kdo apply` / `kdo factory ...` — CLI entry points for the factory.
//!
//! Stores live at `<workspace>/.kdo/factory.db`. The reconciler runs
//! in-process on a tokio runtime — `daemon` keeps it alive, `tick` runs
//! one pass and exits (CI-friendly).

use kdo_factory::{
    build_connector, parse_spec, Connector, KeyStore, PluginSet, Reconciler, RunStatus, Store,
    TaskStatus,
};
use miette::IntoDiagnostic;
use owo_colors::OwoColorize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tabled::{Table, Tabled};
use tokio::sync::watch;

const FACTORY_DB: &str = ".kdo/factory.db";

fn db_path() -> miette::Result<PathBuf> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    Ok(cwd.join(FACTORY_DB))
}

fn runtime() -> miette::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

fn workspace_root() -> miette::Result<PathBuf> {
    std::env::current_dir().into_diagnostic()
}

fn connector(workspace: &Path) -> miette::Result<Arc<dyn Connector>> {
    let built = build_connector(workspace).map_err(|err| miette::miette!("{err}"))?;
    eprintln!(
        "  {} providers from env, {} and {}",
        "connector".dimmed(),
        "~/.kdo/credentials.toml".yellow(),
        ".kdo/plugins/".yellow()
    );
    Ok(built)
}

fn reconciler(store: Store) -> miette::Result<Reconciler> {
    let workspace = workspace_root()?;
    let connector = connector(&workspace)?;
    Ok(Reconciler::new(store, connector).with_workspace(workspace))
}

async fn open_store() -> miette::Result<Store> {
    let path = db_path()?;
    Store::open(&path).await.map_err(|e| miette::miette!("{e}"))
}

// --- apply ----------------------------------------------------------------

pub fn cmd_apply(file: &Path) -> miette::Result<()> {
    let yaml = std::fs::read_to_string(file).into_diagnostic()?;
    let doc = parse_spec(&yaml).map_err(|e| miette::miette!("{e}"))?;
    let parsed_json = serde_json::to_string(&doc).into_diagnostic()?;

    let rt = runtime()?;
    let spec = rt.block_on(async {
        let store = open_store().await?;
        store
            .create_spec(
                None,
                &doc.metadata.name,
                &doc.kind,
                doc.metadata.project.as_deref(),
                &yaml,
                &parsed_json,
            )
            .await
            .map_err(|e| miette::miette!("{e}"))
    })?;

    eprintln!(
        "{} created spec {} ({})",
        "kdo apply".cyan().bold(),
        spec.name.yellow().bold(),
        spec.id.dimmed()
    );
    eprintln!(
        "  {} run {} to drive the reconciler",
        "next".dimmed(),
        "kdo factory daemon".yellow().bold()
    );
    Ok(())
}

// --- status ---------------------------------------------------------------

#[derive(Tabled)]
struct SpecRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Kind")]
    kind: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Spec ID")]
    id: String,
}

#[derive(Tabled)]
struct RunRow {
    #[tabled(rename = "Run ID")]
    id: String,
    #[tabled(rename = "Spec")]
    spec: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Cost $")]
    cost: String,
    #[tabled(rename = "Tokens")]
    tokens: String,
}

#[derive(Tabled)]
struct TaskRow {
    #[tabled(rename = "Role")]
    role: String,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Model")]
    model: String,
}

pub fn cmd_status(spec_id: Option<&str>, run_id: Option<&str>) -> miette::Result<()> {
    let rt = runtime()?;
    rt.block_on(async {
        let store = open_store().await?;

        if let Some(rid) = run_id {
            // Run detail.
            let run = store
                .get_run(rid)
                .await
                .map_err(|e| miette::miette!("{e}"))?;
            eprintln!(
                "{} run {} ({})",
                "kdo factory".cyan().bold(),
                run.id.yellow(),
                colorize_status(run.status.as_str())
            );
            eprintln!(
                "  cost ${:.4} • tokens {}/{} • spec {}",
                run.cost_usd, run.tokens_in, run.tokens_out, run.spec_id
            );
            let tasks = store
                .list_tasks(&run.id)
                .await
                .map_err(|e| miette::miette!("{e}"))?;
            let rows: Vec<TaskRow> = tasks
                .into_iter()
                .map(|t| TaskRow {
                    role: t.role.as_str().into(),
                    title: t.title,
                    status: t.status.as_str().into(),
                    model: t.model,
                })
                .collect();
            println!("{}", Table::new(rows));
            return Ok::<_, miette::Report>(());
        }

        let specs = store
            .list_specs(None)
            .await
            .map_err(|e| miette::miette!("{e}"))?;
        let specs: Vec<_> = match spec_id {
            Some(s) => specs.into_iter().filter(|sp| sp.id == s).collect(),
            None => specs,
        };

        if specs.is_empty() {
            eprintln!(
                "{} no specs yet — run {} -f spec.yaml",
                "kdo factory".cyan().bold(),
                "kdo apply".yellow()
            );
            return Ok(());
        }

        let spec_rows: Vec<SpecRow> = specs
            .iter()
            .map(|s| SpecRow {
                name: s.name.clone(),
                kind: s.kind.clone(),
                status: s.status.as_str().into(),
                id: s.id.clone(),
            })
            .collect();
        eprintln!("{} specs", "kdo factory".cyan().bold());
        println!("{}", Table::new(spec_rows));

        let runs = store
            .list_runs(None)
            .await
            .map_err(|e| miette::miette!("{e}"))?;
        if !runs.is_empty() {
            let run_rows: Vec<RunRow> = runs
                .iter()
                .map(|r| RunRow {
                    id: r.id.chars().take(8).collect(),
                    spec: r.spec_id.chars().take(8).collect(),
                    status: r.status.as_str().into(),
                    cost: format!("{:.4}", r.cost_usd),
                    tokens: format!("{}/{}", r.tokens_in, r.tokens_out),
                })
                .collect();
            eprintln!();
            eprintln!("{} runs", "kdo factory".cyan().bold());
            println!("{}", Table::new(run_rows));
        }
        Ok(())
    })
}

fn colorize_status(s: &str) -> String {
    match s {
        "succeeded" => s.green().bold().to_string(),
        "failed" => s.red().bold().to_string(),
        "running" | "awaiting_merge" => s.yellow().bold().to_string(),
        "pending" => s.dimmed().to_string(),
        _ => s.into(),
    }
}

// --- tick / daemon --------------------------------------------------------

pub fn cmd_tick() -> miette::Result<()> {
    let rt = runtime()?;
    rt.block_on(async {
        let store = open_store().await?;
        let recon = reconciler(store)?;
        recon.tick().await.map_err(|e| miette::miette!("{e}"))?;
        eprintln!("  {} tick complete", "ok".green());
        Ok(())
    })
}

pub fn cmd_merge(run_id: &str) -> miette::Result<()> {
    let rt = runtime()?;
    rt.block_on(async {
        let store = open_store().await?;
        let recon = reconciler(store)?;
        recon
            .merge_finished_run(run_id)
            .await
            .map_err(|err| miette::miette!("{err}"))?;
        eprintln!(
            "{} merge attempted for {}",
            "kdo factory".cyan().bold(),
            run_id.yellow()
        );
        Ok(())
    })
}

pub fn cmd_keys_status() -> miette::Result<()> {
    let workspace = workspace_root()?;
    let plugins = PluginSet::load(&workspace).map_err(|err| miette::miette!("{err}"))?;
    let keys = KeyStore::load_default().map_err(|err| miette::miette!("{err}"))?;
    eprintln!("{}", "kdo keys".cyan().bold());
    match keys.path() {
        Some(path) if path.exists() => eprintln!("  {} {}", "file".dimmed(), path.display()),
        Some(path) => eprintln!(
            "  {} {} {}",
            "file".dimmed(),
            path.display(),
            "(absent)".yellow()
        ),
        None => eprintln!("  {} no credentials path", "file".dimmed()),
    }
    if let Some(warning) = keys.perm_warning() {
        eprintln!("  {} {warning}", "warn".yellow());
    }
    for status in keys.status(plugins.providers()) {
        let mark = if status.present {
            "set".green().to_string()
        } else {
            "missing".yellow().to_string()
        };
        eprintln!("  {} {mark}", status.provider);
    }
    eprintln!(
        "  {} secrets stay in the environment or the credentials file",
        "note".dimmed()
    );
    Ok(())
}

/// Factory checks for `kdo doctor`. Returns `(issues, warnings)`.
pub fn doctor(root: &Path) -> (usize, usize) {
    let mut issues = 0;
    let mut warnings = 0;
    match PluginSet::load(root) {
        Ok(plugins) => {
            eprintln!("  {} factory plugins ({})", "ok".green(), plugins.len());
        }
        Err(err) => {
            eprintln!("  {} factory plugins: {err}", "err".red());
            issues += 1;
        }
    }
    match KeyStore::load_default() {
        Ok(keys) => {
            if let Some(warning) = keys.perm_warning() {
                eprintln!("  {} {warning}", "warn".yellow());
                warnings += 1;
            } else if keys.path().is_some_and(|path| path.exists()) {
                eprintln!("  {} credentials file", "ok".green());
            } else {
                eprintln!(
                    "  {} no credentials file (environment keys still work)",
                    "info".blue()
                );
            }
        }
        Err(err) => {
            eprintln!("  {} credentials: {err}", "err".red());
            issues += 1;
        }
    }
    match std::process::Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => {
            eprintln!("  {} git for factory worktrees", "ok".green());
        }
        _ => {
            eprintln!("  {} git is required for factory worktrees", "err".red());
            issues += 1;
        }
    }
    (issues, warnings)
}

pub fn cmd_daemon(poll_ms: u64) -> miette::Result<()> {
    let rt = runtime()?;
    rt.block_on(async move {
        let store = open_store().await?;
        let recon = reconciler(store)?.with_poll_interval(Duration::from_millis(poll_ms));
        eprintln!(
            "{} reconciling — Ctrl-C to stop",
            "kdo factory daemon".cyan().bold()
        );
        let (tx, rx) = watch::channel(false);
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = tx.send(true);
        });
        recon.run(rx).await.map_err(|e| miette::miette!("{e}"))?;
        eprintln!("  {} stopped", "ok".green());
        Ok(())
    })
}

// --- logs -----------------------------------------------------------------

pub fn cmd_logs(run_id: &str, follow: bool) -> miette::Result<()> {
    let rt = runtime()?;
    rt.block_on(async move {
        let store = open_store().await?;
        let mut last_id: i64 = 0;
        loop {
            let events = store
                .list_events(run_id)
                .await
                .map_err(|e| miette::miette!("{e}"))?;
            let cutoff = last_id;
            for ev in events.iter().filter(|e| e.id > cutoff) {
                last_id = ev.id;
                let payload = ev.payload.as_deref().unwrap_or("");
                let kind = match ev.kind.as_str() {
                    k if k.ends_with(".succeeded") => k.green().bold().to_string(),
                    k if k.ends_with(".failed") => k.red().bold().to_string(),
                    k if k.ends_with(".started") => k.yellow().to_string(),
                    k => k.dimmed().to_string(),
                };
                println!(
                    "{} {} {}",
                    ev.created_at.format("%H:%M:%S").to_string().dimmed(),
                    kind,
                    truncate(payload, 120).dimmed()
                );
            }

            if !follow {
                break;
            }
            // Stop following once the run terminates.
            if let Ok(run) = store.get_run(run_id).await {
                if matches!(
                    run.status,
                    RunStatus::Succeeded | RunStatus::Failed | RunStatus::Cancelled
                ) {
                    // Emit any final tasks.
                    let tasks = store
                        .list_tasks(run_id)
                        .await
                        .map_err(|e| miette::miette!("{e}"))?;
                    if tasks.iter().all(|t| {
                        matches!(
                            t.status,
                            TaskStatus::Succeeded | TaskStatus::Failed | TaskStatus::Skipped
                        )
                    }) {
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    })
}

fn truncate(s: &str, n: usize) -> String {
    let one_line: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    if one_line.chars().count() > n {
        format!("{}…", one_line.chars().take(n).collect::<String>())
    } else {
        one_line
    }
}
