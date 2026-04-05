use std::collections::VecDeque;

use surrealdb::types::RecordId;

use crate::db::SurrealClient;
use crate::db::documents::DocumentSummary;

const MAX_DEPTH: usize = 100;

pub async fn set_parent(
    db: &SurrealClient,
    child_id: &RecordId,
    parent_id: Option<&RecordId>,
) -> crate::error::Result<()> {
    if let Some(pid) = parent_id {
        if child_id == pid {
            return Err(crate::error::AppError::Hierarchy(
                "document cannot be its own parent".into(),
            ));
        }

        let ancestors = get_ancestors(db, pid).await?;
        if ancestors.iter().any(|a| a.id == *child_id) {
            return Err(crate::error::AppError::Hierarchy(
                "cycle detected: parent is a descendant of child".into(),
            ));
        }

        db.query(
            "BEGIN TRANSACTION; \
             DELETE child_of WHERE in = $child; \
             RELATE $child -> child_of -> $parent; \
             COMMIT TRANSACTION;",
        )
        .bind(("child", child_id.clone()))
        .bind(("parent", pid.clone()))
        .await?;
    } else {
        db.query("DELETE child_of WHERE in = $child")
            .bind(("child", child_id.clone()))
            .await?;
    }

    Ok(())
}

pub async fn get_parent(
    db: &SurrealClient,
    document_id: &RecordId,
) -> crate::error::Result<Option<DocumentSummary>> {
    let result: Option<DocumentSummary> = db
        .query(
            "SELECT id, source, source_id, title FROM document \
             WHERE id IN (SELECT VALUE out FROM child_of WHERE in = $doc) LIMIT 1",
        )
        .bind(("doc", document_id.clone()))
        .await?
        .take(0)?;
    Ok(result)
}

pub async fn get_children(
    db: &SurrealClient,
    document_id: &RecordId,
) -> crate::error::Result<Vec<DocumentSummary>> {
    let results: Vec<DocumentSummary> = db
        .query(
            "SELECT id, source, source_id, title FROM document \
             WHERE id IN (SELECT VALUE in FROM child_of WHERE out = $doc) \
             ORDER BY title ASC",
        )
        .bind(("doc", document_id.clone()))
        .await?
        .take(0)?;
    Ok(results)
}

pub async fn get_ancestors(
    db: &SurrealClient,
    document_id: &RecordId,
) -> crate::error::Result<Vec<DocumentSummary>> {
    let mut ancestors = Vec::new();
    let mut current = document_id.clone();

    for _ in 0..MAX_DEPTH {
        match get_parent(db, &current).await? {
            Some(parent) => {
                current = parent.id.clone();
                ancestors.push(parent);
            }
            None => break,
        }
    }

    Ok(ancestors)
}

