use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::cli::progress::ProgressBar;
use crate::config::EmbeddingConfig;
use crate::db;
use crate::embed::service::EmbedRole;

pub async fn run(batch_size: usize, force: bool, db_path: PathBuf) -> crate::error::Result<()> {
    let embedding_config = EmbeddingConfig::from_env()?;
    let db = db::init(&db_path, embedding_config.dimension).await?;
    let embedding_service = crate::embed::create_embedding_service(&embedding_config)?;

    if force {
        println!("Force mode: clearing all existing vectors...");
        db::chunks::clear_all_vectors(&db).await?;
    }

    let chunk_counts = db::chunks::count_chunks(&db).await?;
    if chunk_counts.pending == 0 {
        println!(
            "Nothing to embed. All {} chunks already have vectors.",
            chunk_counts.total
        );
        db::shutdown(db).await;
        return Ok(());
    }

    println!(
        "Embedding {} pending chunks (batch size: {}, model: {})",
        chunk_counts.pending, batch_size, embedding_config.model
    );

    let total_pending = chunk_counts.pending;
    let start = std::time::Instant::now();

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        cancelled_clone.store(true, Ordering::Relaxed);
    });

    let mut pb = ProgressBar::new(total_pending, "embedding");
    let mut total_embedded = 0usize;

    loop {
        if cancelled.load(Ordering::Relaxed) {
            pb.finish();
            println!("\nInterrupted. {} chunks embedded so far.", total_embedded);
            db::shutdown(db).await;
            return Ok(());
        }

        let pending = db::chunks::get_pending_chunks(&db, batch_size).await?;
        if pending.is_empty() {
            break;
        }

        let batch_len = pending.len();
        let texts: Vec<String> = pending.iter().map(|c| c.content.clone()).collect();
        let vectors = embedding_service
            .embed_with_role(texts, EmbedRole::Passage)
            .await?;

        let updates: Vec<_> = pending
            .iter()
            .zip(vectors.into_iter())
            .map(|(chunk, vector)| (chunk.id.clone(), vector))
            .collect();
        db::chunks::bulk_update_chunk_vectors(&db, updates).await?;

        total_embedded += batch_len;
        pb.inc(batch_len);
    }

    pb.finish();
    let elapsed = start.elapsed();
    println!("\nEmbed complete:");
    println!("  Chunks embedded: {}", total_embedded);
    println!("  Elapsed: {:.1}s", elapsed.as_secs_f64());

    db::shutdown(db).await;
    Ok(())
}
