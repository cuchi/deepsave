use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::services::tags;

/// Record a user confirmation/correction for a merchant, strengthening the
/// categorization memory that is injected into future AI prompts.
pub async fn record_confirmation(
    pool: &PgPool,
    merchant: &str,
    category_id: Option<Uuid>,
) -> Result<()> {
    let merchant = tags::strip_accents(merchant.trim()).to_lowercase();
    if merchant.is_empty() {
        return Ok(());
    }

    sqlx::query(
        "INSERT INTO merchant_memory (merchant, category_id, confidence, confirm_count, last_confirmed_at)
         VALUES ($1, $2, 0.5, 1, now())
         ON CONFLICT (merchant) DO UPDATE SET
           category_id = COALESCE(EXCLUDED.category_id, merchant_memory.category_id),
           confirm_count = merchant_memory.confirm_count + 1,
           confidence = LEAST(1.0, merchant_memory.confidence + 0.2),
           last_confirmed_at = now(),
           updated_at = now()",
    )
    .bind(&merchant)
    .bind(category_id)
    .execute(pool)
    .await?;

    Ok(())
}
