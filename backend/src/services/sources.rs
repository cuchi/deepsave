use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

/// Detect `(bank, document_kind)` from raw document text.
/// `kind` is `'bank_statement' | 'card_statement'` (matching `documents.kind`).
pub fn detect_bank_kind(text: &str) -> Option<(&'static str, &'static str)> {
    let lower = text.to_lowercase();

    if lower.contains("data de compra") {
        return Some(("c6", "card_statement"));
    }
    if lower.contains("extrato de conta corrente") || lower.contains("data lançamento") {
        return Some(("c6", "bank_statement"));
    }
    if lower.contains("identificador") && lower.contains("descri") {
        return Some(("nubank", "bank_statement"));
    }
    if lower.contains("title") && lower.contains("amount") {
        return Some(("nubank", "card_statement"));
    }
    if lower.contains("cartões caixa")
        || lower.contains("cartoes caixa")
        || lower.contains("compras (cartão")
        || lower.contains("compras (cartao")
    {
        return Some(("caixa", "card_statement"));
    }
    if lower.contains("extrato por período") || lower.contains("extrato por periodo") {
        return Some(("caixa", "bank_statement"));
    }
    None
}

/// Look up the `sources.id` for a detected (bank, kind).
pub async fn source_id_for_text(pool: &PgPool, text: &str) -> Result<Option<Uuid>> {
    let Some((bank, kind)) = detect_bank_kind(text) else {
        return Ok(None);
    };
    let id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM sources WHERE bank = $1 AND kind = $2")
        .bind(bank)
        .bind(kind)
        .fetch_optional(pool)
        .await?;
    Ok(id)
}

/// Detect the source for an uploaded document (from its filename + bytes).
/// Images (receipts) have no fundamental source.
pub async fn detect_source(
    pool: &PgPool,
    filename: &str,
    content_type: &str,
    data: &[u8],
) -> Result<Option<Uuid>> {
    let lower = filename.to_lowercase();

    let text: Option<String> = if lower.ends_with(".csv") || content_type.contains("csv") {
        Some(String::from_utf8_lossy(data).to_string())
    } else if lower.ends_with(".pdf") || content_type.contains("pdf") {
        let bytes = data.to_vec();
        tokio::task::spawn_blocking(move || pdf_extract::extract_text_from_mem(&bytes).ok())
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let Some(text) = text else {
        return Ok(None);
    };
    source_id_for_text(pool, &text).await
}

/// Re-detect sources for documents that have none (e.g. uploaded before the
/// sources feature existed). Reads each file and updates `documents.source_id`.
pub async fn backfill_null_sources(pool: PgPool) -> Result<usize> {
    let docs: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, filename, content_type, file_path FROM documents WHERE source_id IS NULL",
    )
    .fetch_all(&pool)
    .await?;

    let mut updated = 0;
    for (id, filename, content_type, file_path) in docs {
        let Ok(data) = tokio::fs::read(&file_path).await else {
            continue;
        };
        if let Some(source_id) = detect_source(&pool, &filename, &content_type, &data).await? {
            sqlx::query("UPDATE documents SET source_id = $1 WHERE id = $2")
                .bind(source_id)
                .bind(id)
                .execute(&pool)
                .await?;
            updated += 1;
        }
    }
    Ok(updated)
}
