use crate::db::SurrealClient;
use surrealdb::types::{RecordId, SurrealValue};

#[derive(Debug, Clone)]
pub struct VectorSearchResult {
    pub id: RecordId,
    pub document: RecordId,
    pub doc_source: Option<String>,
    pub doc_title: Option<String>,
    pub content: String,
    pub score: f64,
}

#[derive(Debug, Clone, SurrealValue)]
pub struct FtsSearchResult {
    pub id: RecordId,
    pub source: Option<String>,
    pub title: Option<String>,
    pub content: String,
    pub score: f64,
}

pub async fn fulltext_search(
    db: &SurrealClient,
    query: &str,
    limit: usize,
    source: Option<&str>,
) -> crate::error::Result<Vec<FtsSearchResult>> {
    let results: Vec<FtsSearchResult> = if let Some(source) = source {
        db.query("SELECT id, source, title, content, search::score(0) AS score FROM document WHERE source = $source AND content @0@ $query ORDER BY score DESC LIMIT $limit")
            .bind(("query", query.to_string()))
            .bind(("source", source.to_string()))
            .bind(("limit", limit))
            .await?
            .take(0)?
    } else {
        db.query("SELECT id, source, title, content, search::score(0) AS score FROM document WHERE content @0@ $query ORDER BY score DESC LIMIT $limit")
            .bind(("query", query.to_string()))
            .bind(("limit", limit))
            .await?
            .take(0)?
    };
    Ok(results)
}
