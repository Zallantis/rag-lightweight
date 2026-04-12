use std::sync::Arc;

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use sha2::{Digest, Sha256};
use surrealdb::types::{RecordId, RecordIdKey};

use crate::config::EmbeddingConfig;
use crate::db::SurrealClient;
use crate::embed::service::EmbeddingService;
use crate::search::pipeline::SearchPipeline;

use super::tools::{
    ContextSearchInput, CreateDocumentInput, DocumentIdInput, GetDocumentInput, ListDocumentsInput,
    MutationResult, SetDocumentParentInput, StatsOutput, UpdateDocumentInput, parse_content_type,
};

const DEFAULT_MAX_TOKENS: usize = 512;

#[derive(Clone)]
pub struct RagServer {
    pipeline: Arc<SearchPipeline>,
    db: SurrealClient,
    embedding_config: EmbeddingConfig,
    embedding_service: Arc<dyn EmbeddingService>,
    tool_router: ToolRouter<Self>,
}

impl RagServer {
    pub fn new(
        pipeline: Arc<SearchPipeline>,
        db: SurrealClient,
        embedding_config: EmbeddingConfig,
        embedding_service: Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            pipeline,
            db,
            embedding_config,
            embedding_service,
            tool_router: Self::tool_router(),
        }
    }

    async fn embed_document_chunks(&self, doc_id: &RecordId) -> crate::error::Result<usize> {
        let pending: Vec<crate::db::chunks::PendingChunk> = self
            .db
            .query("SELECT id, content FROM chunk WHERE document = $doc AND vector IS NONE")
            .bind(("doc", doc_id.clone()))
            .await?
            .take(0)?;

        if pending.is_empty() {
            return Ok(0);
        }

        let texts: Vec<String> = pending.iter().map(|c| c.content.clone()).collect();
        let vectors = self
            .embedding_service
            .embed_with_role(texts, crate::embed::service::EmbedRole::Passage)
            .await?;

        let updates: Vec<(RecordId, Vec<f32>)> = pending
            .iter()
            .zip(vectors.into_iter())
            .map(|(chunk, vector)| (chunk.id.clone(), vector))
            .collect();

        let count = updates.len();
        crate::db::chunks::bulk_update_chunk_vectors(&self.db, updates).await?;
        Ok(count)
    }
}

fn error_json(msg: &str) -> String {
    serde_json::json!({"error": msg}).to_string()
}

fn record_key_str(key: &RecordIdKey) -> String {
    match key {
        RecordIdKey::String(s) => s.clone(),
        RecordIdKey::Number(n) => n.to_string(),
        RecordIdKey::Uuid(u) => u.to_string(),
        other => format!("{:?}", other),
    }
}

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn generate_source_id(title: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let hash = &content_hash(title)[..8];
    format!("mcp-{ts}-{hash}")
}

