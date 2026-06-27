use thiserror::Error;

#[derive(Error,Debug)]
pub enum AppError{
    #[error("Generic error: {0}")]
    Other(String),
    #[error("Azure error: {0}")]
    Azure(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Serde json error: {0}")]
    Json(#[from] serde_json::Error),
}
