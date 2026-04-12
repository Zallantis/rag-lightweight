pub mod chunks;
pub mod documents;
pub mod filter;
pub mod hierarchy;
pub mod schema;
pub mod search;
pub mod stats;

use std::path::Path;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};

pub type SurrealClient = Surreal<Db>;

pub async fn connect(db_path: &Path) -> crate::error::Result<SurrealClient> {
    let db: SurrealClient = Surreal::new::<SurrealKv>(db_path).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("lock") || msg.contains("locked") || msg.contains("permission") {
            crate::error::AppError::Config(format!(
                "Cannot open database at '{}': {}. \
                 If the daemon (serve) is running, stop it before running CLI commands.",
                db_path.display(),
                msg
            ))
        } else {
            crate::error::AppError::Database(e)
        }
    })?;
    db.use_ns("rag").use_db("main").await?;
    Ok(db)
}

pub async fn init(db_path: &Path, dimension: usize) -> crate::error::Result<SurrealClient> {
    let db = connect(db_path).await?;
    schema::apply(&db, dimension).await?;
    Ok(db)
}

pub async fn shutdown(db: SurrealClient) {
    db.invalidate().await.ok();
    drop(db);
}
