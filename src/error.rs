use thiserror::Error;

#[derive(Error,Debug)]
pub enum AppError{
    #[error("Generic error: {0}")]
    Other(String),
    #[error("Azure error: {0}")]
    Azure(String),
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
}
