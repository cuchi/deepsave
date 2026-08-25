use anyhow::Result;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::services::tags;

/// Record a user confirmation/correction for a merchant, strengthening the
/// categorization memory that is injected into future AI prompts.
///
/// Tags **accumulate** (union): re-confirming with fewer tags never erases
/// previously confirmed ones — memory tags grow over time. Category stays
/// "latest wins" (COALESCE keeps the previous one when a confirmation has none).
pub async fn record_confirmation(
    pool: &PgPool,
    merchant: &str,
    category_id: Option<Uuid>,
    item_tags: &[String],
) -> Result<()> {
    let merchant = tags::strip_accents(merchant.trim()).to_lowercase();
    if merchant.is_empty() {
        return Ok(());
    }
    let item_tags = tags::normalize(item_tags);

    sqlx::query(
        "INSERT INTO merchant_memory (merchant, category_id, tags, confidence, confirm_count, last_confirmed_at)
         VALUES ($1, $2, $3, 0.5, 1, now())
         ON CONFLICT (merchant) DO UPDATE SET
           category_id = COALESCE(EXCLUDED.category_id, merchant_memory.category_id),
           tags = COALESCE(
             (SELECT array_agg(t ORDER BY ord)
              FROM (SELECT t, min(ord) AS ord
                    FROM unnest(merchant_memory.tags || EXCLUDED.tags) WITH ORDINALITY AS u(t, ord)
                    GROUP BY t) s),
             '{}'::text[]),
           confirm_count = merchant_memory.confirm_count + 1,
           confidence = LEAST(1.0, merchant_memory.confidence + 0.2),
           last_confirmed_at = now(),
           updated_at = now()",
    )
    .bind(&merchant)
    .bind(category_id)
    .bind(&item_tags)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Preview-before-apply
// ---------------------------------------------------------------------------

/// One item that would change if the remembered category/tags were applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreviewItem {
    pub item_id: Uuid,
    pub merchant: String,
    pub description: String,
    pub occurred_on: NaiveDate,
    pub amount_cents: i64,
    pub current_category: Option<String>,
    pub proposed_category: Option<String>,
    /// Tags missing on the item that memory would add.
    pub tags_to_add: Vec<String>,
    /// Subset of `["category", "tags"]` — what would actually change.
    pub changes: Vec<String>,
}

/// `(merchant, category_id, category_name, tags)` — one row per memory entry.
type MemoryRow = (String, Option<Uuid>, Option<String>, Vec<String>);

/// `(item_id, merchant, description, occurred_on, amount_cents, category_id, category_name, tags)`
type ItemRow = (
    Uuid,
    Option<String>,
    String,
    NaiveDate,
    i64,
    Option<Uuid>,
    Option<String>,
    Vec<String>,
);

/// Load memory rows, optionally restricted to one (normalized) merchant.
async fn load_memory(pool: &PgPool, merchant: Option<&str>) -> Result<Vec<MemoryRow>> {
    let rows: Vec<MemoryRow> = sqlx::query_as(
        "SELECT m.merchant, m.category_id, c.name, m.tags
         FROM merchant_memory m
         LEFT JOIN categories c ON c.id = m.category_id
         WHERE m.category_id IS NOT NULL OR cardinality(m.tags) > 0",
    )
    .fetch_all(pool)
    .await?;

    let normalized = merchant.map(|m| tags::strip_accents(m.trim()).to_lowercase());
    Ok(rows
        .into_iter()
        .filter(|(m, _, _, _)| normalized.as_deref().map_or(true, |want| m == want))
        .collect())
}

