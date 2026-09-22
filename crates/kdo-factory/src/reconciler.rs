//! The reconciliation loop.
//!
//! `tick()` runs one pass:
//!   1. promote pending specs into a run, a git worktree, and four tasks
//!   2. advance the next pending task (or retry a blocked merge)
//!   3. after review passes, merge onto the current branch when it is clean
//!
//! `run()` loops `tick()` until cancelled.

use crate::brief::workspace_brief;
use crate::connector::{CompletionRequest, Connector};
use crate::error::{FactoryError, FactoryResult};
use crate::keys::{redact, KeyStore};
use crate::merge;
use crate::plugin::{PluginSet, ToolName};
use crate::resources::{RunStatus, Spec, SpecStatus, Task, TaskRole, TaskStatus};
use crate::spec::{parse_spec, AgentAssignments, Budget, SpecDocument};
use crate::store::Store;
use crate::tools::{self, ToolContext};
use crate::turn::{parse_model_turn, require_verdict, ModelTurn};
use crate::worktree;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{debug, info, warn};

/// Optional callback invoked on every event append.
pub type EventSink = Arc<dyn Fn(&str, Option<&str>, &str, Option<&str>) + Send + Sync>;

#[derive(Clone)]
pub struct Reconciler {
    store: Store,
    connector: Arc<dyn Connector>,
    sink: Option<EventSink>,
    poll_interval: Duration,
    workspace: PathBuf,
}

struct TaskOutcome {
    status: TaskStatus,
    output: String,
    tokens_in: i64,
    tokens_out: i64,
    cost_usd: f64,
}

impl TaskOutcome {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Failed,
            output: message.into(),
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
        }
    }

    fn with_usage(mut self, tokens_in: i64, tokens_out: i64, cost_usd: f64) -> Self {
        self.tokens_in = tokens_in;
        self.tokens_out = tokens_out;
        self.cost_usd = cost_usd;
        self
    }
}

