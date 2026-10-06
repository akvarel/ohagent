use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EloopError {
    #[error("repository does not exist or is not a directory: {0}")]
    InvalidRepository(PathBuf),

    #[error("goal cannot be empty")]
    EmptyGoal,

    #[error("task contract is invalid: {0}")]
    InvalidTask(String),

    #[error("run already exists: {0}")]
    RunAlreadyExists(PathBuf),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("command failed: {0}")]
    Command(String),
}

pub type Result<T> = std::result::Result<T, EloopError>;

pub(crate) fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> EloopError {
    EloopError::Io {
        path: path.into(),
        source,
    }
}
