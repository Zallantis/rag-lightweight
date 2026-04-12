use crate::db::SurrealClient;
use std::collections::HashMap;
use surrealdb::types::{RecordId, RecordIdKey, SurrealValue};

const MAX_CHUNKS_PER_DOC: usize = 3;

struct Entry {
    chunk_id: RecordId,
    doc_key: String,
    content: String,
    norm_vector: Vec<f32>,
}

pub struct DocInfo {
    pub source: String,
    pub title: String,
}

pub struct VectorIndex {
    entries: Vec<Entry>,
    docs: HashMap<String, DocInfo>,
}

pub struct VectorHit {
    pub chunk_id: RecordId,
    pub document_id: RecordId,
    pub doc_source: Option<String>,
    pub doc_title: Option<String>,
    pub content: String,
    pub score: f32,
}

impl VectorIndex {
    pub async fn build(db: &SurrealClient) -> crate::error::Result<Self> {
        let doc_rows: Vec<DocRow> = db
            .query("SELECT id, source, title FROM document")
            .await?
            .take(0)?;

        let docs: HashMap<String, DocInfo> = doc_rows
            .into_iter()
            .map(|d| {
                let key = record_key_str(&d.id.key);
                (
                    key,
                    DocInfo {
                        source: d.source,
                        title: d.title,
                    },
                )
            })
            .collect();

        let rows: Vec<IndexRow> = db
            .query("SELECT id, document, content, vector FROM chunk WHERE vector IS NOT NONE")
            .await?
            .take(0)?;

        let entries: Vec<Entry> = rows
            .into_iter()
            .filter_map(|r| {
                let v = r.vector?;
                if v.is_empty() || r.content.trim().len() < 20 {
                    return None;
                }
                let norm = norm_l2(&v);
                if norm < 1e-10 {
                    return None;
                }
                let inv = 1.0 / norm;
                let norm_vector: Vec<f32> = v.iter().map(|x| x * inv).collect();
                let doc_key = record_key_str(&r.document.key);
                Some(Entry {
                    chunk_id: r.id,
                    doc_key,
                    content: r.content,
                    norm_vector,
                })
            })
            .collect();

        tracing::info!(
            vectors = entries.len(),
            documents = docs.len(),
            "vector index built"
        );
        Ok(Self { entries, docs })
    }

    pub fn search(
        &self,
        query_vector: &[f32],
        limit: usize,
        source: Option<&str>,
    ) -> Vec<VectorHit> {
        if self.entries.is_empty() {
            return vec![];
        }

        let query_norm = norm_l2(query_vector);
        if query_norm < 1e-10 {
            return vec![];
        }
        let inv = 1.0 / query_norm;
        let q: Vec<f32> = query_vector.iter().map(|x| x * inv).collect();

        let iter = self.entries.iter().enumerate();

        let mut scored: Vec<(usize, f32)> = if let Some(src) = source {
            iter.filter(|(_, e)| {
                self.docs
                    .get(&e.doc_key)
                    .map(|d| d.source == src)
                    .unwrap_or(false)
            })
            .map(|(i, e)| (i, dot(&q, &e.norm_vector)))
            .collect()
        } else {
            iter.map(|(i, e)| (i, dot(&q, &e.norm_vector))).collect()
        };

        scored.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut doc_counts: HashMap<&str, usize> = HashMap::new();
        let mut filtered = Vec::with_capacity(limit);
        for (i, score) in scored {
            let doc_key = &self.entries[i].doc_key;
            let count = doc_counts.entry(doc_key).or_insert(0);
            if *count >= MAX_CHUNKS_PER_DOC {
                continue;
            }
            *count += 1;
            filtered.push((i, score));
            if filtered.len() >= limit {
                break;
            }
        }

        filtered
            .into_iter()
            .map(|(i, score)| {
                let e = &self.entries[i];
                let doc = self.docs.get(&e.doc_key);
                VectorHit {
                    chunk_id: e.chunk_id.clone(),
                    document_id: RecordId::new("document", e.doc_key.as_str()),
                    doc_source: doc.map(|d| d.source.clone()),
                    doc_title: doc.map(|d| d.title.clone()),
                    content: e.content.clone(),
                    score,
                }
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn norm_l2(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn record_key_str(key: &RecordIdKey) -> String {
    match key {
        RecordIdKey::String(s) => s.clone(),
        RecordIdKey::Number(n) => n.to_string(),
        RecordIdKey::Uuid(u) => u.to_string(),
        _ => format!("{:?}", key),
    }
}

#[derive(SurrealValue)]
struct DocRow {
    id: RecordId,
    source: String,
    title: String,
}

#[derive(SurrealValue)]
struct IndexRow {
    id: RecordId,
    document: RecordId,
    content: String,
    vector: Option<Vec<f32>>,
}
