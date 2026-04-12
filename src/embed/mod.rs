pub mod grpc_adapter;
pub mod http_adapter;
mod proto;
pub mod service;

use crate::config::{EmbeddingConfig, EmbeddingProvider};
use crate::embed::service::EmbeddingService;
use std::sync::Arc;

pub fn create_embedding_service(
    config: &EmbeddingConfig,
) -> crate::error::Result<Arc<dyn EmbeddingService>> {
    match config.provider {
        EmbeddingProvider::Http => Ok(Arc::new(http_adapter::HttpEmbeddingService::new(
            config.clone(),
        ))),
        EmbeddingProvider::Grpc => {
            let url = config.grpc_url.as_deref().ok_or_else(|| {
                crate::error::AppError::Config(
                    "EMBED_SERVICE_URL is required for gRPC provider".into(),
                )
            })?;
            Ok(Arc::new(grpc_adapter::GrpcEmbeddingService::new(
                url,
                config.model.clone(),
                config.grpc_auth_token.clone(),
                config.grpc_ca_cert_path.as_deref(),
            )?))
        }
    }
}