pub async fn get_descendants(
    db: &SurrealClient,
    document_id: &RecordId,
) -> crate::error::Result<Vec<DocumentSummary>> {
    let mut descendants = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back(document_id.clone());

    let mut depth = 0;
    while let Some(current) = queue.pop_front() {
        if depth >= MAX_DEPTH {
            break;
        }
        let children = get_children(db, &current).await?;
        for child in children {
            queue.push_back(child.id.clone());
            descendants.push(child);
        }
        depth += 1;
    }

    Ok(descendants)
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
    async fn set_and_get_parent() {
        let db = test_db().await;
        let (parent_id, _) = upsert_document(&db, "test", "p1", "Parent", "content", "h1")
            .await
            .unwrap();
        let (child_id, _) = upsert_document(&db, "test", "c1", "Child", "content", "h2")
            .await
            .unwrap();

        set_parent(&db, &child_id, Some(&parent_id)).await.unwrap();

        let parent = get_parent(&db, &child_id).await.unwrap();
        assert!(parent.is_some());
        assert_eq!(parent.unwrap().title, "Parent");
    }

    #[tokio::test]
    async fn root_document_has_no_parent() {
        let db = test_db().await;
        let (doc_id, _) = upsert_document(&db, "test", "r1", "Root", "content", "h1")
            .await
            .unwrap();
        let parent = get_parent(&db, &doc_id).await.unwrap();
        assert!(parent.is_none());
    }

    #[tokio::test]
    async fn get_children_returns_direct_children() {
        let db = test_db().await;
        let (parent_id, _) = upsert_document(&db, "test", "p1", "Parent", "c", "h1")
            .await
            .unwrap();
        let (c1, _) = upsert_document(&db, "test", "c1", "Alpha", "c", "h2")
            .await
            .unwrap();
        let (c2, _) = upsert_document(&db, "test", "c2", "Beta", "c", "h3")
            .await
            .unwrap();

        set_parent(&db, &c1, Some(&parent_id)).await.unwrap();
        set_parent(&db, &c2, Some(&parent_id)).await.unwrap();

        let children = get_children(&db, &parent_id).await.unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].title, "Alpha");
        assert_eq!(children[1].title, "Beta");
    }

    #[tokio::test]
    async fn remove_parent_makes_root() {
        let db = test_db().await;
        let (parent_id, _) = upsert_document(&db, "test", "p1", "Parent", "c", "h1")
            .await
            .unwrap();
        let (child_id, _) = upsert_document(&db, "test", "c1", "Child", "c", "h2")
            .await
            .unwrap();

        set_parent(&db, &child_id, Some(&parent_id)).await.unwrap();
        set_parent(&db, &child_id, None).await.unwrap();

        let parent = get_parent(&db, &child_id).await.unwrap();
        assert!(parent.is_none());
    }

    #[tokio::test]
    async fn self_parent_is_rejected() {
        let db = test_db().await;
        let (doc_id, _) = upsert_document(&db, "test", "d1", "Doc", "c", "h1")
            .await
            .unwrap();
        let result = set_parent(&db, &doc_id, Some(&doc_id)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cycle_detection_prevents_circular_hierarchy() {
        let db = test_db().await;
        let (a, _) = upsert_document(&db, "test", "a", "A", "c", "h1").await.unwrap();
        let (b, _) = upsert_document(&db, "test", "b", "B", "c", "h2").await.unwrap();
        let (c, _) = upsert_document(&db, "test", "c", "C", "c", "h3").await.unwrap();

        set_parent(&db, &b, Some(&a)).await.unwrap();
        set_parent(&db, &c, Some(&b)).await.unwrap();

        let result = set_parent(&db, &a, Some(&c)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_ancestors_returns_path_to_root() {
        let db = test_db().await;
        let (root, _) = upsert_document(&db, "test", "r", "Root", "c", "h1").await.unwrap();
        let (mid, _) = upsert_document(&db, "test", "m", "Mid", "c", "h2").await.unwrap();
        let (leaf, _) = upsert_document(&db, "test", "l", "Leaf", "c", "h3").await.unwrap();

        set_parent(&db, &mid, Some(&root)).await.unwrap();
        set_parent(&db, &leaf, Some(&mid)).await.unwrap();

        let ancestors = get_ancestors(&db, &leaf).await.unwrap();
        assert_eq!(ancestors.len(), 2);
        assert_eq!(ancestors[0].title, "Mid");
        assert_eq!(ancestors[1].title, "Root");
    }

    #[tokio::test]
    async fn get_descendants_returns_full_subtree() {
        let db = test_db().await;
        let (root, _) = upsert_document(&db, "test", "r", "Root", "c", "h1").await.unwrap();
        let (c1, _) = upsert_document(&db, "test", "c1", "C1", "c", "h2").await.unwrap();
        let (c2, _) = upsert_document(&db, "test", "c2", "C2", "c", "h3").await.unwrap();
        let (gc1, _) = upsert_document(&db, "test", "gc1", "GC1", "c", "h4").await.unwrap();

        set_parent(&db, &c1, Some(&root)).await.unwrap();
        set_parent(&db, &c2, Some(&root)).await.unwrap();
        set_parent(&db, &gc1, Some(&c1)).await.unwrap();

        let desc = get_descendants(&db, &root).await.unwrap();
        assert_eq!(desc.len(), 3);
    }

    #[tokio::test]
    async fn replacing_parent_updates_hierarchy() {
        let db = test_db().await;
        let (p1, _) = upsert_document(&db, "test", "p1", "P1", "c", "h1").await.unwrap();
        let (p2, _) = upsert_document(&db, "test", "p2", "P2", "c", "h2").await.unwrap();
        let (child, _) = upsert_document(&db, "test", "ch", "Child", "c", "h3").await.unwrap();

        set_parent(&db, &child, Some(&p1)).await.unwrap();
        set_parent(&db, &child, Some(&p2)).await.unwrap();

        let parent = get_parent(&db, &child).await.unwrap();
        assert_eq!(parent.unwrap().title, "P2");

        let p1_children = get_children(&db, &p1).await.unwrap();
        assert!(p1_children.is_empty());
    }
}
