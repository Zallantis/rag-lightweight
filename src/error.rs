use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Ingest error: {0}")]
    Ingest(String),

    #[error("Search error: {0}")]
    Search(String),

    #[error("Hierarchy error: {0}")]
    Hierarchy(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
