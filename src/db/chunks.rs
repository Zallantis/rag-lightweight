use serde::Serialize;
use surrealdb::types::{RecordId, SurrealValue};

use crate::db::SurrealClient;

pub async fn replace_chunks(
    db: &SurrealClient,
    document_id: &RecordId,
    chunks: Vec<(String, usize, Option<String>)>,
) -> crate::error::Result<usize> {
    let count = chunks.len();

    let mut stmts = vec![
        "BEGIN TRANSACTION".to_string(),
        "DELETE chunk WHERE document = $doc".to_string(),
    ];
    for (i, _) in chunks.iter().enumerate() {
        stmts.push(format!(
            "CREATE chunk SET document = $doc, content = $content_{i}, position = $pos_{i}, token_count = $tokens_{i}, content_hash = $hash_{i}"
        ));
    }
    stmts.push("COMMIT TRANSACTION".to_string());

    let sql = stmts.join("; ");
    let mut q = db.query(sql).bind(("doc", document_id.clone()));
    for (i, (content, token_count, content_hash)) in chunks.into_iter().enumerate() {
        q = q
            .bind((format!("content_{i}"), content))
            .bind((format!("pos_{i}"), i as i64))
            .bind((format!("tokens_{i}"), token_count as i64))
            .bind((format!("hash_{i}"), content_hash));
    }
    q.await?;

    Ok(count)
}

#[derive(Debug, SurrealValue)]
pub struct PendingChunk {
    pub id: RecordId,
    pub content: String,
}

pub async fn get_pending_chunks(
    db: &SurrealClient,
    limit: usize,
) -> crate::error::Result<Vec<PendingChunk>> {
    let chunks: Vec<PendingChunk> = db
        .query("SELECT id, content FROM chunk WHERE vector IS NONE LIMIT $limit")
        .bind(("limit", limit))
        .await?
        .take(0)?;
    Ok(chunks)
}

#[derive(SurrealValue)]
struct VectorMerge {
    vector: Vec<f32>,
    embedded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(SurrealValue)]
struct UpdateResult {}

pub async fn bulk_update_chunk_vectors(
    db: &SurrealClient,
    updates: Vec<(RecordId, Vec<f32>)>,
) -> crate::error::Result<()> {
    for (id, vec) in updates {
        let _: Option<UpdateResult> = db
            .update(id)
            .merge(VectorMerge {
                vector: vec,
                embedded_at: chrono::Utc::now(),
            })
            .await?;
    }
    Ok(())
}

pub async fn clear_all_vectors(db: &SurrealClient) -> crate::error::Result<()> {
    db.query("UPDATE chunk SET vector = NONE, embedded_at = NONE")
        .await?;
    Ok(())
}

pub async fn count_chunks(db: &SurrealClient) -> crate::error::Result<ChunkCounts> {
    let total: Option<CountResult> = db
        .query("SELECT count() AS count FROM chunk GROUP ALL")
        .await?
        .take(0)?;
    let embedded: Option<CountResult> = db
        .query("SELECT count() AS count FROM chunk WHERE vector IS NOT NONE GROUP ALL")
        .await?
        .take(0)?;

    let total = total.map(|r| r.count).unwrap_or(0);
    let embedded = embedded.map(|r| r.count).unwrap_or(0);

    Ok(ChunkCounts {
        total,
        embedded,
        pending: total - embedded,
    })
}

#[derive(Debug, SurrealValue)]
struct CountResult {
    count: usize,
}

#[derive(Debug, Serialize, SurrealValue)]
pub struct ChunkCounts {
    pub total: usize,
    pub embedded: usize,
    pub pending: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::documents::upsert_document;
    use tempfile::tempdir;

    async fn test_db() -> db::SurrealClient {
        let dir = tempdir().unwrap();
        let db = db::init(dir.path(), 3).await.unwrap();
        std::mem::forget(dir);
        db
    }

