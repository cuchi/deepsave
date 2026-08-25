use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::MemoryEntry;
use crate::services::{memory, tags};
use crate::AppState;

const MEMORY_COLS: &str = "m.id, m.merchant, m.category_id, c.name AS category_name, \
     m.tags, m.confidence, m.confirm_count, m.last_confirmed_at";

// For INSERT/UPDATE RETURNING (no `categories` join available).
const MEMORY_RETURNING: &str = "id, merchant, category_id, NULL::text AS category_name, \
     tags, confidence, confirm_count, last_confirmed_at";

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
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn create_memory(
    State(state): State<AppState>,
    Json(input): Json<MemoryInput>,
) -> Result<Json<MemoryEntry>, AppError> {
    let merchant = tags::strip_accents(input.merchant.trim()).to_lowercase();
    if merchant.is_empty() {
        return Err(AppError::bad_request("merchant is empty"));
    }
    let tags = tags::normalize(&input.tags);

    let entry = sqlx::query_as::<_, MemoryEntry>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO merchant_memory (merchant, category_id, tags, confidence, confirm_count)
         VALUES ($1, $2, $3, 0.5, 0)
         ON CONFLICT (merchant) DO UPDATE SET
           category_id = EXCLUDED.category_id,
           tags = EXCLUDED.tags,
           updated_at = now()
         RETURNING {MEMORY_RETURNING}"
    )))
    .bind(&merchant)
    .bind(input.category_id)
    .bind(&tags)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(entry))
}

#[derive(Debug, Deserialize)]
pub struct MemoryUpdate {
    pub category_id: Option<Uuid>,
    /// `Some` replaces the remembered tags; `None` keeps them.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

pub async fn update_memory(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<MemoryUpdate>,
) -> Result<Json<MemoryEntry>, AppError> {
    let tags = input.tags.as_deref().map(tags::normalize);
    let entry = sqlx::query_as::<_, MemoryEntry>(sqlx::AssertSqlSafe(format!(
        "UPDATE merchant_memory SET category_id = $1,
           tags = COALESCE($2, merchant_memory.tags),
           updated_at = now()
         WHERE id = $3
         RETURNING {MEMORY_RETURNING}"
    )))
    .bind(input.category_id)
    .bind(tags)
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

// ---------------------------------------------------------------------------
// Preview-before-apply (logic lives in services/memory.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    /// Merchant (normalized later). Absent/null = all merchants with memory.
    #[serde(default)]
    pub merchant: Option<String>,
}

/// `POST /memory/preview` — the items that *would* change if the remembered
/// category/tags were applied (single merchant or all). The user picks which
/// ones to apply and sends the ids to `POST /memory/apply`.
pub async fn preview(
    State(state): State<AppState>,
    Json(input): Json<PreviewRequest>,
) -> Result<Json<Vec<memory::PreviewItem>>, AppError> {
    let items = memory::preview_candidates(&state.pool, input.merchant.as_deref()).await?;
    Ok(Json(items))
}

#[derive(Debug, Deserialize)]
pub struct ApplyRequest {
    /// Optional merchant restriction (validated against the ids).
    #[serde(default)]
    pub merchant: Option<String>,
    /// Item ids to apply memory to (from the preview selection).
    pub ids: Vec<Uuid>,
}

/// `POST /memory/apply` — apply the remembered category + tags **only** to the
/// selected ids. Category replaces/clears (as today); tags are added (union).
pub async fn apply(
    State(state): State<AppState>,
    Json(input): Json<ApplyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let updated = memory::apply_selected(&state.pool, input.merchant.as_deref(), &input.ids).await?;
    Ok(Json(json!({ "updated": updated })))
}