/// Load merchant-bearing items; `ids: None` = all.
async fn load_items(pool: &PgPool, ids: Option<&[Uuid]>) -> Result<Vec<ItemRow>> {
    let items: Vec<ItemRow> = sqlx::query_as(
        "SELECT i.id, i.merchant, i.description, i.occurred_on, i.amount_cents,
                i.category_id, c.name, i.tags
         FROM items i
         LEFT JOIN categories c ON c.id = i.category_id
         WHERE i.merchant IS NOT NULL
           AND ($1::uuid[] IS NULL OR i.id = ANY($1))",
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

/// Compute the preview rows for the given memory entries and items.
fn build_preview(memory: &[MemoryRow], items: Vec<ItemRow>) -> Vec<PreviewItem> {
    let by_merchant: std::collections::HashMap<&str, &MemoryRow> =
        memory.iter().map(|m| (m.0.as_str(), m)).collect();

    let mut out = Vec::new();
    for (id, merchant, description, occurred_on, amount_cents, cat_id, cat_name, item_tags) in items {
        let Some(merchant) = merchant else { continue };
        let norm = tags::strip_accents(merchant.trim()).to_lowercase();
        let Some((_, mem_cat_id, mem_cat_name, mem_tags)) = by_merchant.get(norm.as_str()) else {
            continue;
        };

        let mut changes = Vec::new();
        let mut proposed_category = None;
        if mem_cat_id.is_some() && cat_id != *mem_cat_id {
            changes.push("category".to_string());
            proposed_category = mem_cat_name.clone();
        }
        let tags_to_add: Vec<String> = mem_tags
            .iter()
            .filter(|t| !item_tags.contains(t))
            .cloned()
            .collect();
        if !tags_to_add.is_empty() {
            changes.push("tags".to_string());
        }
        if changes.is_empty() {
            continue;
        }

        out.push(PreviewItem {
            item_id: id,
            merchant,
            description,
            occurred_on,
            amount_cents,
            current_category: cat_name,
            proposed_category,
            tags_to_add,
            changes,
        });
    }
    out.sort_by(|a, b| a.merchant.cmp(&b.merchant).then(a.occurred_on.cmp(&b.occurred_on)));
    out
}

/// The items that *would* change if the remembered category/tags were applied
/// (single merchant or all). The user picks which ones to apply and sends the
/// ids to [`apply_selected`].
pub async fn preview_candidates(pool: &PgPool, merchant: Option<&str>) -> Result<Vec<PreviewItem>> {
    let memory = load_memory(pool, merchant).await?;
    if memory.is_empty() {
        return Ok(Vec::new());
    }
    let items = load_items(pool, None).await?;
    Ok(build_preview(&memory, items))
}

/// Apply the remembered category + tags **only** to the selected item ids.
/// Category replaces/clears (as today); tags are added (union) — situational
/// tags on the items stay. Idempotent; returns how many rows were updated.
pub async fn apply_selected(
    pool: &PgPool,
    merchant: Option<&str>,
    ids: &[Uuid],
) -> Result<usize> {
    let memory = load_memory(pool, merchant).await?;
    if memory.is_empty() || ids.is_empty() {
        return Ok(0);
    }
    let items = load_items(pool, Some(ids)).await?;

    let by_merchant: std::collections::HashMap<&str, &MemoryRow> =
        memory.iter().map(|m| (m.0.as_str(), m)).collect();

    let mut tx = pool.begin().await?;
    let mut updated = 0usize;
    for (id, merchant, _, _, _, _, _, _) in items {
        let Some(merchant) = merchant else { continue };
        let norm = tags::strip_accents(merchant.trim()).to_lowercase();
        let Some((_, mem_cat_id, _, mem_tags)) = by_merchant.get(norm.as_str()) else {
            continue;
        };

        let res = sqlx::query(
            "UPDATE items SET
               category_id = COALESCE($1, category_id),
               tags = COALESCE(
                 (SELECT array_agg(t ORDER BY ord)
                  FROM (SELECT t, min(ord) AS ord
                        FROM unnest(items.tags || $2) WITH ORDINALITY AS u(t, ord)
                        GROUP BY t) s),
                 '{}'::text[]),
               updated_at = now()
             WHERE id = $3",
        )
        .bind(mem_cat_id)
        .bind(mem_tags)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        updated += res.rows_affected() as usize;
    }
    tx.commit().await?;

    Ok(updated)
}
