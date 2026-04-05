use serde::Serialize;
use surrealdb::types::{RecordId, RecordIdKey, SurrealValue};

use crate::db::SurrealClient;

fn serialize_record_id<S>(id: &RecordId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let key_str = match &id.key {
        RecordIdKey::String(s) => s.clone(),
        RecordIdKey::Number(n) => n.to_string(),
        RecordIdKey::Uuid(u) => u.to_string(),
        other => format!("{:?}", other),
    };
    serializer.serialize_str(&key_str)
}

#[derive(Debug, Clone, Serialize, SurrealValue)]
pub struct Document {
    #[serde(serialize_with = "serialize_record_id")]
    pub id: RecordId,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub content: String,
    pub content_hash: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub custom_attributes: Option<serde_json::Value>,
}

pub async fn upsert_document(
    db: &SurrealClient,
    source: &str,
    source_id: &str,
    title: &str,
    content: &str,
    content_hash: &str,
) -> crate::error::Result<(RecordId, bool)> {
    #[derive(Debug, SurrealValue)]
    struct DocumentLookup {
        id: RecordId,
        content_hash: Option<String>,
    }

    let existing: Option<DocumentLookup> = db
        .query("SELECT id, content_hash FROM document WHERE source = $source AND source_id = $source_id LIMIT 1")
        .bind(("source", source.to_string()))
        .bind(("source_id", source_id.to_string()))
        .await?
        .take(0)?;

    if let Some(ref doc) = existing {
        if doc.content_hash.as_deref() == Some(content_hash) {
            return Ok((doc.id.clone(), false));
        }
    }

    #[derive(Debug, SurrealValue)]
    struct UpsertResult {
        id: RecordId,
    }

    let doc: Option<UpsertResult> = db
        .query(
            "UPSERT document SET source = $source, \
                 source_id = $source_id, \
                 title = $title, \
                 content = $content, \
                 content_hash = $content_hash, \
                 updated_at = time::now() \
             WHERE source = $source AND source_id = $source_id",
        )
        .bind(("source", source.to_string()))
        .bind(("source_id", source_id.to_string()))
        .bind(("title", title.to_string()))
        .bind(("content", content.to_string()))
        .bind(("content_hash", content_hash.to_string()))
        .await?
        .take(0)?;

    let doc = doc.ok_or_else(|| crate::error::AppError::Ingest(
        format!("UPSERT document returned no record for source_id={source_id}")
    ))?;
    Ok((doc.id, true))
}

pub async fn get_document(db: &SurrealClient, id: &str) -> crate::error::Result<Option<Document>> {
    let doc: Option<Document> = db
        .query("SELECT id, source, source_id, title, content, content_hash, metadata, custom_attributes FROM $id")
        .bind(("id", RecordId::new("document", id)))
        .await?
        .take(0)?;
    Ok(doc)
}

#[derive(Debug, Clone, Serialize, SurrealValue)]
pub struct DocumentSummary {
    #[serde(serialize_with = "serialize_record_id")]
    pub id: RecordId,
    pub source: String,
    pub source_id: String,
    pub title: String,
}

pub async fn list_documents(
    db: &SurrealClient,
    source: Option<&str>,
    limit: usize,
    offset: usize,
    filter: Option<&crate::db::filter::FilterResult>,
) -> crate::error::Result<Vec<DocumentSummary>> {
    let mut where_parts = Vec::new();
    if source.is_some() {
        where_parts.push("source = $source".to_string());
    }
    if let Some(f) = filter {
        where_parts.push(f.where_clause.clone());
    }

    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_parts.join(" AND "))
    };

    let sql = format!(
        "SELECT id, source, source_id, title, created_at FROM document{where_clause} ORDER BY created_at DESC LIMIT $limit START $offset"
    );

    let mut q = db.query(sql).bind(("limit", limit)).bind(("offset", offset));
    if let Some(source) = source {
        q = q.bind(("source", source.to_string()));
    }
    if let Some(f) = filter {
        for (name, value) in &f.bindings {
            q = q.bind((name.clone(), value.clone()));
        }
    }

    let docs: Vec<DocumentSummary> = q.await?.take(0)?;
    Ok(docs)
}

pub async fn count_documents(db: &SurrealClient) -> crate::error::Result<usize> {
    let result: Option<CountResult> = db
        .query("SELECT count() AS count FROM document GROUP ALL")
        .await?
        .take(0)?;
    Ok(result.map(|r| r.count).unwrap_or(0))
}

#[derive(Debug, SurrealValue)]
struct CountResult {
    count: usize,
}

