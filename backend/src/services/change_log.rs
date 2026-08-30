//! Durable change log: append-only record of every category/tag change the user
//! makes, so the AI can learn from the user's curation history (F10).
//!
//! The identity used for grouping is `merchant_key` — the item's merchant name
//! normalized, falling back to its description when merchant is NULL (Pluggy
//! items). Same normalization the AI payloads use, so `hist` matches cleanly.

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::services::tags;

/// Normalize an item's merchant-or-description into a stable identity key
/// (strip accents, lowercase, drop non-alphanumerics).
pub fn merchant_key(merchant: Option<&str>, description: &str) -> String {
    let src = merchant
        .map(|m| tags::strip_accents(m).to_lowercase())
        .unwrap_or_else(|| tags::strip_accents(description).to_lowercase());
    src.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Record a change for an item (skipped when nothing actually changed).
/// `before`/`after` are the category/tags before and after the mutation.
pub async fn log_item_change(
    pool: &PgPool,
    item_id: Uuid,
    before_cat: Option<Uuid>,
    after_cat: Option<Uuid>,
    before_tags: &[String],
    after_tags: &[String],
    source: &str,
) -> Result<()> {
    if before_cat == after_cat && before_tags == after_tags {
        return Ok(());
    }
    let Some((merchant, description, occurred_on)): Option<(Option<String>, String, chrono::NaiveDate)> =
        sqlx::query_as("SELECT merchant, description, occurred_on FROM items WHERE id = $1")
            .bind(item_id)
            .fetch_optional(pool)
            .await?
    else {
        return Ok(());
    };
    let key = merchant_key(merchant.as_deref(), &description);

    sqlx::query(
        "INSERT INTO change_log
           (item_id, merchant_key, category_before, category_after, tags_before, tags_after, source, tx_date)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(item_id)
    .bind(&key)
    .bind(before_cat)
    .bind(after_cat)
    .bind(before_tags)
    .bind(after_tags)
    .bind(source)
    .bind(occurred_on)
    .execute(pool)
    .await?;
    Ok(())
}
