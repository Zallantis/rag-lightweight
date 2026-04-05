use crate::db::SurrealClient;

pub async fn apply(db: &SurrealClient, dimension: usize) -> crate::error::Result<()> {

    let schema = format!(r#"
        DEFINE TABLE IF NOT EXISTS document SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS source ON document TYPE string;
        DEFINE FIELD IF NOT EXISTS source_id ON document TYPE string;
        DEFINE FIELD IF NOT EXISTS title ON document TYPE string;
        DEFINE FIELD IF NOT EXISTS content ON document TYPE string;
        DEFINE FIELD IF NOT EXISTS content_hash ON document TYPE option<string>;
        DEFINE FIELD IF NOT EXISTS metadata ON document TYPE option<object> FLEXIBLE;
        DEFINE FIELD IF NOT EXISTS created_at ON document TYPE datetime DEFAULT time::now();
        DEFINE FIELD IF NOT EXISTS updated_at ON document TYPE datetime DEFAULT time::now();

        DEFINE INDEX IF NOT EXISTS idx_document_source ON document FIELDS source;
        DEFINE INDEX IF NOT EXISTS idx_document_source_id ON document FIELDS source, source_id UNIQUE;
        DEFINE INDEX IF NOT EXISTS idx_document_content_hash ON document FIELDS content_hash;

        DEFINE ANALYZER IF NOT EXISTS vs TOKENIZERS blank,class FILTERS lowercase,ascii,snowball(english);
        DEFINE INDEX IF NOT EXISTS ft_content ON document FIELDS content FULLTEXT ANALYZER vs BM25(1.2, 0.75);

        DEFINE TABLE IF NOT EXISTS chunk SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS document ON chunk TYPE record<document>;
        DEFINE FIELD IF NOT EXISTS content ON chunk TYPE string;
        DEFINE FIELD IF NOT EXISTS position ON chunk TYPE int;
        DEFINE FIELD IF NOT EXISTS token_count ON chunk TYPE option<int>;
        DEFINE FIELD IF NOT EXISTS content_hash ON chunk TYPE option<string>;
        DEFINE FIELD IF NOT EXISTS embedded_at ON chunk TYPE option<datetime>;
        DEFINE FIELD IF NOT EXISTS vector ON chunk TYPE option<array<float>>;
        DEFINE FIELD IF NOT EXISTS metadata ON chunk TYPE option<object> FLEXIBLE;
        DEFINE FIELD IF NOT EXISTS created_at ON chunk TYPE datetime DEFAULT time::now();

        DEFINE INDEX IF NOT EXISTS idx_chunk_document ON chunk FIELDS document;
        DEFINE INDEX IF NOT EXISTS idx_chunk_doc_position ON chunk FIELDS document, position UNIQUE;
        DEFINE INDEX IF NOT EXISTS idx_chunk_vector ON chunk FIELDS vector HNSW DIMENSION {dimension} DIST COSINE TYPE F32;

        DEFINE FIELD IF NOT EXISTS custom_attributes ON document TYPE option<object> FLEXIBLE;

        DEFINE TABLE IF NOT EXISTS child_of SCHEMAFULL TYPE RELATION FROM document TO document;
        DEFINE FIELD IF NOT EXISTS created_at ON child_of TYPE datetime DEFAULT time::now();
        DEFINE INDEX IF NOT EXISTS idx_child_of_in ON child_of FIELDS in UNIQUE;
        DEFINE INDEX IF NOT EXISTS idx_child_of_out ON child_of FIELDS out;
    "#);

    db.query(schema).await?;
    tracing::info!("Database schema applied (dimension={dimension})");
    Ok(())
}