#[tool_router]
impl RagServer {
    #[tool(
        description = "Search the knowledge base using semantic + full-text hybrid search. Returns ranked results with document context."
    )]
    async fn context_search(&self, Parameters(input): Parameters<ContextSearchInput>) -> String {
        if input.query.is_empty() || input.query.len() > 8192 {
            return error_json("query must be between 1 and 8192 characters");
        }
        if input.source.as_deref().map(|s| s.len()).unwrap_or(0) > 256 {
            return error_json("source filter must be 256 characters or fewer");
        }
        let limit = input.limit.unwrap_or(5).min(50);
        let source = input.source.as_deref();
        tracing::info!(query = %input.query, limit, source = ?source, "context_search");
        match self
            .pipeline
            .search(&input.query, Some(limit), source)
            .await
        {
            Ok(results) => {
                tracing::debug!(count = results.len(), "context_search complete");
                serde_json::to_string(&results).unwrap_or_else(|e| {
                    tracing::error!(tool = "context_search", error = %e, "serialization failed");
                    error_json("serialization failed")
                })
            }
            Err(e) => {
                tracing::error!(tool = "context_search", error = %e, "tool execution failed");
                error_json(&e.to_string())
            }
        }
    }

    #[tool(description = "Retrieve a single document by its ID.")]
    async fn get_document(&self, Parameters(input): Parameters<GetDocumentInput>) -> String {
        if input.id.is_empty() || input.id.len() > 256 {
            return error_json("id must be between 1 and 256 characters");
        }
        tracing::info!(id = %input.id, "get_document");
        match crate::db::documents::get_document(&self.db, &input.id).await {
            Ok(Some(doc)) => {
                tracing::debug!(id = %input.id, "get_document found");
                serde_json::to_string(&doc).unwrap_or_else(|e| {
                    tracing::error!(tool = "get_document", error = %e, "serialization failed");
                    error_json("serialization failed")
                })
            }
            Ok(None) => {
                tracing::debug!(id = %input.id, "get_document not found");
                error_json(&format!("document not found: {}", input.id))
            }
            Err(e) => {
                tracing::error!(tool = "get_document", error = %e, "tool execution failed");
                error_json(&e.to_string())
            }
        }
    }

    #[tool(
        description = "List documents with optional filtering by source and custom_attributes. Supports pagination via limit and offset. Use filters for custom_attributes DSL: operators $eq, $ne, $gt, $gte, $lt, $lte, $in, $contains, $any, $all; logical $and, $or; nested paths."
    )]
    async fn list_documents(&self, Parameters(input): Parameters<ListDocumentsInput>) -> String {
        if input.source.as_deref().map(|s| s.len()).unwrap_or(0) > 256 {
            return error_json("source filter must be 256 characters or fewer");
        }
        let limit = input.limit.unwrap_or(20).min(200);
        let offset = input.offset.unwrap_or(0);
        let source = input.source.as_deref();

        let filter = match &input.filters {
            Some(f) => match crate::db::filter::parse_filters(f) {
                Ok(r) => Some(r),
                Err(e) => return error_json(&format!("invalid filters: {e}")),
            },
            None => None,
        };

        tracing::info!(limit, offset, source = ?source, has_filters = filter.is_some(), "list_documents");
        match crate::db::documents::list_documents(&self.db, source, limit, offset, filter.as_ref())
            .await
        {
            Ok(docs) => {
                tracing::debug!(count = docs.len(), "list_documents complete");
                serde_json::to_string(&docs).unwrap_or_else(|e| {
                    tracing::error!(tool = "list_documents", error = %e, "serialization failed");
                    error_json("serialization failed")
                })
            }
            Err(e) => {
                tracing::error!(tool = "list_documents", error = %e, "tool execution failed");
                error_json(&e.to_string())
            }
        }
    }

    #[tool(
        description = "Get statistics about the knowledge base including document counts, chunk counts, and embedding status."
    )]
    async fn stats(&self) -> String {
        tracing::info!("stats");
        let (documents, chunk_counts, by_source) = match crate::db::stats::get_stats(&self.db).await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(tool = "stats", error = %e, "get_stats failed");
                return error_json("failed to query stats");
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
            error_json("serialization failed")
        })
    }

    #[tool(
        description = "Create a new document in the knowledge base. Content is immediately chunked, embedded, and indexed for search."
    )]
    async fn create_document(&self, Parameters(input): Parameters<CreateDocumentInput>) -> String {
        if input.title.is_empty() || input.title.len() > 1024 {
            return error_json("title must be between 1 and 1024 characters");
        }
        if input.content.is_empty() || input.content.len() > 1_000_000 {
            return error_json("content must be between 1 and 1,000,000 characters");
        }

        let source = input.source.unwrap_or_else(|| "mcp".to_string());
        let source_id = input
            .source_id
            .unwrap_or_else(|| generate_source_id(&input.title));
        let hash = content_hash(&input.content);

        let (doc_id, _) = match crate::db::documents::upsert_document(
            &self.db,
            &source,
            &source_id,
            &input.title,
            &input.content,
            &hash,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => return error_json(&e.to_string()),
        };

        if input.custom_attributes.is_some()
            && let Err(e) = crate::db::documents::update_document_fields(
                &self.db,
                &doc_id,
                None,
                input.custom_attributes.as_ref(),
            )
            .await
        {
            tracing::warn!(error = %e, "failed to set custom_attributes");
        }

        let file_type = parse_content_type(input.content_type.as_deref(), &input.content);
        let mut parser = tree_sitter::Parser::new();
        let chunks = crate::ingest::chunker::chunk_content(
            &input.content,
            &file_type,
            DEFAULT_MAX_TOKENS,
            &mut parser,
        );

        let chunk_data: Vec<(String, usize, Option<String>)> = chunks
            .iter()
            .map(|c| {
                (
                    c.content.clone(),
                    c.token_count,
                    Some(c.content_hash.clone()),
                )
            })
            .collect();

        let chunks_created =
            match crate::db::chunks::replace_chunks(&self.db, &doc_id, chunk_data).await {
                Ok(n) => n,
                Err(e) => return error_json(&e.to_string()),
            };

        let chunks_embedded = match self.embed_document_chunks(&doc_id).await {
            Ok(n) => n,
            Err(e) => {
                tracing::error!(error = %e, "embedding failed, chunks stored without vectors");
                0
            }
        };

        if chunks_embedded > 0
            && let Err(e) = self.pipeline.rebuild_index().await
        {
            tracing::error!(error = %e, "vector index rebuild failed");
        }

        if let Some(parent_id_str) = &input.parent_id {
            let parent_rid = RecordId::new("document", parent_id_str.as_str());
            if let Err(e) =
                crate::db::hierarchy::set_parent(&self.db, &doc_id, Some(&parent_rid)).await
            {
                tracing::warn!(error = %e, "failed to set parent");
            }
        }

        tracing::info!(
            id = %record_key_str(&doc_id.key),
            chunks_created,
            chunks_embedded,
            "create_document complete"
        );

        let result = MutationResult {
            id: record_key_str(&doc_id.key),
            title: input.title,
            source,
            chunks_created,
            chunks_embedded,
        };
        serde_json::to_string(&result).unwrap_or_else(|e| error_json(&e.to_string()))
    }

    #[tool(
        description = "Update an existing document. When content changes, the document is re-chunked, re-embedded, and the search index is updated."
    )]
    async fn update_document(&self, Parameters(input): Parameters<UpdateDocumentInput>) -> String {
        if input.id.is_empty() || input.id.len() > 256 {
            return error_json("id must be between 1 and 256 characters");
        }

        let existing = match crate::db::documents::get_document(&self.db, &input.id).await {
            Ok(Some(doc)) => doc,
            Ok(None) => return error_json(&format!("document not found: {}", input.id)),
            Err(e) => return error_json(&e.to_string()),
        };

        let new_title = input.title.as_deref().unwrap_or(&existing.title);
        let new_content = input.content.as_deref().unwrap_or(&existing.content);
        let content_changed = new_content != existing.content;

        let mut chunks_created = 0;
        let mut chunks_embedded = 0;

        if content_changed {
            let hash = content_hash(new_content);
            let (doc_id, _) = match crate::db::documents::upsert_document(
                &self.db,
                &existing.source,
                &existing.source_id,
                new_title,
                new_content,
                &hash,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => return error_json(&e.to_string()),
            };

            let file_type = parse_content_type(input.content_type.as_deref(), new_content);
            let mut parser = tree_sitter::Parser::new();
            let chunks = crate::ingest::chunker::chunk_content(
                new_content,
                &file_type,
                DEFAULT_MAX_TOKENS,
                &mut parser,
            );

            let chunk_data: Vec<(String, usize, Option<String>)> = chunks
                .iter()
                .map(|c| {
                    (
                        c.content.clone(),
                        c.token_count,
                        Some(c.content_hash.clone()),
                    )
                })
                .collect();

            chunks_created =
                match crate::db::chunks::replace_chunks(&self.db, &doc_id, chunk_data).await {
                    Ok(n) => n,
                    Err(e) => return error_json(&e.to_string()),
                };

            chunks_embedded = match self.embed_document_chunks(&doc_id).await {
                Ok(n) => n,
                Err(e) => {
                    tracing::error!(error = %e, "embedding failed during update");
                    0
                }
            };

            if chunks_embedded > 0
                && let Err(e) = self.pipeline.rebuild_index().await
            {
                tracing::error!(error = %e, "vector index rebuild failed");
            }

            if input.custom_attributes.is_some()
                && let Err(e) = crate::db::documents::update_document_fields(
                    &self.db,
                    &doc_id,
                    None,
                    input.custom_attributes.as_ref(),
                )
                .await
            {
                tracing::warn!(error = %e, "failed to update custom_attributes");
            }
        } else {
            let needs_update = input.title.is_some() || input.custom_attributes.is_some();
            if needs_update {
                let doc_id = existing.id.clone();
                if let Err(e) = crate::db::documents::update_document_fields(
                    &self.db,
                    &doc_id,
                    input.title.as_deref(),
                    input.custom_attributes.as_ref(),
                )
                .await
                {
                    return error_json(&e.to_string());
                }
            }
        }

        tracing::info!(
            id = %input.id,
            content_changed,
            chunks_created,
            chunks_embedded,
            "update_document complete"
        );

        let result = MutationResult {
            id: input.id,
            title: new_title.to_string(),
            source: existing.source,
            chunks_created,
            chunks_embedded,
        };
        serde_json::to_string(&result).unwrap_or_else(|e| error_json(&e.to_string()))
    }

    #[tool(
        description = "Set or remove the parent of a document in the hierarchy. Omit parent_id to make the document a root."
    )]
    async fn set_document_parent(
        &self,
        Parameters(input): Parameters<SetDocumentParentInput>,
    ) -> String {
        if input.child_id.is_empty() || input.child_id.len() > 256 {
            return error_json("child_id must be between 1 and 256 characters");
        }
        let child_rid = RecordId::new("document", input.child_id.as_str());
        let parent_rid = input
            .parent_id
            .as_ref()
            .map(|p| RecordId::new("document", p.as_str()));

        tracing::info!(child = %input.child_id, parent = ?input.parent_id, "set_document_parent");
        match crate::db::hierarchy::set_parent(&self.db, &child_rid, parent_rid.as_ref()).await {
            Ok(()) => serde_json::json!({"ok": true}).to_string(),
            Err(e) => {
                tracing::error!(tool = "set_document_parent", error = %e, "failed");
                error_json(&e.to_string())
            }
        }
    }

    #[tool(
        description = "Get the parent document of the given document. Returns null if it is a root document."
    )]
    async fn get_document_parent(&self, Parameters(input): Parameters<DocumentIdInput>) -> String {
        if input.id.is_empty() || input.id.len() > 256 {
            return error_json("id must be between 1 and 256 characters");
        }
        let rid = RecordId::new("document", input.id.as_str());
        match crate::db::hierarchy::get_parent(&self.db, &rid).await {
            Ok(parent) => {
                serde_json::to_string(&parent).unwrap_or_else(|e| error_json(&e.to_string()))
            }
            Err(e) => error_json(&e.to_string()),
        }
    }

    #[tool(description = "Get all direct child documents of the given document.")]
    async fn get_document_children(
        &self,
        Parameters(input): Parameters<DocumentIdInput>,
    ) -> String {
        if input.id.is_empty() || input.id.len() > 256 {
            return error_json("id must be between 1 and 256 characters");
        }
        let rid = RecordId::new("document", input.id.as_str());
        match crate::db::hierarchy::get_children(&self.db, &rid).await {
            Ok(children) => {
                serde_json::to_string(&children).unwrap_or_else(|e| error_json(&e.to_string()))
            }
            Err(e) => error_json(&e.to_string()),
        }
    }

    #[tool(description = "Get all ancestors of a document from immediate parent up to root.")]
    async fn get_document_ancestors(
        &self,
        Parameters(input): Parameters<DocumentIdInput>,
    ) -> String {
        if input.id.is_empty() || input.id.len() > 256 {
            return error_json("id must be between 1 and 256 characters");
        }
        let rid = RecordId::new("document", input.id.as_str());
        match crate::db::hierarchy::get_ancestors(&self.db, &rid).await {
            Ok(ancestors) => {
                serde_json::to_string(&ancestors).unwrap_or_else(|e| error_json(&e.to_string()))
            }
            Err(e) => error_json(&e.to_string()),
        }
    }

    #[tool(description = "Get all descendants of a document (children, grandchildren, etc.).")]
    async fn get_document_descendants(
        &self,
        Parameters(input): Parameters<DocumentIdInput>,
    ) -> String {
        if input.id.is_empty() || input.id.len() > 256 {
            return error_json("id must be between 1 and 256 characters");
        }
        let rid = RecordId::new("document", input.id.as_str());
        match crate::db::hierarchy::get_descendants(&self.db, &rid).await {
            Ok(descendants) => {
                serde_json::to_string(&descendants).unwrap_or_else(|e| error_json(&e.to_string()))
            }
            Err(e) => error_json(&e.to_string()),
        }
    }
}

#[tool_handler]
impl ServerHandler for RagServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("wzd-rag-lightweight", "0.1.0"))
    }
}
