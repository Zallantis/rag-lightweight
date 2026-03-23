use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
pub struct ContextSearchInput {
    #[schemars(description = "Search query text")]
    pub query: String,
    #[schemars(description = "Max results to return (default 5, max 50)")]
    pub limit: Option<usize>,
    #[schemars(description = "Filter by document source label")]
    pub source: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetDocumentInput {
    #[schemars(description = "Document record ID (without 'document:' prefix)")]
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListDocumentsInput {
    #[schemars(description = "Filter by document source label")]
    pub source: Option<String>,
    #[schemars(description = "Max documents to return (default 20, max 200)")]
    pub limit: Option<usize>,
    #[schemars(description = "Offset for pagination")]
    pub offset: Option<usize>,
}

#[derive(Serialize)]
pub struct StatsOutput {
    pub documents: usize,
    pub chunks: usize,
    pub embedded_chunks: usize,
    pub pending_chunks: usize,
    pub documents_by_source: Vec<crate::db::documents::SourceCount>,
    pub embedding_model: String,
    pub embedding_dimension: usize,
}
