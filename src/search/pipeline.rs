use crate::config::SearchConfig;
use crate::db::SurrealClient;
use crate::db::search as db_search;
use crate::embed::service::EmbeddingService;
use crate::search::merge;
use crate::search::vector_index::VectorIndex;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Serialize, Clone)]
pub struct SearchResult {
    pub document_id: String,
    pub source: Option<String>,
    pub title: Option<String>,
    pub content: String,
    pub score: f64,
}

pub struct SearchPipeline {
    db: SurrealClient,
    embedding_service: Arc<dyn EmbeddingService>,
    search_config: SearchConfig,
    vector_index: RwLock<VectorIndex>,
}

impl SearchPipeline {
    pub async fn new(
        db: SurrealClient,
        embedding_service: Arc<dyn EmbeddingService>,
        search_config: SearchConfig,
    ) -> crate::error::Result<Self> {
        let vector_index = VectorIndex::build(&db).await?;
        Ok(Self {
            db,
            embedding_service,
            search_config,
            vector_index: RwLock::new(vector_index),
        })
    }

    pub async fn search(
        &self,
        query: &str,
        limit: Option<usize>,
        source_filter: Option<&str>,
    ) -> crate::error::Result<Vec<SearchResult>> {
        let top_k = limit.unwrap_or(self.search_config.top_k);
        let retrieve_limit = self.search_config.retrieve_limit;

        tracing::debug!(query, top_k, retrieve_limit, "embedding query");
        let query_vectors = self
            .embedding_service
            .embed_with_role(
                vec![query.to_string()],
                crate::embed::service::EmbedRole::Query,
            )
            .await?;
        let query_vector = query_vectors
            .into_iter()
            .next()
            .ok_or_else(|| crate::error::AppError::Search("No embedding returned".into()))?;

        let index = self.vector_index.read().await;
        let vec_hits = index.search(&query_vector, retrieve_limit, source_filter);
        drop(index);

        let vec_results: Vec<db_search::VectorSearchResult> = vec_hits
            .into_iter()
            .map(|h| db_search::VectorSearchResult {
                id: h.chunk_id,
                document: h.document_id,
                doc_source: h.doc_source,
                doc_title: h.doc_title,
                content: h.content,
                score: h.score as f64,
            })
            .collect();

        let fts_results =
            db_search::fulltext_search(&self.db, query, retrieve_limit, source_filter).await?;

        tracing::debug!(
            vector_hits = vec_results.len(),
            fts_hits = fts_results.len(),
            "search results retrieved"
        );

        let mut merged = merge::rrf_merge(vec_results, fts_results);
        merged.truncate(top_k);

        for result in &mut merged {
            if let Ok(Some(doc)) =
                crate::db::documents::get_document(&self.db, &result.document_id).await
            {
                result.content = doc.content;
                if result.source.is_none() {
                    result.source = Some(doc.source);
                }
                if result.title.is_none() {
                    result.title = Some(doc.title);
                }
            }
        }

        tracing::debug!(final_count = merged.len(), "search merged and truncated");
        Ok(merged)
    }

    pub async fn rebuild_index(&self) -> crate::error::Result<()> {
        let new_index = VectorIndex::build(&self.db).await?;
        let mut index = self.vector_index.write().await;
        *index = new_index;
        tracing::info!("vector index rebuilt");
        Ok(())
    }

    pub fn embedding_service(&self) -> &Arc<dyn EmbeddingService> {
        &self.embedding_service
    }
}
