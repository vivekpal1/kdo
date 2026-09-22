//! Inline SQL migrations. Idempotent — run on every store init.
//!
//! The `factory_*` prefix avoids collisions with kdow's `users`,
//! `workspaces`, `tasks`, etc. tables when the same SQLite file is
//! shared between the two systems.

use sqlx::SqlitePool;

pub async fn run(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let stmts = [
        "CREATE TABLE IF NOT EXISTS factory_specs (
            id TEXT PRIMARY KEY,
            workspace_id TEXT,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            project TEXT,
            body TEXT NOT NULL,
            parsed_json TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
        "CREATE INDEX IF NOT EXISTS idx_factory_specs_status ON factory_specs(status)",
        "CREATE INDEX IF NOT EXISTS idx_factory_specs_workspace ON factory_specs(workspace_id)",
        "CREATE TABLE IF NOT EXISTS factory_runs (
            id TEXT PRIMARY KEY,
            spec_id TEXT NOT NULL REFERENCES factory_specs(id) ON DELETE CASCADE,
            workspace_id TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            started_at TEXT,
            completed_at TEXT,
            cost_usd REAL NOT NULL DEFAULT 0,
            tokens_in INTEGER NOT NULL DEFAULT 0,
            tokens_out INTEGER NOT NULL DEFAULT 0,
            error TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
        "CREATE INDEX IF NOT EXISTS idx_factory_runs_status ON factory_runs(status)",
        "CREATE INDEX IF NOT EXISTS idx_factory_runs_spec ON factory_runs(spec_id)",
        "CREATE INDEX IF NOT EXISTS idx_factory_runs_workspace ON factory_runs(workspace_id)",
        "CREATE TABLE IF NOT EXISTS factory_tasks (
            id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL REFERENCES factory_runs(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            description TEXT,
            role TEXT NOT NULL,
            model TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            output TEXT,
            started_at TEXT,
            completed_at TEXT,
            cost_usd REAL NOT NULL DEFAULT 0,
            tokens_in INTEGER NOT NULL DEFAULT 0,
            tokens_out INTEGER NOT NULL DEFAULT 0,
            error TEXT
        )",
        "CREATE INDEX IF NOT EXISTS idx_factory_tasks_run ON factory_tasks(run_id)",
        "CREATE INDEX IF NOT EXISTS idx_factory_tasks_status ON factory_tasks(status)",
        "CREATE TABLE IF NOT EXISTS factory_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id TEXT NOT NULL,
            task_id TEXT,
            kind TEXT NOT NULL,
            payload TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
        "CREATE INDEX IF NOT EXISTS idx_factory_events_run ON factory_events(run_id)",
    ];

    for stmt in stmts {
        sqlx::query(stmt).execute(pool).await?;
    }

    // Additive columns. Duplicate-column errors mean a later open of an
    // already-migrated file — ignore those, fail on anything else.
    for stmt in [
        "ALTER TABLE factory_runs ADD COLUMN branch TEXT",
        "ALTER TABLE factory_runs ADD COLUMN worktree_path TEXT",
        "ALTER TABLE factory_runs ADD COLUMN base_sha TEXT",
    ] {
        if let Err(err) = sqlx::query(stmt).execute(pool).await {
            let msg = err.to_string().to_lowercase();
            if !msg.contains("duplicate column") {
                return Err(err);
            }
        }
    }
    Ok(())
}
