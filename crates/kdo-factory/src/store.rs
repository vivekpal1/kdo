//! SQLite-backed persistence for factory resources. Wraps a sqlx
//! `SqlitePool` and exposes typed CRUD that the reconciler operates on.

use crate::error::{FactoryError, FactoryResult};
use crate::resources::{Event, Run, RunStatus, Spec, SpecStatus, Task, TaskRole, TaskStatus};
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;

/// Thin wrapper around a sqlx pool. Cloneable — clones share the pool.
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Open a SQLite file (creates if missing) and run migrations.
    pub async fn open(path: &Path) -> FactoryResult<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let url = format!("sqlite://{}", path.display());
        let opts = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;
        crate::migrations::run(&pool).await?;
        Ok(Self { pool })
    }

    /// Wrap an existing pool (kdow backend already has one).
    pub async fn from_pool(pool: SqlitePool) -> FactoryResult<Self> {
        crate::migrations::run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // --- Spec --------------------------------------------------------------

    pub async fn create_spec(
        &self,
        workspace_id: Option<&str>,
        name: &str,
        kind: &str,
        project: Option<&str>,
        body: &str,
        parsed_json: &str,
    ) -> FactoryResult<Spec> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO factory_specs (id, workspace_id, name, kind, project, body, parsed_json, status)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'pending')",
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(name)
        .bind(kind)
        .bind(project)
        .bind(body)
        .bind(parsed_json)
        .execute(&self.pool)
        .await?;
        self.get_spec(&id).await
    }

    pub async fn get_spec(&self, id: &str) -> FactoryResult<Spec> {
        let row = sqlx::query("SELECT * FROM factory_specs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| FactoryError::NotFound(format!("spec {id}")))?;
        row_to_spec(&row)
    }

    pub async fn list_specs(&self, workspace_id: Option<&str>) -> FactoryResult<Vec<Spec>> {
        let rows = if let Some(ws) = workspace_id {
            sqlx::query(
                "SELECT * FROM factory_specs WHERE workspace_id = ? ORDER BY created_at DESC",
            )
            .bind(ws)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query("SELECT * FROM factory_specs ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await?
        };
        rows.iter().map(row_to_spec).collect()
    }

    pub async fn pending_specs(&self) -> FactoryResult<Vec<Spec>> {
        let rows = sqlx::query(
            "SELECT * FROM factory_specs WHERE status = 'pending' ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_spec).collect()
    }

    pub async fn set_spec_status(&self, id: &str, status: SpecStatus) -> FactoryResult<()> {
        sqlx::query("UPDATE factory_specs SET status = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Run ---------------------------------------------------------------

    pub async fn create_run(
        &self,
        spec_id: &str,
        workspace_id: Option<&str>,
    ) -> FactoryResult<Run> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO factory_runs (id, spec_id, workspace_id, status) VALUES (?, ?, ?, 'pending')",
        )
        .bind(&id)
        .bind(spec_id)
        .bind(workspace_id)
        .execute(&self.pool)
        .await?;
        self.get_run(&id).await
    }

    pub async fn get_run(&self, id: &str) -> FactoryResult<Run> {
        let row = sqlx::query("SELECT * FROM factory_runs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| FactoryError::NotFound(format!("run {id}")))?;
        row_to_run(&row)
    }

    pub async fn list_runs(&self, workspace_id: Option<&str>) -> FactoryResult<Vec<Run>> {
        let rows = if let Some(ws) = workspace_id {
            sqlx::query(
                "SELECT * FROM factory_runs WHERE workspace_id = ? ORDER BY created_at DESC",
            )
            .bind(ws)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query("SELECT * FROM factory_runs ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await?
        };
        rows.iter().map(row_to_run).collect()
    }

    pub async fn pending_runs(&self) -> FactoryResult<Vec<Run>> {
        let rows = sqlx::query(
            "SELECT * FROM factory_runs WHERE status IN ('pending', 'running', 'awaiting_merge') ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_run).collect()
    }

    pub async fn start_run(&self, id: &str) -> FactoryResult<()> {
        sqlx::query(
            "UPDATE factory_runs SET status = 'running', started_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn finish_run(
        &self,
        id: &str,
        status: RunStatus,
        error: Option<&str>,
    ) -> FactoryResult<()> {
        sqlx::query(
            "UPDATE factory_runs
             SET status = ?, completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), error = ?
             WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_run_status(
        &self,
        id: &str,
        status: RunStatus,
        error: Option<&str>,
    ) -> FactoryResult<()> {
        sqlx::query("UPDATE factory_runs SET status = ?, error = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(error)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_run_layout(
        &self,
        id: &str,
        branch: &str,
        worktree_path: &str,
        base_sha: &str,
    ) -> FactoryResult<()> {
        sqlx::query(
            "UPDATE factory_runs SET branch = ?, worktree_path = ?, base_sha = ? WHERE id = ?",
        )
        .bind(branch)
        .bind(worktree_path)
        .bind(base_sha)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn add_run_usage(
        &self,
        id: &str,
        tokens_in: i64,
        tokens_out: i64,
        cost_usd: f64,
    ) -> FactoryResult<()> {
        sqlx::query(
            "UPDATE factory_runs
             SET tokens_in = tokens_in + ?, tokens_out = tokens_out + ?, cost_usd = cost_usd + ?
             WHERE id = ?",
        )
        .bind(tokens_in)
        .bind(tokens_out)
        .bind(cost_usd)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // --- Task --------------------------------------------------------------

    pub async fn create_task(
        &self,
        run_id: &str,
        title: &str,
        description: Option<&str>,
        role: TaskRole,
        model: &str,
    ) -> FactoryResult<Task> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO factory_tasks (id, run_id, title, description, role, model, status)
             VALUES (?, ?, ?, ?, ?, ?, 'pending')",
        )
        .bind(&id)
        .bind(run_id)
        .bind(title)
        .bind(description)
        .bind(role.as_str())
        .bind(model)
        .execute(&self.pool)
        .await?;
        self.get_task(&id).await
    }

    pub async fn get_task(&self, id: &str) -> FactoryResult<Task> {
        let row = sqlx::query("SELECT * FROM factory_tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| FactoryError::NotFound(format!("task {id}")))?;
        row_to_task(&row)
    }

    pub async fn list_tasks(&self, run_id: &str) -> FactoryResult<Vec<Task>> {
        let rows = sqlx::query("SELECT * FROM factory_tasks WHERE run_id = ?")
            .bind(run_id)
            .fetch_all(&self.pool)
            .await?;
        let mut tasks = rows
            .iter()
            .map(row_to_task)
            .collect::<FactoryResult<Vec<_>>>()?;
        tasks.sort_by_key(|task| task.role.order());
        Ok(tasks)
    }

    pub async fn next_pending_task(&self, run_id: &str) -> FactoryResult<Option<Task>> {
        let rows =
            sqlx::query("SELECT * FROM factory_tasks WHERE run_id = ? AND status = 'pending'")
                .bind(run_id)
                .fetch_all(&self.pool)
                .await?;
        let mut tasks: Vec<Task> = rows.iter().map(row_to_task).collect::<Result<_, _>>()?;
        tasks.sort_by_key(|t| t.role.order());
        Ok(tasks.into_iter().next())
    }

    pub async fn start_task(&self, id: &str) -> FactoryResult<()> {
        sqlx::query(
            "UPDATE factory_tasks
             SET status = 'running', started_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn finish_task(
        &self,
        id: &str,
        status: TaskStatus,
        output: Option<&str>,
        error: Option<&str>,
        tokens_in: i64,
        tokens_out: i64,
        cost_usd: f64,
    ) -> FactoryResult<()> {
        sqlx::query(
            "UPDATE factory_tasks
             SET status = ?, output = ?, error = ?,
                 completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 tokens_in = ?, tokens_out = ?, cost_usd = ?
             WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(output)
        .bind(error)
        .bind(tokens_in)
        .bind(tokens_out)
        .bind(cost_usd)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // --- Events ------------------------------------------------------------

    pub async fn append_event(
        &self,
        run_id: &str,
        task_id: Option<&str>,
        kind: &str,
        payload: Option<&str>,
    ) -> FactoryResult<()> {
        sqlx::query(
            "INSERT INTO factory_events (run_id, task_id, kind, payload) VALUES (?, ?, ?, ?)",
        )
        .bind(run_id)
        .bind(task_id)
        .bind(kind)
        .bind(payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_events(&self, run_id: &str) -> FactoryResult<Vec<Event>> {
        let rows = sqlx::query(
            "SELECT id, run_id, task_id, kind, payload, created_at
             FROM factory_events WHERE run_id = ? ORDER BY id ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Event {
                    id: r.try_get("id")?,
                    run_id: r.try_get("run_id")?,
                    task_id: r.try_get("task_id")?,
                    kind: r.try_get("kind")?,
                    payload: r.try_get("payload")?,
                    created_at: parse_ts(r.try_get("created_at")?)?,
                })
            })
            .collect()
    }
}

// --- row mappers ----------------------------------------------------------

fn parse_ts(s: String) -> FactoryResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| FactoryError::InvalidSpec(format!("bad timestamp `{s}`: {e}")))
}

fn parse_ts_opt(s: Option<String>) -> FactoryResult<Option<DateTime<Utc>>> {
    match s {
        Some(s) => parse_ts(s).map(Some),
        None => Ok(None),
    }
}

fn row_to_spec(r: &sqlx::sqlite::SqliteRow) -> FactoryResult<Spec> {
    let status: String = r.try_get("status")?;
    Ok(Spec {
        id: r.try_get("id")?,
        workspace_id: r.try_get("workspace_id")?,
        name: r.try_get("name")?,
        kind: r.try_get("kind")?,
        project: r.try_get("project")?,
        body: r.try_get("body")?,
        parsed_json: r.try_get("parsed_json")?,
        status: SpecStatus::parse(&status)
            .ok_or_else(|| FactoryError::InvalidSpec(format!("unknown spec status `{status}`")))?,
        created_at: parse_ts(r.try_get("created_at")?)?,
    })
}

fn row_to_run(r: &sqlx::sqlite::SqliteRow) -> FactoryResult<Run> {
    let status: String = r.try_get("status")?;
    Ok(Run {
        id: r.try_get("id")?,
        spec_id: r.try_get("spec_id")?,
        workspace_id: r.try_get("workspace_id")?,
        status: RunStatus::parse(&status)
            .ok_or_else(|| FactoryError::InvalidSpec(format!("unknown run status `{status}`")))?,
        started_at: parse_ts_opt(r.try_get("started_at")?)?,
        completed_at: parse_ts_opt(r.try_get("completed_at")?)?,
        cost_usd: r.try_get("cost_usd")?,
        tokens_in: r.try_get("tokens_in")?,
        tokens_out: r.try_get("tokens_out")?,
        error: r.try_get("error")?,
        created_at: parse_ts(r.try_get("created_at")?)?,
        branch: r.try_get("branch")?,
        worktree_path: r.try_get("worktree_path")?,
        base_sha: r.try_get("base_sha")?,
    })
}

fn row_to_task(r: &sqlx::sqlite::SqliteRow) -> FactoryResult<Task> {
    let status: String = r.try_get("status")?;
    let role: String = r.try_get("role")?;
    Ok(Task {
        id: r.try_get("id")?,
        run_id: r.try_get("run_id")?,
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        role: TaskRole::parse(&role)
            .ok_or_else(|| FactoryError::InvalidSpec(format!("unknown role `{role}`")))?,
        model: r.try_get("model")?,
        status: TaskStatus::parse(&status)
            .ok_or_else(|| FactoryError::InvalidSpec(format!("unknown task status `{status}`")))?,
        output: r.try_get("output")?,
        started_at: parse_ts_opt(r.try_get("started_at")?)?,
        completed_at: parse_ts_opt(r.try_get("completed_at")?)?,
        cost_usd: r.try_get("cost_usd")?,
        tokens_in: r.try_get("tokens_in")?,
        tokens_out: r.try_get("tokens_out")?,
        error: r.try_get("error")?,
    })
}
