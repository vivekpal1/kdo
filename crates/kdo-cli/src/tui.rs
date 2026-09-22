//! `kdo tui` — a full-screen view of the factory.
//!
//! The screen is a projection of the store. Tests cover that projection,
//! not the terminal.

use kdo_factory::{KeyStore, PluginSet, Reconciler, Store};
use kdo_graph::WorkspaceGraph;
use miette::IntoDiagnostic;
use owo_colors::OwoColorize;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::io;
use std::path::Path;
use std::time::Duration;

const TIDE: Color = Color::Rgb(32, 140, 156);
const DEEP: Color = Color::Rgb(14, 18, 28);
const INK: Color = Color::Rgb(232, 236, 239);
const DIM: Color = Color::Rgb(140, 152, 160);

pub struct DashSnap {
    pub specs: Vec<String>,
    pub runs: Vec<(String, String)>,
    pub tasks: Vec<String>,
    pub events: Vec<String>,
    pub keys: String,
    pub graph: String,
    pub selected: usize,
}

pub fn key_line(pairs: &[(String, bool)]) -> String {
    if pairs.is_empty() {
        return "no providers".into();
    }
    pairs
        .iter()
        .map(|(name, present)| format!("{name} {}", if *present { "set" } else { "missing" }))
        .collect::<Vec<_>>()
        .join("  ")
}

pub fn clamp_selected(selected: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        selected.min(len - 1)
    }
}

pub fn graph_line(root: &Path) -> String {
    match WorkspaceGraph::discover(root) {
        Ok(graph) => {
            let names: Vec<&str> = graph
                .projects()
                .iter()
                .take(8)
                .map(|project| project.name.as_str())
                .collect();
            if names.is_empty() {
                "graph: no projects".into()
            } else {
                format!("graph: {} — {}", graph.projects().len(), names.join(", "))
            }
        }
        Err(err) => format!("graph unavailable: {err}"),
    }
}

struct RawGuard;

impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub fn cmd_tui() -> miette::Result<()> {
    let root = std::env::current_dir().into_diagnostic()?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;
    let db = root.join(".kdo/factory.db");
    let store = rt
        .block_on(Store::open(&db))
        .map_err(|err| miette::miette!("{err}"))?;

    enable_raw_mode().into_diagnostic()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).into_diagnostic()?;
    let _guard = RawGuard;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).into_diagnostic()?;
    let mut selected = 0usize;

    loop {
        let snap = rt
            .block_on(load_snap(&store, &root, selected))
            .map_err(|err| miette::miette!("{err}"))?;
        selected = snap.selected;
        terminal
            .draw(|frame| draw(frame, &snap))
            .into_diagnostic()?;
        if !event::poll(Duration::from_millis(200)).into_diagnostic()? {
            continue;
        }
        let Event::Key(key) = event::read().into_diagnostic()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Char('j') | KeyCode::Down => selected = selected.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Char('m') => {
                if let Some((_, id)) = snap.runs.get(selected) {
                    let id = id.clone();
                    let workspace = root.clone();
                    let merged = store.clone();
                    rt.block_on(async move {
                        let connector = kdo_factory::build_connector(&workspace)
                            .unwrap_or_else(|_| std::sync::Arc::new(kdo_factory::MockConnector));
                        let recon = Reconciler::new(merged, connector).with_workspace(workspace);
                        let _ = recon.merge_finished_run(&id).await;
                    });
                }
            }
            _ => {}
        }
    }
    // RawGuard disables the terminal on drop.
    drop(terminal);
    eprintln!("{}", "kdo tui".cyan().bold());
    Ok(())
}

