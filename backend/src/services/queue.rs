use std::time::Duration;

use sqlx::PgPool;
use tracing::{error, info};

use crate::models::DocumentRow;
use crate::services::ai::AiClient;
use crate::services::ingest;

const DOC_COLS: &str = "id, kind, account_id, source_id, filename, content_type, sha256, file_path, \
     status, error_message, ocr_text, uploaded_at, processed_at";

/// Background worker: claim `pending` documents and process them.
/// Survives restarts because the queue lives in the `documents` table.
pub async fn run_worker(pool: PgPool, ai: AiClient) {
    info!("document worker started");
    loop {
        match claim_next(&pool).await {
            Some(doc) => {
                info!(document = %doc.id, filename = %doc.filename, "processing document");
                if let Err(e) = ingest::process_document(&pool, &doc, &ai).await {
                    error!(document = %doc.id, "processing failed: {e:#}");
                    let _ = sqlx::query(
                        "UPDATE documents SET status = 'failed', error_message = $1, processed_at = now() WHERE id = $2",
                    )
                    .bind(format!("{e:#}"))
                    .bind(doc.id)
                    .execute(&pool)
                    .await;
                } else {
                    info!(document = %doc.id, "document processed");
                }
            }
            None => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
}

async fn claim_next(pool: &PgPool) -> Option<DocumentRow> {
    sqlx::query_as::<_, DocumentRow>(sqlx::AssertSqlSafe(format!(
        "UPDATE documents SET status = 'processing'
         WHERE id = (SELECT id FROM documents WHERE status = 'pending' ORDER BY uploaded_at LIMIT 1)
         RETURNING {DOC_COLS}"
    )))
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}
