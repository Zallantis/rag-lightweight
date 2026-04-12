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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProvider {
    Http,
    Grpc,
}

#[derive(Clone, Debug)]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProvider,
    pub api_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub dimension: usize,
    pub grpc_url: Option<String>,
    pub grpc_auth_token: Option<String>,
    pub grpc_ca_cert_path: Option<String>,
}

impl EmbeddingConfig {
    pub fn from_env() -> crate::error::Result<Self> {
        let provider = match std::env::var("EMBEDDING_PROVIDER").ok().as_deref() {
            Some("grpc") => EmbeddingProvider::Grpc,
            _ => EmbeddingProvider::Http,
        };

        let model = std::env::var("EMBEDDING_MODEL")
            .map_err(|_| crate::error::AppError::Config("EMBEDDING_MODEL is required".into()))?;
        let dimension = std::env::var("EMBEDDING_DIMENSION")
            .map_err(|_| crate::error::AppError::Config("EMBEDDING_DIMENSION is required".into()))?
            .parse::<usize>()
            .map_err(|_| {
                crate::error::AppError::Config("EMBEDDING_DIMENSION must be a number".into())
            })?;

        let (api_url, api_key, grpc_url, grpc_auth_token, grpc_ca_cert_path) = match provider {
            EmbeddingProvider::Http => {
                let api_url = std::env::var("EMBEDDING_API_URL").map_err(|_| {
                    crate::error::AppError::Config("EMBEDDING_API_URL is required".into())
                })?;
                let api_key = std::env::var("EMBEDDING_API_KEY").ok();
                (api_url, api_key, None, None, None)
            }
            EmbeddingProvider::Grpc => {
                let grpc_url = std::env::var("INFERENCE_SERVICE_URL")
                    .unwrap_or_else(|_| "http://localhost:50060".to_string());
                let grpc_auth_token = std::env::var("INFERENCE_SERVICE_AUTH_TOKEN").ok();
                let grpc_ca_cert_path = std::env::var("INFERENCE_SERVICE_CA_CERT").ok();
                (
                    String::new(),
                    None,
                    Some(grpc_url),
                    grpc_auth_token,
                    grpc_ca_cert_path,
                )
            }
        };

        Ok(Self {
            provider,
            api_url,
            api_key,
            model,
            dimension,
            grpc_url,
            grpc_auth_token,
            grpc_ca_cert_path,
        })
    }
}

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub token: Option<String>,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        Self {
            token: std::env::var("MCP_AUTH_TOKEN")
                .ok()
                .filter(|t| !t.is_empty()),
        }
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
            std::env::remove_var("EMBEDDING_PROVIDER");
            std::env::set_var("EMBEDDING_API_URL", url);
            std::env::set_var("EMBEDDING_MODEL", model);
            std::env::set_var("EMBEDDING_DIMENSION", dimension);
            std::env::remove_var("EMBEDDING_API_KEY");
        }
    }

    fn clear_embedding_env() {
        unsafe {
            std::env::remove_var("EMBEDDING_PROVIDER");
            std::env::remove_var("EMBEDDING_API_URL");
            std::env::remove_var("EMBEDDING_MODEL");
            std::env::remove_var("EMBEDDING_DIMENSION");
            std::env::remove_var("EMBEDDING_API_KEY");
            std::env::remove_var("INFERENCE_SERVICE_URL");
            std::env::remove_var("INFERENCE_SERVICE_AUTH_TOKEN");
            std::env::remove_var("INFERENCE_SERVICE_CA_CERT");
        }
    }

    #[test]
    fn embedding_config_succeeds_with_all_required_vars() {
        let _lock = ENV_LOCK.lock().unwrap();
        set_embedding_env("http://localhost:8080", "text-embed-v1", "768");
        let config = EmbeddingConfig::from_env().unwrap();
        assert_eq!(config.provider, EmbeddingProvider::Http);
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("EMBEDDING_API_URL")
        );
        clear_embedding_env();
    }

    #[test]
    fn embedding_config_fails_when_dimension_not_a_number() {
        let _lock = ENV_LOCK.lock().unwrap();
        set_embedding_env("http://localhost", "model", "not-a-number");
        let result = EmbeddingConfig::from_env();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("EMBEDDING_DIMENSION")
        );
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
    fn auth_config_returns_none_when_var_not_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("MCP_AUTH_TOKEN") };
        let config = AuthConfig::from_env();
        assert!(config.token.is_none());
    }

    #[test]
    fn auth_config_returns_none_when_var_empty() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("MCP_AUTH_TOKEN", "") };
        let config = AuthConfig::from_env();
        assert!(config.token.is_none());
        unsafe { std::env::remove_var("MCP_AUTH_TOKEN") };
    }

    #[test]
    fn auth_config_returns_token_when_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("MCP_AUTH_TOKEN", "abc123") };
        let config = AuthConfig::from_env();
        assert_eq!(config.token, Some("abc123".to_string()));
        unsafe { std::env::remove_var("MCP_AUTH_TOKEN") };
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
        assert_eq!(
            config.retrieve_limit, 100,
            "Invalid value should use default"
        );
        unsafe { std::env::remove_var("RETRIEVE_LIMIT") };
    }

    #[test]
    fn embedding_config_grpc_provider_uses_defaults() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_embedding_env();
        unsafe {
            std::env::set_var("EMBEDDING_PROVIDER", "grpc");
            std::env::set_var("EMBEDDING_MODEL", "bge-m3");
            std::env::set_var("EMBEDDING_DIMENSION", "1024");
        }
        let config = EmbeddingConfig::from_env().unwrap();
        assert_eq!(config.provider, EmbeddingProvider::Grpc);
        assert_eq!(config.grpc_url, Some("http://localhost:50060".to_string()));
        assert!(config.grpc_auth_token.is_none());
        assert!(config.grpc_ca_cert_path.is_none());
        clear_embedding_env();
    }

    #[test]
    fn embedding_config_grpc_provider_reads_tokens() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_embedding_env();
        unsafe {
            std::env::set_var("EMBEDDING_PROVIDER", "grpc");
            std::env::set_var("EMBEDDING_MODEL", "bge-m3");
            std::env::set_var("EMBEDDING_DIMENSION", "1024");
            std::env::set_var("INFERENCE_SERVICE_URL", "http://gpu-host:50060");
            std::env::set_var("INFERENCE_SERVICE_AUTH_TOKEN", "embed-tok");
        }
        let config = EmbeddingConfig::from_env().unwrap();
        assert_eq!(config.grpc_url, Some("http://gpu-host:50060".to_string()));
        assert_eq!(config.grpc_auth_token, Some("embed-tok".to_string()));
        clear_embedding_env();
    }

    #[test]
    fn embedding_config_grpc_does_not_require_api_url() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_embedding_env();
        unsafe {
            std::env::set_var("EMBEDDING_PROVIDER", "grpc");
            std::env::set_var("EMBEDDING_MODEL", "model");
            std::env::set_var("EMBEDDING_DIMENSION", "768");
        }
        let result = EmbeddingConfig::from_env();
        assert!(result.is_ok());
        clear_embedding_env();
    }
}
