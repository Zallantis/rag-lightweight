use std::sync::Arc;

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

use crate::config::EmbeddingConfig;
use crate::db::SurrealClient;
use crate::search::pipeline::SearchPipeline;

use super::tools::{ContextSearchInput, GetDocumentInput, ListDocumentsInput, StatsOutput};

#[derive(Clone)]
pub struct RagServer {
    pipeline: Arc<SearchPipeline>,
    db: SurrealClient,
    embedding_config: EmbeddingConfig,
    tool_router: ToolRouter<Self>,
}

impl RagServer {
    pub fn new(
        pipeline: Arc<SearchPipeline>,
        db: SurrealClient,
        embedding_config: EmbeddingConfig,
    ) -> Self {
        Self {
            pipeline,
            db,
            embedding_config,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl RagServer {
    #[tool(description = "Search the knowledge base using semantic + full-text hybrid search. Returns ranked results with document context.")]
    async fn context_search(
        &self,
        Parameters(input): Parameters<ContextSearchInput>,
    ) -> String {
        if input.query.is_empty() || input.query.len() > 8192 {
            return serde_json::json!({"error": "query must be between 1 and 8192 characters"}).to_string();
        }
        if input.source.as_deref().map(|s| s.len()).unwrap_or(0) > 256 {
            return serde_json::json!({"error": "source filter must be 256 characters or fewer"}).to_string();
        }
        let limit = input.limit.unwrap_or(5).min(50);
        let source = input.source.as_deref();
        tracing::info!(query = %input.query, limit, source = ?source, "context_search");
        match self.pipeline.search(&input.query, Some(limit), source).await {
            Ok(results) => {
                tracing::debug!(count = results.len(), "context_search complete");
                serde_json::to_string(&results).unwrap_or_else(|e| {
                    tracing::error!(tool = "context_search", error = %e, "serialization failed");
                    serde_json::json!({"error": "serialization failed"}).to_string()
                })
            }
            Err(e) => {
                tracing::error!(tool = "context_search", error = %e, "tool execution failed");
                serde_json::json!({"error": e.to_string()}).to_string()
            }
        }
    }

    #[tool(description = "Retrieve a single document by its ID.")]
    async fn get_document(&self, Parameters(input): Parameters<GetDocumentInput>) -> String {
        if input.id.is_empty() || input.id.len() > 256 {
            return serde_json::json!({"error": "id must be between 1 and 256 characters"}).to_string();
        }
        tracing::info!(id = %input.id, "get_document");
        match crate::db::documents::get_document(&self.db, &input.id).await {
            Ok(Some(doc)) => {
                tracing::debug!(id = %input.id, "get_document found");
                serde_json::to_string(&doc).unwrap_or_else(|e| {
                    tracing::error!(tool = "get_document", error = %e, "serialization failed");
                    serde_json::json!({"error": "serialization failed"}).to_string()
                })
            }
            Ok(None) => {
                tracing::debug!(id = %input.id, "get_document not found");
                serde_json::json!({"error": format!("document not found: {}", input.id)}).to_string()
            }
            Err(e) => {
                tracing::error!(tool = "get_document", error = %e, "tool execution failed");
                serde_json::json!({"error": e.to_string()}).to_string()
            }
        }
    }

    #[tool(description = "List documents with optional filtering by source. Supports pagination via limit and offset.")]
    async fn list_documents(
        &self,
        Parameters(input): Parameters<ListDocumentsInput>,
    ) -> String {
        if input.source.as_deref().map(|s| s.len()).unwrap_or(0) > 256 {
            return serde_json::json!({"error": "source filter must be 256 characters or fewer"}).to_string();
        }
        let limit = input.limit.unwrap_or(20).min(200);
        let offset = input.offset.unwrap_or(0);
        let source = input.source.as_deref();
        tracing::info!(limit, offset, source = ?source, "list_documents");
        match crate::db::documents::list_documents(&self.db, source, limit, offset).await {
            Ok(docs) => {
                tracing::debug!(count = docs.len(), "list_documents complete");
                serde_json::to_string(&docs).unwrap_or_else(|e| {
                    tracing::error!(tool = "list_documents", error = %e, "serialization failed");
                    serde_json::json!({"error": "serialization failed"}).to_string()
                })
            }
            Err(e) => {
                tracing::error!(tool = "list_documents", error = %e, "tool execution failed");
                serde_json::json!({"error": e.to_string()}).to_string()
            }
        }
    }

    #[tool(description = "Get statistics about the knowledge base including document counts, chunk counts, and embedding status.")]
    async fn stats(&self) -> String {
        tracing::info!("stats");
        let (documents, chunk_counts, by_source) =
            match crate::db::stats::get_stats(&self.db).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(tool = "stats", error = %e, "get_stats failed");
                    return serde_json::json!({"error": "failed to query stats"}).to_string();
                }
            };
        let output = StatsOutput {
            documents,
            chunks: chunk_counts.total,
            embedded_chunks: chunk_counts.embedded,
            pending_chunks: chunk_counts.pending,
            documents_by_source: by_source,
            embedding_model: self.embedding_config.model.clone(),
            embedding_dimension: self.embedding_config.dimension,
        };
        tracing::debug!(
            documents = output.documents,
            chunks = output.chunks,
            embedded = output.embedded_chunks,
            pending = output.pending_chunks,
            "stats complete"
        );
        serde_json::to_string(&output).unwrap_or_else(|e| {
            tracing::error!(tool = "stats", error = %e, "serialization failed");
            serde_json::json!({"error": "serialization failed"}).to_string()
        })
    }
}

#[tool_handler]
impl ServerHandler for RagServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("wzd-rag-lightweight", "0.1.0"))
    }
}
