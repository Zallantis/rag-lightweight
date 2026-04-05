use std::sync::Arc;
use std::path::PathBuf;

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::config::{EmbeddingConfig, SearchConfig};
use crate::db;
use crate::embed::http_adapter::HttpEmbeddingService;
use crate::embed::service::EmbeddingService;
use crate::mcp::server::RagServer;
use crate::search::pipeline::SearchPipeline;

pub async fn run(host: String, port: u16, db_path: PathBuf) -> crate::error::Result<()> {
    let embedding_config = EmbeddingConfig::from_env()?;
    let search_config = SearchConfig::from_env();
    let db_client = db::init(&db_path, embedding_config.dimension).await?;

    let embedding_service: Arc<dyn EmbeddingService> =
        Arc::new(HttpEmbeddingService::new(embedding_config.clone()));

    let pipeline = Arc::new(SearchPipeline::new(
        db_client.clone(),
        embedding_service.clone(),
        search_config,
    ).await?);

    let ct = CancellationToken::new();

    let service = StreamableHttpService::new(
        {
            let pipeline = pipeline.clone();
            let db_client = db_client.clone();
            let embedding_config = embedding_config.clone();
            let embedding_service = embedding_service.clone();
            move || Ok(RagServer::new(
                pipeline.clone(),
                db_client.clone(),
                embedding_config.clone(),
                embedding_service.clone(),
            ))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig {
            stateful_mode: true,
            cancellation_token: ct.clone(),
            ..Default::default()
        },
    );

    let router = axum::Router::new()
        .nest_service("/mcp", service.clone())
        .nest_service("/http", service);

    let addr = format!("{host}:{port}");
    tracing::info!("MCP server starting on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| crate::error::AppError::Config(format!("failed to bind {addr}: {e}")))?;

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let ctrl_c = tokio::signal::ctrl_c();
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to register SIGTERM handler");

            tokio::select! {
                _ = ctrl_c => {},
                _ = sigterm.recv() => {},
            }

            tracing::info!("shutdown signal received, stopping MCP server");
            ct.cancel();
        })
        .await
        .map_err(|e| crate::error::AppError::Config(format!("server error: {e}")))?;

    Ok(())
}
