use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{BulkItemUpdate, Item, ItemInput, TagsMode};
use crate::services::{memory, tags};
use crate::AppState;

pub(crate) const ITEM_COLS: &str = "id, parent_id, document_id, source, kind, status, account_id, \
     transfer_group_id, installment, installment_count, recurring_id, occurred_on, posted_on, \
     merchant, description, amount_cents, currency, category_id, suggested_category, tags, raw_line, \
     match_confidence, created_at, updated_at";

/// Kinds a bulk edit may assign (must match what the app/parsers produce).
const BULK_KINDS: [&str; 6] = [
    "expense",
    "income",
    "refund",
    "card_payment",
    "investment",
    "internal",
];

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// YYYY-MM, optional.
    pub month: Option<String>,
    /// Filter by item status, optional (e.g. 'pending_review').
    pub status: Option<String>,
    /// Free-text search across description, merchant and tags.
    pub search: Option<String>,
    pub category_id: Option<Uuid>,
    pub kind: Option<String>,
    /// Filter by an exact tag.
    pub tag: Option<String>,
    /// Filter by bank ('nubank' | 'c6' | 'caixa').
    pub bank: Option<String>,
    /// 'date' (default) or 'value'.
    pub sort: Option<String>,
}

fn month_range(month: Option<&str>) -> Result<(Option<NaiveDate>, Option<NaiveDate>), AppError> {
    let Some(m) = month else {
        return Ok((None, None));
    };
    let (y, mo) = m
        .split_once('-')
        .ok_or_else(|| AppError::bad_request("month must be YYYY-MM"))?;
    let year: i32 = y
        .parse()
        .map_err(|_| AppError::bad_request("invalid month"))?;
    let month_num: u32 = mo
        .parse()
        .map_err(|_| AppError::bad_request("invalid month"))?;
    if !(1..=12).contains(&month_num) {
        return Err(AppError::bad_request("invalid month"));
    }
    let start = NaiveDate::from_ymd_opt(year, month_num, 1)
        .ok_or_else(|| AppError::bad_request("invalid month"))?;
    let (next_y, next_m) = if month_num == 12 {
        (year + 1, 1)
    } else {
        (year, month_num + 1)
    };
    let end = NaiveDate::from_ymd_opt(next_y, next_m, 1)
        .ok_or_else(|| AppError::bad_request("invalid month"))?;
    Ok((Some(start), Some(end)))
}

// Note: the `format!` output is composed only of constant column lists (never user
// input), so wrapping in `AssertSqlSafe` is safe.
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Item>>, AppError> {
    let (start, end) = month_range(q.month.as_deref())?;
    let items = sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(format!(
        "SELECT {ITEM_COLS} FROM items
         WHERE ($1::date IS NULL OR occurred_on >= $1)
           AND ($2::date IS NULL OR occurred_on < $2)
           AND ($3::text IS NULL OR status = $3)
           AND ($4::text IS NULL
                OR description ILIKE '%' || $4 || '%'
                OR COALESCE(merchant, '') ILIKE '%' || $4 || '%'
                OR array_to_string(tags, ' ') ILIKE '%' || $4 || '%')
           AND ($5::uuid IS NULL OR category_id = $5)
           AND ($6::text IS NULL OR kind = $6)
           AND ($7::text IS NULL OR $7 = ANY(tags))
           AND ($8::text IS NULL OR EXISTS (
                 SELECT 1 FROM documents d
                 JOIN sources s ON s.id = d.source_id
                 WHERE d.id = items.document_id AND s.bank = $8))
         ORDER BY
           CASE WHEN $9 = 'value' THEN abs(amount_cents) END DESC,
           occurred_on DESC, created_at DESC"
    )))
    .bind(start)
    .bind(end)
    .bind(q.status)
    .bind(q.search)
    .bind(q.category_id)
    .bind(q.kind)
    .bind(q.tag)
    .bind(q.bank)
    .bind(q.sort)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(items))
}