async fn load_snap(
    store: &Store,
    root: &Path,
    selected: usize,
) -> kdo_factory::FactoryResult<DashSnap> {
    let specs = store
        .list_specs(None)
        .await?
        .into_iter()
        .take(12)
        .map(|spec| format!("{}  {}", spec.status.as_str(), spec.name))
        .collect();
    let runs: Vec<(String, String)> = store
        .list_runs(None)
        .await?
        .into_iter()
        .take(12)
        .map(|run| {
            (
                format!(
                    "{}  ${:.4}  {}",
                    run.status.as_str(),
                    run.cost_usd,
                    run.id.chars().take(8).collect::<String>()
                ),
                run.id,
            )
        })
        .collect();
    let selected = clamp_selected(selected, runs.len());
    let (tasks, events) = if let Some((_, id)) = runs.get(selected) {
        let tasks = store
            .list_tasks(id)
            .await?
            .into_iter()
            .map(|task| {
                format!(
                    "{}  {}  {}",
                    task.role.as_str(),
                    task.status.as_str(),
                    task.model
                )
            })
            .collect();
        let events = store
            .list_events(id)
            .await?
            .into_iter()
            .rev()
            .take(10)
            .map(|event| {
                let payload = event.payload.unwrap_or_default();
                let payload = payload.chars().take(80).collect::<String>();
                format!("{}  {payload}", event.kind)
            })
            .collect();
        (tasks, events)
    } else {
        (Vec::new(), Vec::new())
    };
    let keys = match (PluginSet::load(root), KeyStore::load_default()) {
        (Ok(plugins), Ok(store)) => key_line(
            &store
                .status(plugins.providers())
                .into_iter()
                .map(|status| (status.provider, status.present))
                .collect::<Vec<_>>(),
        ),
        (Err(err), _) | (_, Err(err)) => format!("keys unavailable: {err}"),
    };
    Ok(DashSnap {
        specs,
        runs,
        tasks,
        events,
        keys,
        graph: graph_line(root),
        selected,
    })
}

fn draw(frame: &mut Frame, snap: &DashSnap) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Percentage(34),
            Constraint::Percentage(28),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " kdo ",
            Style::default()
                .fg(DEEP)
                .bg(TIDE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" factory ", Style::default().fg(INK)),
        Span::styled(&snap.graph, Style::default().fg(DIM)),
    ]))
    .block(block("workspace"));
    frame.render_widget(title, chunks[0]);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    frame.render_widget(lines_widget("specs", &snap.specs, None), columns[0]);
    let run_lines: Vec<String> = snap.runs.iter().map(|(label, _)| label.clone()).collect();
    frame.render_widget(
        lines_widget("runs", &run_lines, Some(snap.selected)),
        columns[1],
    );
    frame.render_widget(lines_widget("tasks", &snap.tasks, None), chunks[2]);
    frame.render_widget(lines_widget("events", &snap.events, None), chunks[3]);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(snap.keys.clone(), Style::default().fg(INK)),
        Span::styled("   j/k move   m merge   q quit", Style::default().fg(DIM)),
    ]))
    .block(block("keys"));
    frame.render_widget(footer, chunks[4]);
}

fn lines_widget<'a>(title: &'a str, rows: &'a [String], selected: Option<usize>) -> Paragraph<'a> {
    let lines: Vec<Line> = if rows.is_empty() {
        vec![Line::from(Span::styled(
            "none yet",
            Style::default().fg(DIM),
        ))]
    } else {
        rows.iter()
            .enumerate()
            .map(|(index, row)| {
                let style = if Some(index) == selected {
                    Style::default().fg(DEEP).bg(TIDE)
                } else {
                    Style::default().fg(INK)
                };
                Line::from(Span::styled(row.clone(), style))
            })
            .collect()
    };
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block(title))
}

fn block(title: &str) -> Block<'_> {
    Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TIDE))
        .style(Style::default().bg(DEEP).fg(INK))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_line_never_includes_a_secret() {
        let line = key_line(&[("anthropic".into(), true), ("openai".into(), false)]);
        assert_eq!(line, "anthropic set  openai missing");
        assert!(!line.contains("sk-"));
    }

    #[test]
    fn selection_clamps_to_runs() {
        assert_eq!(clamp_selected(4, 2), 1);
        assert_eq!(clamp_selected(0, 0), 0);
    }
}
