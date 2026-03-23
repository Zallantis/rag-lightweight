use std::path::PathBuf;
use crate::config::EmbeddingConfig;
use crate::db;

pub async fn run(db_path: PathBuf) -> crate::error::Result<()> {
    let embed_cfg = EmbeddingConfig::from_env().ok();

    let dimension = embed_cfg.as_ref().map(|c| c.dimension).unwrap_or(0);
    let db = db::init(&db_path, dimension).await?;

    let doc_count = db::documents::count_documents(&db).await?;
    let chunk_counts = db::chunks::count_chunks(&db).await?;
    let by_source = db::documents::documents_by_source(&db).await?;

    let (embedding_model, embedding_dimension) = match embed_cfg {
        Some(cfg) => (cfg.model, cfg.dimension.to_string()),
        None => ("not set".to_string(), "not set".to_string()),
    };

    println!("RAG Database Status");
    println!("===================");
    println!("Documents:        {}", doc_count);
    println!("Chunks:           {}", chunk_counts.total);
    println!("  Embedded:       {}", chunk_counts.embedded);
    println!("  Pending:        {}", chunk_counts.pending);
    println!("Embedding model:  {}", embedding_model);
    println!("Dimension:        {}", embedding_dimension);

    if !by_source.is_empty() {
        println!("\nDocuments by source:");
        for s in &by_source {
            println!("  {}: {}", s.source, s.count);
        }
    }

    db::shutdown(db).await;
    Ok(())
}
