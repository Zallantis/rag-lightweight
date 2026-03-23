use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "wzd-rag-lightweight", about = "Lightweight RAG system")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// SurrealDB data directory
    #[arg(long, env = "DB_PATH", default_value = "./data/surreal", global = true)]
    pub db_path: PathBuf,

    /// Log level
    #[arg(long, env = "LOG_LEVEL", default_value = "info", global = true)]
    pub log_level: String,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start MCP server for search requests
    Serve {
        /// Listen address
        #[arg(long, env = "HOST", default_value = "127.0.0.1")]
        host: String,
        /// Listen port
        #[arg(long, env = "PORT", default_value_t = 3100)]
        port: u16,
    },
    /// Scan files, create documents, chunk
    Ingest {
        /// Path to scan
        path: PathBuf,
        /// Filter by file extensions (comma-separated)
        #[arg(long)]
        extensions: Option<String>,
        /// Exclude glob patterns (comma-separated)
        #[arg(long)]
        exclude: Option<String>,
        /// Source label
        #[arg(long, default_value = "local")]
        source: String,
        /// Max tokens per chunk
        #[arg(long, env = "MAX_CHUNK_TOKENS", default_value_t = 512)]
        max_tokens: usize,
    },
    /// Embed all pending chunks via external API
    Embed {
        /// Texts per API call
        #[arg(long, env = "EMBEDDING_BATCH_SIZE", default_value_t = 64)]
        batch_size: usize,
        /// Re-embed all chunks, not just pending
        #[arg(long)]
        force: bool,
    },
    /// Show DB statistics
    Status,
}

#[derive(Clone, Debug)]
pub struct EmbeddingConfig {
    pub api_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub dimension: usize,
}

impl EmbeddingConfig {
    pub fn from_env() -> crate::error::Result<Self> {
        let api_url = std::env::var("EMBEDDING_API_URL")
            .map_err(|_| crate::error::AppError::Config("EMBEDDING_API_URL is required".into()))?;
        let api_key = std::env::var("EMBEDDING_API_KEY").ok();
        let model = std::env::var("EMBEDDING_MODEL")
            .map_err(|_| crate::error::AppError::Config("EMBEDDING_MODEL is required".into()))?;
        let dimension = std::env::var("EMBEDDING_DIMENSION")
            .map_err(|_| crate::error::AppError::Config("EMBEDDING_DIMENSION is required".into()))?
            .parse::<usize>()
            .map_err(|_| crate::error::AppError::Config("EMBEDDING_DIMENSION must be a number".into()))?;

        Ok(Self { api_url, api_key, model, dimension })
    }
}

pub struct SearchConfig {
    pub retrieve_limit: usize,
    pub top_k: usize,
}

impl SearchConfig {
    pub fn from_env() -> Self {
        Self {
            retrieve_limit: std::env::var("RETRIEVE_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
            top_k: std::env::var("SEARCH_TOP_K")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_embedding_env(url: &str, model: &str, dimension: &str) {
        unsafe {
            std::env::set_var("EMBEDDING_API_URL", url);
            std::env::set_var("EMBEDDING_MODEL", model);
            std::env::set_var("EMBEDDING_DIMENSION", dimension);
            std::env::remove_var("EMBEDDING_API_KEY");
        }
    }

    fn clear_embedding_env() {
        unsafe {
            std::env::remove_var("EMBEDDING_API_URL");
            std::env::remove_var("EMBEDDING_MODEL");
            std::env::remove_var("EMBEDDING_DIMENSION");
            std::env::remove_var("EMBEDDING_API_KEY");
        }
    }

    #[test]
    fn embedding_config_succeeds_with_all_required_vars() {
        let _lock = ENV_LOCK.lock().unwrap();
        set_embedding_env("http://localhost:8080", "text-embed-v1", "768");
        let config = EmbeddingConfig::from_env().unwrap();
        assert_eq!(config.api_url, "http://localhost:8080");
        assert_eq!(config.model, "text-embed-v1");
        assert_eq!(config.dimension, 768);
        assert!(config.api_key.is_none());
        clear_embedding_env();
    }

    #[test]
    fn embedding_config_includes_api_key_when_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        set_embedding_env("http://localhost", "model", "512");
        unsafe { std::env::set_var("EMBEDDING_API_KEY", "sk-secret") };
        let config = EmbeddingConfig::from_env().unwrap();
        assert_eq!(config.api_key, Some("sk-secret".to_string()));
        clear_embedding_env();
    }

    #[test]
    fn embedding_config_fails_when_api_url_missing() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_embedding_env();
        unsafe {
            std::env::set_var("EMBEDDING_MODEL", "model");
            std::env::set_var("EMBEDDING_DIMENSION", "768");
        }
        let result = EmbeddingConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("EMBEDDING_API_URL"));
        clear_embedding_env();
    }

    #[test]
    fn embedding_config_fails_when_dimension_not_a_number() {
        let _lock = ENV_LOCK.lock().unwrap();
        set_embedding_env("http://localhost", "model", "not-a-number");
        let result = EmbeddingConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("EMBEDDING_DIMENSION"));
        clear_embedding_env();
    }

    #[test]
    fn embedding_config_dimension_zero_is_accepted() {
        // Documents current behaviour: dimension=0 is NOT validated, passes through.
        // This test should be updated to assert an error once validation is added.
        let _lock = ENV_LOCK.lock().unwrap();
        set_embedding_env("http://localhost", "model", "0");
        let result = EmbeddingConfig::from_env();
        // Currently succeeds — this is the bug
        assert!(result.is_ok());
        assert_eq!(result.unwrap().dimension, 0);
        clear_embedding_env();
    }

    #[test]
    fn search_config_uses_defaults_when_vars_not_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("RETRIEVE_LIMIT");
            std::env::remove_var("SEARCH_TOP_K");
        }
        let config = SearchConfig::from_env();
        assert_eq!(config.retrieve_limit, 100);
        assert_eq!(config.top_k, 5);
    }

    #[test]
    fn search_config_parses_valid_values() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("RETRIEVE_LIMIT", "50");
            std::env::set_var("SEARCH_TOP_K", "10");
        }
        let config = SearchConfig::from_env();
        assert_eq!(config.retrieve_limit, 50);
        assert_eq!(config.top_k, 10);
        unsafe {
            std::env::remove_var("RETRIEVE_LIMIT");
            std::env::remove_var("SEARCH_TOP_K");
        }
    }

    #[test]
    fn search_config_falls_back_to_default_on_invalid_value() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("RETRIEVE_LIMIT", "not-a-number") };
        let config = SearchConfig::from_env();
        assert_eq!(config.retrieve_limit, 100, "Invalid value should use default");
        unsafe { std::env::remove_var("RETRIEVE_LIMIT") };
    }
}
