use thiserror::Error;

#[derive(Debug, Error)]
pub enum FactoryError {
    #[error("sqlite error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yml::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid spec: {0}")]
    InvalidSpec(String),

    #[error("connector error: {0}")]
    Connector(String),

    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),

    #[error("worktree error: {0}")]
    Worktree(String),

    #[error("working tree has uncommitted changes outside .kdo/")]
    DirtyWorktree,

    #[error("plugin error: {0}")]
    Plugin(String),

    #[error("unknown provider for model {0}")]
    UnknownProvider(String),

    #[error("missing credential for {0}")]
    MissingCredential(String),

    #[error("tool denied: {0}")]
    ToolDenied(String),

    #[error("invalid review verdict: {0}")]
    InvalidVerdict(String),

    #[error("merge conflict: {0}")]
    MergeConflict(String),
}

pub type FactoryResult<T> = Result<T, FactoryError>;