impl Reconciler {
    pub fn new(store: Store, connector: Arc<dyn Connector>) -> Self {
        Self {
            store,
            connector,
            sink: None,
            poll_interval: Duration::from_millis(500),
            workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_event_sink(mut self, sink: EventSink) -> Self {
        self.sink = Some(sink);
        self
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Workspace whose graph, `kdo.toml`, and git checkout this loop uses.
    #[must_use]
    pub fn with_workspace(mut self, workspace: PathBuf) -> Self {
        self.workspace = workspace;
        self
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn spawn(self) -> ReconcilerHandle {
        let (tx, rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            let _ = self.run(rx).await;
        });
        ReconcilerHandle {
            cancel: tx,
            task: Some(task),
        }
    }

    pub async fn run(&self, mut cancel: watch::Receiver<bool>) -> FactoryResult<()> {
        loop {
            if *cancel.borrow() {
                break;
            }
            if let Err(err) = self.tick().await {
                warn!(error = %err, "reconciler tick failed");
            }
            tokio::select! {
                _ = tokio::time::sleep(self.poll_interval) => {}
                _ = cancel.changed() => {
                    if *cancel.borrow() {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// One reconciliation pass. Idempotent.
    pub async fn tick(&self) -> FactoryResult<()> {
        for spec in self.store.pending_specs().await? {
            self.promote_spec(spec).await?;
        }
        for run in self.store.pending_runs().await? {
            self.advance_run(&run.id).await?;
        }
        Ok(())
    }

    /// Retry the merge for a run that is waiting on a clean checkout.
    pub async fn merge_finished_run(&self, run_id: &str) -> FactoryResult<()> {
        self.finish_or_merge(run_id).await
    }

    async fn promote_spec(&self, spec: Spec) -> FactoryResult<()> {
        info!(spec = %spec.name, "promoting spec to run");
        self.store
            .set_spec_status(&spec.id, SpecStatus::Planning)
            .await?;
        let parsed = match parse_spec(&spec.body) {
            Ok(parsed) => parsed,
            Err(err) => {
                self.store
                    .set_spec_status(&spec.id, SpecStatus::Failed)
                    .await?;
                warn!(error = %err, "spec parse failed");
                return Ok(());
            }
        };
        let plugins = match PluginSet::load(&self.workspace) {
            Ok(plugins) => plugins,
            Err(err) => {
                self.store
                    .set_spec_status(&spec.id, SpecStatus::Failed)
                    .await?;
                warn!(error = %err, "plugin load failed");
                return Ok(());
            }
        };
        let run = self
            .store
            .create_run(&spec.id, spec.workspace_id.as_deref())
            .await?;
        self.emit(&run.id, None, "run.created", Some(&spec.name))
            .await?;

        match worktree::create(&self.workspace, &run.id) {
            Ok(worktree) => {
                self.store
                    .set_run_layout(
                        &run.id,
                        &worktree.branch,
                        &worktree.path.to_string_lossy(),
                        &worktree.base_sha,
                    )
                    .await?;
            }
            Err(err) => {
                self.fail_run(&run.id, &spec.id, &err.to_string()).await?;
                return Ok(());
            }
        }

        for role in [
            TaskRole::Planner,
            TaskRole::Implementer,
            TaskRole::Tester,
            TaskRole::Reviewer,
        ] {
            let Some(agent) = plugins.agent_for_role(role.as_str()) else {
                self.fail_run(
                    &run.id,
                    &spec.id,
                    &format!("no agent for {}", role.as_str()),
                )
                .await?;
                return Ok(());
            };
            let model = if agent.command_only {
                "command".to_string()
            } else {
                spec_model(role, &parsed.spec.agents)
                    .filter(|model| !model.is_empty())
                    .or_else(|| {
                        if agent.model.is_empty() {
                            None
                        } else {
                            Some(agent.model.clone())
                        }
                    })
                    .unwrap_or_else(|| default_model(role).to_string())
            };
            self.store
                .create_task(
                    &run.id,
                    role_title(role),
                    Some(&parsed.spec.description),
                    role,
                    &model,
                )
                .await?;
        }
        self.store
            .set_spec_status(&spec.id, SpecStatus::Running)
            .await?;
        Ok(())
    }

    async fn advance_run(&self, run_id: &str) -> FactoryResult<()> {
        let run = self.store.get_run(run_id).await?;
        if run.status == RunStatus::Pending {
            self.store.start_run(run_id).await?;
            self.emit(run_id, None, "run.started", None).await?;
        }
        if run.status == RunStatus::AwaitingMerge {
            return self.finish_or_merge(run_id).await;
        }

        let tasks = self.store.list_tasks(run_id).await?;
        if tasks.iter().any(|task| task.status == TaskStatus::Failed) {
            return self
                .fail_run(run_id, &run.spec_id, "one or more tasks failed")
                .await;
        }
        if let Some(task) = tasks
            .into_iter()
            .find(|task| task.status == TaskStatus::Pending)
        {
            return self.execute_task(run_id, &task).await;
        }
        self.finish_or_merge(run_id).await
    }

    async fn execute_task(&self, run_id: &str, task: &Task) -> FactoryResult<()> {
        debug!(task = %task.title, role = task.role.as_str(), "executing task");
        self.store.start_task(&task.id).await?;
        self.emit(
            run_id,
            Some(&task.id),
            "task.started",
            Some(task.role.as_str()),
        )
        .await?;

        let run = self.store.get_run(run_id).await?;
        let spec_row = self.store.get_spec(&run.spec_id).await?;
        let parsed = parse_spec(&spec_row.body)?;
        let worktree = run.worktree_path.clone().unwrap_or_default();
        let worktree_path = PathBuf::from(&worktree);

        let outcome = if task.model == "command" {
            self.command_task(&parsed, &worktree_path)
        } else {
            self.model_task(
                run_id,
                task,
                &parsed,
                &worktree_path,
                run.base_sha.as_deref(),
            )
            .await
        };

        let outcome = match outcome {
            Ok(mut outcome) => {
                if matches!(task.role, TaskRole::Implementer | TaskRole::Tester)
                    && outcome.status == TaskStatus::Succeeded
                    && !worktree.is_empty()
                {
                    let message = format!("kdo: {} ({})", spec_row.name, task.role.as_str());
                    if let Err(err) = worktree::commit_all(Path::new(&worktree), &message) {
                        outcome.status = TaskStatus::Failed;
                        outcome.output = err.to_string();
                    }
                }
                outcome
            }
            Err(err) => TaskOutcome::failed(redact_known(&err.to_string())),
        };

        self.store
            .finish_task(
                &task.id,
                outcome.status,
                Some(&outcome.output),
                if outcome.status == TaskStatus::Failed {
                    Some(outcome.output.as_str())
                } else {
                    None
                },
                outcome.tokens_in,
                outcome.tokens_out,
                outcome.cost_usd,
            )
            .await?;
        self.store
            .add_run_usage(
                run_id,
                outcome.tokens_in,
                outcome.tokens_out,
                outcome.cost_usd,
            )
            .await?;
        let kind = match outcome.status {
            TaskStatus::Failed => "task.failed",
            TaskStatus::Skipped => "task.skipped",
            _ => "task.succeeded",
        };
        self.emit(
            run_id,
            Some(&task.id),
            kind,
            Some(&clip(&outcome.output, 2_000)),
        )
        .await?;
        Ok(())
    }

    fn command_task(&self, spec: &SpecDocument, worktree: &Path) -> FactoryResult<TaskOutcome> {
        match tools::run_builtin_test(&self.workspace, worktree, spec.metadata.project.as_deref()) {
            Ok(None) => Ok(TaskOutcome {
                status: TaskStatus::Skipped,
                output: "no test task; skipped".into(),
                tokens_in: 0,
                tokens_out: 0,
                cost_usd: 0.0,
            }),
            Ok(Some(output)) => Ok(TaskOutcome {
                status: TaskStatus::Succeeded,
                output,
                tokens_in: 0,
                tokens_out: 0,
                cost_usd: 0.0,
            }),
            Err(err) => Ok(TaskOutcome::failed(err.to_string())),
        }
    }

    async fn model_task(
        &self,
        run_id: &str,
        task: &Task,
        spec: &SpecDocument,
        worktree: &Path,
        base_sha: Option<&str>,
    ) -> FactoryResult<TaskOutcome> {
        let plugins = PluginSet::load(&self.workspace)?;
        let Some(agent) = plugins.agent_for_role(task.role.as_str()).cloned() else {
            return Ok(TaskOutcome::failed(format!(
                "no agent for {}",
                task.role.as_str()
            )));
        };
        let secrets = collect_secrets(&plugins);
        let budget_tokens = usize::try_from(spec.spec.budget.max_tokens).unwrap_or(4096);
        let brief = workspace_brief(
            &self.workspace,
            spec.metadata.project.as_deref(),
            budget_tokens,
        );
        let tasks = self.store.list_tasks(run_id).await?;
        let prior = prior_text(&tasks);
        let run = self.store.get_run(run_id).await?;
        let mut tokens_in = u64::try_from(run.tokens_in).unwrap_or(0);
        let mut tokens_out = u64::try_from(run.tokens_out).unwrap_or(0);
        let mut cost = run.cost_usd;
        let start_in = tokens_in;
        let start_out = tokens_out;
        let start_cost = cost;
        let mut transcript = String::new();
        let mut iterations = 0u32;

        loop {
            if let Some(reason) =
                over_budget(iterations, tokens_in, tokens_out, cost, &spec.spec.budget)
            {
                return Ok(TaskOutcome::failed(reason).with_usage(
                    delta(tokens_in, start_in),
                    delta(tokens_out, start_out),
                    cost - start_cost,
                ));
            }
            let user = user_prompt(spec, &brief, &prior, &transcript);
            let completion = match self
                .connector
                .complete(CompletionRequest {
                    model: &task.model,
                    system: &agent.system,
                    user: &user,
                    max_tokens: 4096,
                })
                .await
            {
                Ok(completion) => completion,
                Err(err) => {
                    return Ok(
                        TaskOutcome::failed(redact(&err.to_string(), &secrets)).with_usage(
                            delta(tokens_in, start_in),
                            delta(tokens_out, start_out),
                            cost - start_cost,
                        ),
                    )
                }
            };
            iterations += 1;
            tokens_in += completion.tokens_in;
            tokens_out += completion.tokens_out;
            cost += completion.cost_usd;
            let text = redact(&completion.text, &secrets);
            let usage = (
                delta(tokens_in, start_in),
                delta(tokens_out, start_out),
                cost - start_cost,
            );

            if task.role == TaskRole::Reviewer {
                if let Ok((pass, notes)) = require_verdict(&text) {
                    return Ok(TaskOutcome {
                        status: if pass {
                            TaskStatus::Succeeded
                        } else {
                            TaskStatus::Failed
                        },
                        output: notes,
                        tokens_in: usage.0,
                        tokens_out: usage.1,
                        cost_usd: usage.2,
                    });
                }
            }

            let turn = match parse_model_turn(&text) {
                Ok(turn) => turn,
                Err(err) => {
                    return Ok(
                        TaskOutcome::failed(err.to_string()).with_usage(usage.0, usage.1, usage.2)
                    )
                }
            };
            match turn {
                ModelTurn::Done { summary } if task.role != TaskRole::Reviewer => {
                    return Ok(TaskOutcome {
                        status: TaskStatus::Succeeded,
                        output: summary,
                        tokens_in: usage.0,
                        tokens_out: usage.1,
                        cost_usd: usage.2,
                    });
                }
                ModelTurn::Verdict { notes, .. } if task.role != TaskRole::Reviewer => {
                    return Ok(TaskOutcome {
                        status: TaskStatus::Succeeded,
                        output: notes,
                        tokens_in: usage.0,
                        tokens_out: usage.1,
                        cost_usd: usage.2,
                    });
                }
                ModelTurn::Tool { name, input } => {
                    let Some(tool) = ToolName::parse(&name) else {
                        return Ok(TaskOutcome::failed(format!("unknown tool `{name}`"))
                            .with_usage(usage.0, usage.1, usage.2));
                    };
                    let ctx = ToolContext {
                        workspace: &self.workspace,
                        worktree,
                        project: spec.metadata.project.as_deref(),
                        brief: &brief,
                        base_sha,
                        allowed: &agent.tools,
                    };
                    match tools::execute(&ctx, tool, &input) {
                        Ok(result) => {
                            let result = redact(&result, &secrets);
                            transcript.push_str(&format!("tool {name}: {result}\n"));
                            if transcript.len() > 8_000 {
                                transcript = clip(&transcript, 8_000);
                            }
                        }
                        Err(err) => {
                            return Ok(TaskOutcome::failed(redact(&err.to_string(), &secrets))
                                .with_usage(usage.0, usage.1, usage.2))
                        }
                    }
                }
                _ => {
                    return Ok(TaskOutcome::failed(format!(
                        "invalid review verdict: {}",
                        clip(&text, 200)
                    ))
                    .with_usage(usage.0, usage.1, usage.2))
                }
            }
        }
    }

    async fn finish_or_merge(&self, run_id: &str) -> FactoryResult<()> {
        let run = self.store.get_run(run_id).await?;
        let Some(branch) = run.branch.clone() else {
            self.store
                .finish_run(run_id, RunStatus::Succeeded, None)
                .await?;
            self.store
                .set_spec_status(&run.spec_id, SpecStatus::Succeeded)
                .await?;
            self.emit(run_id, None, "run.succeeded", None).await?;
            return Ok(());
        };
        match merge::try_merge(&self.workspace, &branch) {
            Ok(()) => {
                if let Some(path) = run.worktree_path.as_deref() {
                    if let Err(err) = worktree::remove(&self.workspace, Path::new(path)) {
                        warn!(error = %err, "worktree remove failed");
                    }
                }
                if let Err(err) = worktree::delete_branch(&self.workspace, &branch) {
                    warn!(error = %err, "branch delete failed");
                }
                self.store
                    .finish_run(run_id, RunStatus::Succeeded, None)
                    .await?;
                self.store
                    .set_spec_status(&run.spec_id, SpecStatus::Succeeded)
                    .await?;
                self.emit(run_id, None, "run.succeeded", Some("merged"))
                    .await?;
            }
            Err(FactoryError::DirtyWorktree) => {
                self.store
                    .set_run_status(
                        run_id,
                        RunStatus::AwaitingMerge,
                        Some("working tree is dirty"),
                    )
                    .await?;
                self.emit(run_id, None, "merge.blocked", Some("working tree is dirty"))
                    .await?;
            }
            Err(err) => {
                self.fail_run(run_id, &run.spec_id, &err.to_string())
                    .await?;
            }
        }
        Ok(())
    }

    async fn fail_run(&self, run_id: &str, spec_id: &str, message: &str) -> FactoryResult<()> {
        let message = redact_known(message);
        self.store
            .finish_run(run_id, RunStatus::Failed, Some(&message))
            .await?;
        self.store
            .set_spec_status(spec_id, SpecStatus::Failed)
            .await?;
        self.emit(run_id, None, "run.failed", Some(&message))
            .await?;
        Ok(())
    }

    async fn emit(
        &self,
        run_id: &str,
        task_id: Option<&str>,
        kind: &str,
        payload: Option<&str>,
    ) -> FactoryResult<()> {
        self.store
            .append_event(run_id, task_id, kind, payload)
            .await?;
        if let Some(sink) = &self.sink {
            sink(run_id, task_id, kind, payload);
        }
        Ok(())
    }
}

pub struct ReconcilerHandle {
    cancel: watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl ReconcilerHandle {
    pub async fn stop(mut self) {
        let _ = self.cancel.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for ReconcilerHandle {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}

fn role_title(role: TaskRole) -> &'static str {
    match role {
        TaskRole::Planner => "Plan",
        TaskRole::Implementer => "Implement",
        TaskRole::Tester => "Test",
        TaskRole::Reviewer => "Review",
    }
}

fn default_model(role: TaskRole) -> &'static str {
    match role {
        TaskRole::Planner => "claude-opus-4-7",
        TaskRole::Implementer | TaskRole::Reviewer => "claude-sonnet-4-6",
        TaskRole::Tester => "claude-haiku-4-5",
    }
}

fn spec_model(role: TaskRole, agents: &AgentAssignments) -> Option<String> {
    match role {
        TaskRole::Planner => agents.planner.clone(),
        TaskRole::Implementer => agents.implementer.clone(),
        TaskRole::Reviewer => agents.reviewer.clone(),
        TaskRole::Tester => agents.tester.clone(),
    }
}

fn over_budget(
    iterations: u32,
    tokens_in: u64,
    tokens_out: u64,
    cost: f64,
    budget: &Budget,
) -> Option<String> {
    if iterations >= budget.max_iterations {
        return Some(format!("max_iterations {}", budget.max_iterations));
    }
    if tokens_in.saturating_add(tokens_out) >= budget.max_tokens {
        return Some(format!("max_tokens {}", budget.max_tokens));
    }
    if cost >= budget.max_cost_usd {
        return Some(format!("max_cost_usd {}", budget.max_cost_usd));
    }
    None
}

fn user_prompt(spec: &SpecDocument, brief: &str, prior: &str, transcript: &str) -> String {
    let acceptance = if spec.spec.acceptance.is_empty() {
        "(none listed)".to_string()
    } else {
        spec.spec
            .acceptance
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let project = spec.metadata.project.as_deref().unwrap_or("(workspace)");
    format!(
        "Specification: {name}\nProject: {project}\n\n{description}\n\nAcceptance:\n{acceptance}\n\nWorkspace graph and context:\n{brief}\n\nEarlier agents:\n{prior}\n\nTool results:\n{transcript}\n\nReply with one JSON object only.\n",
        name = spec.metadata.name,
        description = spec.spec.description.trim(),
    )
}

fn prior_text(tasks: &[Task]) -> String {
    let mut out = String::new();
    for task in tasks {
        if matches!(
            task.status,
            TaskStatus::Succeeded | TaskStatus::Skipped | TaskStatus::Failed
        ) {
            out.push_str(&format!(
                "## {} ({})\n{}\n\n",
                task.role.as_str(),
                task.status.as_str(),
                task.output.as_deref().unwrap_or("")
            ));
        }
    }
    clip(&out, 8_000)
}

fn collect_secrets(plugins: &PluginSet) -> Vec<String> {
    let mut secrets = KeyStore::load_default()
        .map(|store| store.secret_values())
        .unwrap_or_default();
    for provider in plugins.providers() {
        if let Ok(value) = std::env::var(&provider.api_key_env) {
            if value.len() >= 8 {
                secrets.push(value);
            }
        }
    }
    secrets
}

fn redact_known(text: &str) -> String {
    let Ok(store) = KeyStore::load_default() else {
        return text.to_string();
    };
    redact(text, &store.secret_values())
}

fn delta(now: u64, start: u64) -> i64 {
    i64::try_from(now.saturating_sub(start)).unwrap_or(i64::MAX)
}

fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::{Completion, CompletionRequest, MockConnector};
    use async_trait::async_trait;
    use std::sync::Mutex;

    fn git_init(dir: &Path) {
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        };
        run(&["init"]);
        run(&["config", "user.email", "kdo@example.com"]);
        run(&["config", "user.name", "kdo"]);
        std::fs::write(dir.join("README.md"), "hi\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-m", "init"]);
    }

    async fn fixture(connector: Arc<dyn Connector>) -> (tempfile::TempDir, Reconciler, Store) {
        let tmp = tempfile::tempdir().unwrap();
        git_init(tmp.path());
        let store = Store::open(&tmp.path().join(".kdo").join("factory.db"))
            .await
            .unwrap();
        let recon =
            Reconciler::new(store.clone(), connector).with_workspace(tmp.path().to_path_buf());
        (tmp, recon, store)
    }

    async fn apply(store: &Store, yaml: &str) {
        let spec = parse_spec(yaml).unwrap();
        let parsed_json = serde_json::to_string(&spec).unwrap();
        store
            .create_spec(
                None,
                &spec.metadata.name,
                &spec.kind,
                spec.metadata.project.as_deref(),
                yaml,
                &parsed_json,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn tick_runs_a_full_spec_to_completion() {
        let (_tmp, recon, store) = fixture(Arc::new(MockConnector)).await;
        apply(
            &store,
            r#"
kind: Feature
metadata:
  name: hello
spec:
  description: print hello
"#,
        )
        .await;

        for _ in 0..8 {
            recon.tick().await.unwrap();
        }

        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Succeeded);
        let tasks = store.list_tasks(&runs[0].id).await.unwrap();
        assert_eq!(tasks.len(), 4);
        assert_eq!(tasks[0].role, TaskRole::Planner);
        assert_eq!(tasks[2].role, TaskRole::Tester);
        assert_eq!(tasks[2].status, TaskStatus::Skipped);
        assert_eq!(tasks[3].role, TaskRole::Reviewer);
        assert_eq!(tasks[3].status, TaskStatus::Succeeded);
        let events = store.list_events(&runs[0].id).await.unwrap();
        assert!(events.iter().any(|event| event.kind == "run.succeeded"));
    }

    struct Scripted {
        left: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Connector for Scripted {
        async fn complete(&self, _req: CompletionRequest<'_>) -> FactoryResult<Completion> {
            let mut left = self.left.lock().unwrap();
            let text = left
                .pop()
                .ok_or_else(|| FactoryError::Connector("no scripted response left".into()))?;
            Ok(Completion {
                text,
                tokens_in: 4,
                tokens_out: 4,
                cost_usd: 0.0,
            })
        }
    }

    #[tokio::test]
    async fn implementer_write_merges_onto_a_clean_tree() {
        let scripted = Scripted {
            // `pop` takes from the end, so the last item runs first.
            left: Mutex::new(vec![
                "{\"pass\":true,\"notes\":\"file exists\"}".into(),
                "{\"done\":true,\"summary\":\"wrote hello.txt\"}".into(),
                "{\"tool\":\"write\",\"input\":{\"path\":\"hello.txt\",\"contents\":\"hello from kdo\\n\"}}".into(),
                "{\"done\":true,\"summary\":\"add hello.txt\"}".into(),
            ]),
        };
        let (tmp, recon, store) = fixture(Arc::new(scripted)).await;
        apply(
            &store,
            r#"
kind: Feature
metadata:
  name: hello-file
spec:
  description: write hello.txt
  acceptance:
    - hello.txt contains hello from kdo
"#,
        )
        .await;
        for _ in 0..8 {
            recon.tick().await.unwrap();
        }
        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs[0].status, RunStatus::Succeeded, "{:?}", runs[0].error);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("hello.txt")).unwrap(),
            "hello from kdo\n"
        );
    }

    #[tokio::test]
    async fn dirty_tree_waits_then_merges() {
        let (tmp, recon, store) = fixture(Arc::new(MockConnector)).await;
        apply(
            &store,
            r#"
kind: Feature
metadata:
  name: hello
spec:
  description: print hello
"#,
        )
        .await;
        for _ in 0..4 {
            recon.tick().await.unwrap();
        }
        std::fs::write(tmp.path().join("DIRTY"), "x\n").unwrap();
        recon.tick().await.unwrap();
        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs[0].status, RunStatus::AwaitingMerge);

        std::fs::remove_file(tmp.path().join("DIRTY")).unwrap();
        recon.tick().await.unwrap();
        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs[0].status, RunStatus::Succeeded);
    }
}
