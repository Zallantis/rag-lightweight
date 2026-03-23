use surrealdb::types::SurrealValue;

use crate::db::SurrealClient;
use crate::db::chunks::ChunkCounts;
use crate::db::documents::SourceCount;

#[derive(Debug, SurrealValue)]
struct CountResult {
    count: usize,
}

pub async fn get_stats(
    db: &SurrealClient,
) -> crate::error::Result<(usize, ChunkCounts, Vec<SourceCount>)> {
    let sql = "\
        SELECT count() AS count FROM document GROUP ALL; \
        SELECT count() AS count FROM chunk GROUP ALL; \
        SELECT count() AS count FROM chunk WHERE vector IS NOT NONE GROUP ALL; \
        SELECT source, count() AS count FROM document GROUP BY source";

    let mut response = db.query(sql).await?;

    let doc_count: Option<CountResult> = response.take(0)?;
    let total: Option<CountResult> = response.take(1)?;
    let embedded: Option<CountResult> = response.take(2)?;
    let by_source: Vec<SourceCount> = response.take(3)?;

    let total = total.map(|r| r.count).unwrap_or(0);
    let embedded = embedded.map(|r| r.count).unwrap_or(0);

    Ok((
        doc_count.map(|r| r.count).unwrap_or(0),
        ChunkCounts {
            total,
            embedded,
            pending: total.saturating_sub(embedded),
        },
        by_source,
    ))
}
