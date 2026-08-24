use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::MemoryEntry;
use crate::services::tags;
use crate::AppState;

const MEMORY_COLS: &str = "m.id, m.merchant, m.category_id, c.name AS category_name, \
     m.confidence, m.confirm_count, m.last_confirmed_at";

// For INSERT/UPDATE RETURNING (no `categories` join available).
const MEMORY_RETURNING: &str = "id, merchant, category_id, NULL::text AS category_name, \
     confidence, confirm_count, last_confirmed_at";

pub async fn list_memory(
    State(state): State<AppState>,
) -> Result<Json<Vec<MemoryEntry>>, AppError> {
    let entries = sqlx::query_as::<_, MemoryEntry>(sqlx::AssertSqlSafe(format!(
        "SELECT {MEMORY_COLS}
         FROM merchant_memory m
         LEFT JOIN categories c ON c.id = m.category_id
         ORDER BY m.confirm_count DESC, m.merchant"
    )))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(entries))
}

#[derive(Debug, Deserialize)]
pub struct MemoryInput {
    pub merchant: String,
    pub category_id: Option<Uuid>,
}

pub async fn create_memory(
    State(state): State<AppState>,
    Json(input): Json<MemoryInput>,
) -> Result<Json<MemoryEntry>, AppError> {
    let merchant = tags::strip_accents(input.merchant.trim()).to_lowercase();
    if merchant.is_empty() {
        return Err(AppError::bad_request("merchant is empty"));
    }

    let entry = sqlx::query_as::<_, MemoryEntry>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO merchant_memory (merchant, category_id, confidence, confirm_count)
         VALUES ($1, $2, 0.5, 0)
         ON CONFLICT (merchant) DO UPDATE SET
           category_id = EXCLUDED.category_id,
           updated_at = now()
         RETURNING {MEMORY_RETURNING}"
    )))
    .bind(&merchant)
    .bind(input.category_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(entry))
}

#[derive(Debug, Deserialize)]
pub struct MemoryUpdate {
    pub category_id: Option<Uuid>,
}

pub async fn update_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<MemoryUpdate>,
) -> Result<Json<MemoryEntry>, AppError> {
    let entry = sqlx::query_as::<_, MemoryEntry>(sqlx::AssertSqlSafe(format!(
        "UPDATE merchant_memory SET category_id = $1, updated_at = now()
         WHERE id = $2
         RETURNING {MEMORY_RETURNING}"
    )))
    .bind(input.category_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("memory entry not found"))?;
    Ok(Json(entry))
}

pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let res = sqlx::query("DELETE FROM merchant_memory WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("memory entry not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct ApplyAllRequest {
    pub merchant: String,
}

/// Apply the merchant's remembered category to every *uncategorized* item of
/// that merchant (any status). Tags are situational and are NOT applied.
pub async fn apply_all(
    State(state): State<AppState>,
    Json(input): Json<ApplyAllRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let normalized = tags::strip_accents(input.merchant.trim()).to_lowercase();

    let mem: Option<(Uuid,)> = sqlx::query_as(
        "SELECT category_id FROM merchant_memory WHERE merchant = $1",
    )
    .bind(&normalized)
    .fetch_optional(&state.pool)
    .await?;
    let Some((category_id,)) = mem else {
        return Err(AppError::not_found("no categorization memory for this merchant"));
    };

    let rows: Vec<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT id, merchant FROM items WHERE merchant IS NOT NULL AND category_id IS NULL",
    )
    .fetch_all(&state.pool)
    .await?;

    let mut updated = 0;
    for (id, item_merchant) in rows {
        if let Some(m) = item_merchant {
            if tags::strip_accents(m.trim()).to_lowercase() == normalized {
                sqlx::query(
                    "UPDATE items SET category_id = $1, updated_at = now() WHERE id = $2",
                )
                .bind(category_id)
                .bind(id)
                .execute(&state.pool)
                .await?;
                updated += 1;
            }
        }
    }

    Ok(Json(json!({ "updated": updated })))
}
