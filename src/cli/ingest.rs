use crate::cli::progress::ProgressBar;
use crate::db;
use crate::ingest::chunker;
use crate::ingest::scanner;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub async fn run(
    path: PathBuf,
    extensions: Option<String>,
    exclude: Option<String>,
    source: String,
    max_tokens: usize,
    db_path: PathBuf,
) -> crate::error::Result<()> {
    let dimension: usize = std::env::var("EMBEDDING_DIMENSION")
        .map_err(|_| crate::error::AppError::Config("EMBEDDING_DIMENSION is required".into()))?
        .parse::<usize>()
        .map_err(|_| {
            crate::error::AppError::Config("EMBEDDING_DIMENSION must be a number".into())
        })?;
    let db = db::init(&db_path, dimension).await?;

    let extensions: Option<Vec<String>> =
        extensions.map(|e| e.split(',').map(|s| s.trim().to_string()).collect());
    let exclude: Option<Vec<String>> =
        exclude.map(|e| e.split(',').map(|s| s.trim().to_string()).collect());

    let files = scanner::scan_directory(&path, extensions.as_deref(), exclude.as_deref());

    let mut parser = tree_sitter::Parser::new();
    let mut processed = 0usize;
    let mut skipped = 0usize;
    let mut total_chunks = 0usize;
    let mut pb = ProgressBar::new(files.len(), "ingesting");

    for file in &files {
        let content = match &file.file_type {
            scanner::FileType::Pdf => match extract_pdf_text(&file.path) {
                Ok(text) => text,
                Err(e) => {
                    tracing::warn!("Failed to extract PDF {}: {}", file.path.display(), e);
                    skipped += 1;
                    pb.inc(1);
                    continue;
                }
            },
            _ => match std::fs::read_to_string(&file.path) {
                Ok(content) => content,
                Err(e) => {
                    tracing::warn!("Failed to read {}: {}", file.path.display(), e);
                    skipped += 1;
                    pb.inc(1);
                    continue;
                }
            },
        };

        if content.trim().is_empty() {
            skipped += 1;
            pb.inc(1);
            continue;
        }

        let content_hash = {
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        let title = file.relative_path.clone();
        let source_id = file.relative_path.clone();

        let (doc_id, changed) = db::documents::upsert_document(
            &db,
            &source,
            &source_id,
            &title,
            &content,
            &content_hash,
        )
        .await?;

        if !changed {
            skipped += 1;
            pb.inc(1);
            continue;
        }

        let chunks = chunker::chunk_content(&content, &file.file_type, max_tokens, &mut parser);

        let chunk_data: Vec<(String, usize, Option<String>)> = chunks
            .into_iter()
            .map(|c| (c.content, c.token_count, Some(c.content_hash)))
            .collect();

        let chunk_count = db::chunks::replace_chunks(&db, &doc_id, chunk_data).await?;
        total_chunks += chunk_count;
        processed += 1;

        pb.inc(1);
        tracing::debug!("Processed {} ({} chunks)", file.relative_path, chunk_count);
    }

    pb.finish();
    println!("\nIngest complete:");
    println!("  Files scanned:  {}", files.len());
    println!("  Processed:      {}", processed);
    println!("  Skipped:        {}", skipped);
    println!("  Chunks created: {}", total_chunks);

    db::shutdown(db).await;
    Ok(())
}

fn extract_pdf_text(path: &std::path::Path) -> crate::error::Result<String> {
    let bytes = std::fs::read(path)?;
    pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| crate::error::AppError::Ingest(format!("PDF extraction failed: {e}")))
}