pub async fn update_document_fields(
    db: &SurrealClient,
    doc_id: &RecordId,
    title: Option<&str>,
    custom_attributes: Option<&serde_json::Value>,
) -> crate::error::Result<()> {
    let mut parts = vec!["updated_at = time::now()".to_string()];
    let mut idx = 0usize;

    if title.is_some() {
        parts.push(format!("title = $p{idx}"));
        idx += 1;
    }
    if custom_attributes.is_some() {
        parts.push(format!("custom_attributes = $p{idx}"));
    }

    let sql = format!("UPDATE $id SET {}", parts.join(", "));
    let mut q = db.query(sql).bind(("id", doc_id.clone()));

    idx = 0;
    if let Some(t) = title {
        q = q.bind((format!("p{idx}"), t.to_string()));
        idx += 1;
    }
    if let Some(ca) = custom_attributes {
        q = q.bind((format!("p{idx}"), ca.clone()));
    }

    q.await?;
    Ok(())
}

pub async fn documents_by_source(db: &SurrealClient) -> crate::error::Result<Vec<SourceCount>> {
    let results: Vec<SourceCount> = db
        .query("SELECT source, count() AS count FROM document GROUP BY source")
        .await?
        .take(0)?;
    Ok(results)
}

#[derive(Debug, Serialize, SurrealValue)]
pub struct SourceCount {
    pub source: String,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn upsert_creates_new_document() {
        let dir = tempdir().unwrap();
        let db = crate::db::init(dir.path(), 384).await.unwrap();
        let (id, created) = upsert_document(&db, "local", "file1", "Title", "Content", "hash1")
            .await
            .unwrap();
        assert!(created);
        assert!(format!("{:?}", id).contains("document"));
    }

    #[tokio::test]
    async fn upsert_skips_when_hash_unchanged() {
        let dir = tempdir().unwrap();
        let db = crate::db::init(dir.path(), 384).await.unwrap();
        let (id1, _) = upsert_document(&db, "local", "file1", "Title", "Content", "hash1")
            .await
            .unwrap();
        let (id2, created) = upsert_document(&db, "local", "file1", "Title", "Content", "hash1")
            .await
            .unwrap();
        assert!(!created);
        assert_eq!(format!("{:?}", id1), format!("{:?}", id2));
    }

    #[tokio::test]
    async fn upsert_updates_when_hash_changes() {
        let dir = tempdir().unwrap();
        let db = crate::db::init(dir.path(), 384).await.unwrap();
        let (id1, _) = upsert_document(&db, "local", "file1", "Title", "Old", "hash1")
            .await
            .unwrap();
        let (id2, created) = upsert_document(&db, "local", "file1", "Title", "New", "hash2")
            .await
            .unwrap();
        assert!(created);
        assert_eq!(format!("{:?}", id1), format!("{:?}", id2));
    }

    #[tokio::test]
    async fn upsert_different_source_id_creates_separate_documents() {
        let dir = tempdir().unwrap();
        let db = crate::db::init(dir.path(), 384).await.unwrap();
        let (id1, _) = upsert_document(&db, "local", "file1", "T1", "C1", "h1")
            .await
            .unwrap();
        let (id2, _) = upsert_document(&db, "local", "file2", "T2", "C2", "h2")
            .await
            .unwrap();
        assert_ne!(format!("{:?}", id1), format!("{:?}", id2));
    }

    #[tokio::test]
    async fn get_document_returns_none_for_missing_id() {
        let dir = tempdir().unwrap();
        let db = crate::db::init(dir.path(), 384).await.unwrap();
        let result = get_document(&db, "nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_documents_returns_all_when_no_source_filter() {
        let dir = tempdir().unwrap();
        let db = crate::db::init(dir.path(), 384).await.unwrap();
        upsert_document(&db, "source_a", "f1", "T1", "C1", "h1").await.unwrap();
        upsert_document(&db, "source_b", "f2", "T2", "C2", "h2").await.unwrap();
        let docs = list_documents(&db, None, 10, 0, None).await.unwrap();
        assert_eq!(docs.len(), 2);
    }

    #[tokio::test]
    async fn list_documents_filters_by_source() {
        let dir = tempdir().unwrap();
        let db = crate::db::init(dir.path(), 384).await.unwrap();
        upsert_document(&db, "source_a", "f1", "T1", "C1", "h1").await.unwrap();
        upsert_document(&db, "source_b", "f2", "T2", "C2", "h2").await.unwrap();
        let docs = list_documents(&db, Some("source_a"), 10, 0, None).await.unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].source, "source_a");
    }
}