    #[tokio::test]
    async fn replace_chunks_stores_all_chunks() {
        let db = test_db().await;
        let (doc_id, _) = upsert_document(&db, "local", "f1", "T", "C", "h")
            .await
            .unwrap();

        let chunks = vec![
            ("chunk one".to_string(), 2usize, Some("hash1".to_string())),
            ("chunk two".to_string(), 3usize, Some("hash2".to_string())),
        ];
        let count = replace_chunks(&db, &doc_id, chunks).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn get_pending_chunks_returns_all_unembedded() {
        let db = test_db().await;
        let (doc_id, _) = upsert_document(&db, "local", "f1", "T", "C", "h")
            .await
            .unwrap();
        replace_chunks(
            &db,
            &doc_id,
            vec![
                ("chunk one".to_string(), 2, Some("h1".to_string())),
                ("chunk two".to_string(), 3, Some("h2".to_string())),
            ],
        )
        .await
        .unwrap();

        let pending = get_pending_chunks(&db, 100).await.unwrap();
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn bulk_update_removes_chunks_from_pending() {
        let db = test_db().await;
        let (doc_id, _) = upsert_document(&db, "local", "f1", "T", "C", "h")
            .await
            .unwrap();
        replace_chunks(
            &db,
            &doc_id,
            vec![
                ("chunk one".to_string(), 2, Some("h1".to_string())),
                ("chunk two".to_string(), 3, Some("h2".to_string())),
            ],
        )
        .await
        .unwrap();

        let pending_before = get_pending_chunks(&db, 100).await.unwrap();
        assert_eq!(pending_before.len(), 2);

        let updates: Vec<(RecordId, Vec<f32>)> = pending_before
            .into_iter()
            .map(|c| (c.id, vec![0.1f32, 0.2, 0.3]))
            .collect();
        bulk_update_chunk_vectors(&db, updates).await.unwrap();

        let pending_after = get_pending_chunks(&db, 100).await.unwrap();
        assert_eq!(pending_after.len(), 0);
    }

    #[tokio::test]
    async fn count_chunks_reflects_embedded_and_pending() {
        let db = test_db().await;
        let (doc_id, _) = upsert_document(&db, "local", "f1", "T", "C", "h")
            .await
            .unwrap();
        replace_chunks(
            &db,
            &doc_id,
            vec![
                ("chunk one".to_string(), 2, None),
                ("chunk two".to_string(), 3, None),
            ],
        )
        .await
        .unwrap();

        let pending = get_pending_chunks(&db, 100).await.unwrap();
        let first_id = pending[0].id.clone();
        bulk_update_chunk_vectors(&db, vec![(first_id, vec![1.0f32, 0.0, 0.0])])
            .await
            .unwrap();

        let counts = count_chunks(&db).await.unwrap();
        assert_eq!(counts.total, 2);
        assert_eq!(counts.embedded, 1);
        assert_eq!(counts.pending, 1);
    }

    #[tokio::test]
    async fn replace_chunks_deletes_previous_chunks() {
        let db = test_db().await;
        let (doc_id, _) = upsert_document(&db, "local", "f1", "T", "C", "h")
            .await
            .unwrap();
        replace_chunks(
            &db,
            &doc_id,
            vec![
                ("old chunk".to_string(), 2, None),
                ("old chunk 2".to_string(), 3, None),
            ],
        )
        .await
        .unwrap();

        replace_chunks(&db, &doc_id, vec![("new chunk".to_string(), 4, None)])
            .await
            .unwrap();

        let counts = count_chunks(&db).await.unwrap();
        assert_eq!(counts.total, 1);
    }

    #[tokio::test]
    async fn get_pending_chunks_respects_limit() {
        let db = test_db().await;
        let (doc_id, _) = upsert_document(&db, "local", "f1", "T", "C", "h")
            .await
            .unwrap();
        replace_chunks(
            &db,
            &doc_id,
            vec![
                ("c1".to_string(), 1, None),
                ("c2".to_string(), 1, None),
                ("c3".to_string(), 1, None),
            ],
        )
        .await
        .unwrap();

        let pending = get_pending_chunks(&db, 2).await.unwrap();
        assert_eq!(pending.len(), 2);
    }
}