/// Core bulk-edit logic, kept separate from the handler so integration tests can
/// call it without building an `AppState`. Runs the field updates in one
/// transaction; unknown ids are silently ignored. Memory recording is a
/// best-effort side effect performed after commit (opt-in via `update_memory`).
pub async fn bulk_update_items(
    pool: &PgPool,
    input: BulkItemUpdate,
) -> Result<serde_json::Value, AppError> {
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<Uuid> = input.ids.into_iter().filter(|id| seen.insert(*id)).collect();
    if ids.is_empty() {
        return Err(AppError::bad_request("ids must not be empty"));
    }
    if ids.len() > 1000 {
        return Err(AppError::bad_request("too many ids (max 1000)"));
    }
    if let Some(kind) = &input.kind {
        if !BULK_KINDS.contains(&kind.as_str()) {
            return Err(AppError::bad_request("invalid kind"));
        }
    }

    let category_changed = input.category_id.is_some();
    let tags = input.tags.as_deref().map(tags::normalize);
    let tags_mode = input.tags_mode.unwrap_or(TagsMode::Replace);

    let mut tx = pool.begin().await?;

    if let Some(kind) = &input.kind {
        sqlx::query("UPDATE items SET kind = $1, updated_at = now() WHERE id = ANY($2)")
            .bind(kind)
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
    }

    if let Some(category_id) = input.category_id {
        sqlx::query("UPDATE items SET category_id = $1, updated_at = now() WHERE id = ANY($2)")
            .bind(category_id)
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
    }

    if let Some(tags) = &tags {
        match tags_mode {
            TagsMode::Replace => {
                sqlx::query("UPDATE items SET tags = $1, updated_at = now() WHERE id = ANY($2)")
                    .bind(tags)
                    .bind(&ids)
                    .execute(&mut *tx)
                    .await?;
            }
            TagsMode::Add => {
                // `tags || $1` then dedupe, keeping first-occurrence order
                // (tags must stay unique per item).
                sqlx::query(
                    "UPDATE items SET tags = (\
                       SELECT array_agg(t ORDER BY ord) \
                       FROM (SELECT t, min(ord) AS ord \
                             FROM unnest(tags || $1) WITH ORDINALITY AS u(t, ord) \
                             GROUP BY t) s \
                     ), updated_at = now() WHERE id = ANY($2)",
                )
                .bind(tags)
                .bind(&ids)
                .execute(&mut *tx)
                .await?;
            }
            TagsMode::Remove => {
                sqlx::query(
                    "UPDATE items SET tags = (SELECT array_agg(t) FROM unnest(tags) AS t \
                     WHERE NOT (t = ANY($1))), updated_at = now() WHERE id = ANY($2)",
                )
                .bind(tags)
                .bind(&ids)
                .execute(&mut *tx)
                .await?;
            }
        };
    }

    tx.commit().await?;

    // Memory is a side channel: run after commit, opt-in only, and only when the
    // category is actually being changed. `category_id` here is Copy, so it is
    // still available after the match above.
    if input.update_memory && category_changed {
        let merchants: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT merchant FROM items WHERE id = ANY($1) AND merchant IS NOT NULL",
        )
        .bind(&ids)
        .fetch_all(pool)
        .await?;
        for m in &merchants {
            memory::record_confirmation(pool, m, input.category_id.flatten()).await?;
        }
    }

    // Report how many of the selected ids actually exist (unknown ids ignored).
    let updated: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE id = ANY($1)")
        .bind(&ids)
        .fetch_one(pool)
        .await?;

    Ok(json!({ "updated": updated }))
}

