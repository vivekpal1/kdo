//! Resource types — the rows persisted to SQLite, plus their string-keyed enums.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecStatus {
    Pending,
    Planning,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl SpecStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Planning => "planning",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "planning" => Self::Planning,
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    /// Review passed, but the main checkout had changes outside `.kdo/`.
    AwaitingMerge,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::AwaitingMerge => "awaiting_merge",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "awaiting_merge" => Self::AwaitingMerge,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRole {
    Planner,
    Implementer,
    Reviewer,
    Tester,
}

impl TaskRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Implementer => "implementer",
            Self::Reviewer => "reviewer",
            Self::Tester => "tester",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "planner" => Self::Planner,
            "implementer" => Self::Implementer,
            "reviewer" => Self::Reviewer,
            "tester" => Self::Tester,
            _ => return None,
        })
    }
    /// Execution order within a run — lower runs first.
    ///
    /// Test runs before review so a failing suite never merges.
    pub fn order(self) -> u8 {
        match self {
            Self::Planner => 0,
            Self::Implementer => 1,
            Self::Tester => 2,
            Self::Reviewer => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub id: String,
    pub workspace_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub project: Option<String>,
    pub body: String,
    pub parsed_json: String,
    pub status: SpecStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub spec_id: String,
    pub workspace_id: Option<String>,
    pub status: RunStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cost_usd: f64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Branch created for this run (`kdo/run/<id>`).
    pub branch: Option<String>,
    /// Absolute or workspace-relative worktree path.
    pub worktree_path: Option<String>,
    /// `HEAD` at the moment the worktree was created.
    pub base_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub run_id: String,
    pub title: String,
    pub description: Option<String>,
    pub role: TaskRole,
    pub model: String,
    pub status: TaskStatus,
    pub output: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cost_usd: f64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub run_id: String,
    pub task_id: Option<String>,
    pub kind: String,
    pub payload: Option<String>,
    pub created_at: DateTime<Utc>,
}
