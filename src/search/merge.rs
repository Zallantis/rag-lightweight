use std::collections::HashMap;
use surrealdb::types::RecordIdKey;
use crate::db::search::{VectorSearchResult, FtsSearchResult};
use crate::search::pipeline::SearchResult;

const RRF_K: f64 = 60.0;

fn record_key_to_string(key: &RecordIdKey) -> String {
    match key {
        RecordIdKey::String(s) => s.clone(),
        RecordIdKey::Number(n) => n.to_string(),
        RecordIdKey::Uuid(u) => u.to_string(),
        _ => format!("{:?}", key),
    }
}

pub fn rrf_merge(
    vector_results: Vec<VectorSearchResult>,
    fts_results: Vec<FtsSearchResult>,
) -> Vec<SearchResult> {
    let mut scores: HashMap<String, MergeEntry> = HashMap::new();

    let mut doc_vector_scores: HashMap<String, VecDocEntry> = HashMap::new();
    for result in &vector_results {
        let doc_id = record_key_to_string(&result.document.key);
        let entry = doc_vector_scores.entry(doc_id.clone()).or_insert(VecDocEntry {
            score: 0.0,
            content: String::new(),
            source: result.doc_source.clone(),
            title: result.doc_title.clone(),
        });
        if result.score > entry.score {
            entry.score = result.score;
            entry.content = result.content.clone();
        }
    }

    let mut vec_ranked: Vec<_> = doc_vector_scores.into_iter().collect();
    vec_ranked.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal));

    for (rank, (doc_id, vec_entry)) in vec_ranked.into_iter().enumerate() {
        let rrf_score = 1.0 / (RRF_K + rank as f64 + 1.0);
        let entry = scores.entry(doc_id.clone()).or_insert_with(|| MergeEntry {
            document_id: doc_id,
            source: vec_entry.source.clone(),
            title: vec_entry.title.clone(),
            content: vec_entry.content,
            rrf_score: 0.0,
        });
        entry.rrf_score += rrf_score;
    }

    for (rank, result) in fts_results.into_iter().enumerate() {
        let doc_id = record_key_to_string(&result.id.key);
        let rrf_score = 1.0 / (RRF_K + rank as f64 + 1.0);
        let entry = scores.entry(doc_id.clone()).or_insert_with(|| MergeEntry {
            document_id: doc_id,
            source: result.source.clone(),
            title: result.title.clone(),
            content: result.content.clone(),
            rrf_score: 0.0,
        });
        entry.rrf_score += rrf_score;
        if entry.source.is_none() {
            entry.source = result.source;
        }
        if entry.title.is_none() {
            entry.title = result.title;
        }
    }

    let mut results: Vec<SearchResult> = scores
        .into_values()
        .map(|e| SearchResult {
            document_id: e.document_id,
            source: e.source,
            title: e.title,
            content: e.content,
            score: e.rrf_score,
        })
        .collect();

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

struct MergeEntry {
    document_id: String,
    source: Option<String>,
    title: Option<String>,
    content: String,
    rrf_score: f64,
}

struct VecDocEntry {
    score: f64,
    content: String,
    source: Option<String>,
    title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use surrealdb::types::RecordId;

    fn make_vec_result(doc_key: &str, chunk_key: &str, score: f64, content: &str) -> VectorSearchResult {
        VectorSearchResult {
            id: RecordId::new("chunk", chunk_key),
            document: RecordId::new("document", doc_key),
            doc_source: None,
            doc_title: None,
            content: content.to_string(),
            score,
        }
    }

    fn make_fts_result(doc_key: &str, score: f64, content: &str) -> FtsSearchResult {
        FtsSearchResult {
            id: RecordId::new("document", doc_key),
            source: Some("local".to_string()),
            title: Some("Test Doc".to_string()),
            content: content.to_string(),
            score,
        }
    }

    #[test]
    fn empty_inputs_return_empty() {
        let result = rrf_merge(vec![], vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn vector_only_results_are_ranked_by_score() {
        let results = rrf_merge(
            vec![
                make_vec_result("doc_b", "chunk_2", 0.7, "b content"),
                make_vec_result("doc_a", "chunk_1", 0.9, "a content"),
            ],
            vec![],
        );
        assert_eq!(results.len(), 2);
        assert!(results[0].document_id.contains("doc_a"));
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn fts_only_results_are_returned() {
        let results = rrf_merge(
            vec![],
            vec![
                make_fts_result("doc_a", 1.5, "content a"),
                make_fts_result("doc_b", 0.5, "content b"),
            ],
        );
        assert_eq!(results.len(), 2);
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn document_appearing_in_both_gets_merged_score() {
        let results = rrf_merge(
            vec![
                make_vec_result("doc_a", "chunk_1", 0.95, "content a"),
                make_vec_result("doc_b", "chunk_2", 0.80, "content b"),
            ],
            vec![
                make_fts_result("doc_a", 2.0, "content a"),
            ],
        );
        assert_eq!(results.len(), 2, "doc_a must not appear twice");
        assert!(results[0].document_id.contains("doc_a"),
            "doc_a should rank first due to merged score");
        let doc_a_score = results.iter().find(|r| r.document_id.contains("doc_a")).unwrap().score;
        let doc_b_score = results.iter().find(|r| r.document_id.contains("doc_b")).unwrap().score;
        assert!(doc_a_score > doc_b_score);
    }

    #[test]
    fn rrf_scores_use_k60_constant() {
        let results = rrf_merge(
            vec![make_vec_result("doc_a", "chunk_1", 1.0, "content")],
            vec![],
        );
        assert_eq!(results.len(), 1);
        let expected = 1.0 / (60.0 + 0.0 + 1.0);
        assert!((results[0].score - expected).abs() < 1e-10);
    }

    #[test]
    fn multiple_chunks_per_document_uses_best_score() {
        let results = rrf_merge(
            vec![
                make_vec_result("doc_a", "chunk_1", 0.5, "lower chunk"),
                make_vec_result("doc_a", "chunk_2", 0.95, "higher chunk"),
            ],
            vec![],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("higher chunk"));
    }

    #[test]
    fn results_sorted_descending_by_score() {
        let results = rrf_merge(
            vec![
                make_vec_result("doc_a", "c1", 0.9, "a"),
                make_vec_result("doc_b", "c2", 0.8, "b"),
                make_vec_result("doc_c", "c3", 0.7, "c"),
            ],
            vec![],
        );
        for i in 0..results.len() - 1 {
            assert!(results[i].score >= results[i + 1].score,
                "Results not sorted: index {i} score {} < index {} score {}",
                results[i].score, i + 1, results[i + 1].score);
        }
    }
}