/// `PATCH /items/bulk` — thin wrapper over [`bulk_update_items`].
pub async fn bulk_update(
    State(state): State<AppState>,
    Json(input): Json<BulkItemUpdate>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(bulk_update_items(&state.pool, input).await?))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    let item = sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(format!(
        "SELECT {ITEM_COLS} FROM items WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("item not found"))?;
    Ok(Json(item))
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<ItemInput>,
) -> Result<Json<Item>, AppError> {
    let item = sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO items
           (parent_id, kind, account_id, installment, installment_count, occurred_on,
            merchant, description, amount_cents, currency, category_id, tags, source, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'manual', 'confirmed')
         RETURNING {ITEM_COLS}"
    )))
    .bind(input.parent_id)
    .bind(&input.kind)
    .bind(input.account_id)
    .bind(input.installment)
    .bind(input.installment_count)
    .bind(input.occurred_on)
    .bind(&input.merchant)
    .bind(&input.description)
    .bind(input.amount_cents)
    .bind(&input.currency)
    .bind(input.category_id)
    .bind(&tags::normalize(&input.tags))
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(item))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<ItemInput>,
) -> Result<Json<Item>, AppError> {
    let item = sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(format!(
        "UPDATE items
         SET parent_id = $1, kind = $2, account_id = $3, installment = $4,
             installment_count = $5, occurred_on = $6, merchant = $7, description = $8,
             amount_cents = $9, currency = $10, category_id = $11, tags = $12,
             updated_at = now()
         WHERE id = $13
         RETURNING {ITEM_COLS}"
    )))
    .bind(input.parent_id)
    .bind(&input.kind)
    .bind(input.account_id)
    .bind(input.installment)
    .bind(input.installment_count)
    .bind(input.occurred_on)
    .bind(&input.merchant)
    .bind(&input.description)
    .bind(input.amount_cents)
    .bind(&input.currency)
    .bind(input.category_id)
    .bind(&tags::normalize(&input.tags))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("item not found"))?;

    // A manual edit is a correction: feed the categorization memory.
    if let Some(m) = input.merchant.as_deref() {
        memory::record_confirmation(&state.pool, m, input.category_id).await?;
    }

    Ok(Json(item))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let res = sqlx::query("DELETE FROM items WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("item not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Mark a `pending_review` item as confirmed, and move its document to
/// `processed` once no pending items remain.
pub async fn confirm(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row: Option<(Option<Uuid>, Option<String>, Option<Uuid>)> =
        sqlx::query_as("SELECT document_id, merchant, category_id FROM items WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((document_id, merchant, category_id)) = row else {
        return Err(AppError::not_found("item not found"));
    };

    sqlx::query("UPDATE items SET status = 'confirmed', updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    // Strengthen the categorization memory on confirmation.
    if let Some(m) = &merchant {
        memory::record_confirmation(&state.pool, m, category_id).await?;
    }
    if let Some(document_id) = document_id {
        finalize_document_if_done(&state.pool, document_id).await?;
    }

    Ok(Json(json!({ "ok": true })))
}

/// Mark a `pending_review` item as rejected.
pub async fn reject(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    set_item_status(&state, id, "rejected").await?;
    Ok(Json(json!({ "ok": true })))
}

/// Apply the remembered category for this item's merchant (one-click).
/// Tags are situational and are NOT applied from memory.
pub async fn apply_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT merchant FROM items WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((Some(merchant),)) = row else {
        return Err(AppError::not_found("item not found or has no merchant"));
    };

    let normalized = tags::strip_accents(merchant.trim()).to_lowercase();
    let mem: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT category_id FROM merchant_memory WHERE merchant = $1",
    )
    .bind(&normalized)
    .fetch_optional(&state.pool)
    .await?;
    let Some((Some(category_id),)) = mem else {
        return Err(AppError::not_found("no categorization memory for this merchant"));
    };

    let item = sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(format!(
        "UPDATE items SET category_id = $1, updated_at = now() WHERE id = $2 RETURNING {ITEM_COLS}"
    )))
    .bind(category_id)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(item))
}

/// Create the suggested category (if needed) and assign it to the item.
pub async fn accept_suggestion(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Item>, AppError> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT suggested_category FROM items WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((Some(name),)) = row else {
        return Err(AppError::bad_request("no suggested category for this item"));
    };
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::bad_request("empty suggested category"));
    }

    let category_id: Uuid = sqlx::query_scalar(
        "INSERT INTO categories (name) VALUES ($1)
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
         RETURNING id",
    )
    .bind(&name)
    .fetch_one(&state.pool)
    .await?;

    let item = sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(format!(
        "UPDATE items SET category_id = $1, suggested_category = NULL, updated_at = now()
         WHERE id = $2 RETURNING {ITEM_COLS}"
    )))
    .bind(category_id)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(item))
}

async fn set_item_status(state: &AppState, id: Uuid, status: &str) -> Result<(), AppError> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as("SELECT document_id FROM items WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    let Some((document_id,)) = row else {
        return Err(AppError::not_found("item not found"));
    };

    sqlx::query("UPDATE items SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status)
        .bind(id)
        .execute(&state.pool)
        .await?;

    if let Some(document_id) = document_id {
        finalize_document_if_done(&state.pool, document_id).await?;
    }
    Ok(())
}

async fn finalize_document_if_done(pool: &PgPool, document_id: Uuid) -> Result<(), AppError> {
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM items WHERE document_id = $1 AND status = 'pending_review'",
    )
    .bind(document_id)
    .fetch_one(pool)
    .await?;
    if pending == 0 {
        sqlx::query("UPDATE documents SET status = 'processed' WHERE id = $1")
            .bind(document_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}
