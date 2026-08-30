use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{AiTagBatch, SuggestionDetail};
use crate::services::ai_tags;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateBatchInput {
    pub ids: Vec<Uuid>,
    /// 'tags' (default) | 'categorize'
    #[serde(default)]
    pub kind: Option<String>,
}

/// `POST /ai-tags/batches` — enqueue AI tagging (or categorization) for the
/// selected items.
pub async fn create_batch(
    State(state): State<AppState>,
    Json(input): Json<CreateBatchInput>,
) -> Result<Json<AiTagBatch>, AppError> {
    let kind = input.kind.as_deref().unwrap_or("tags");
    Ok(Json(ai_tags::enqueue_batch(&state.pool, input.ids, kind).await?))
}

/// `GET /ai-tags/batches` — recent batches (newest first).
pub async fn list_batches(
    State(state): State<AppState>,
) -> Result<Json<Vec<AiTagBatch>>, AppError> {
    let batches: Vec<AiTagBatch> = sqlx::query_as(
        "SELECT b.id, b.status, b.error_message, b.created_at, b.processed_at, b.kind,
                count(s.id)::bigint AS item_count
         FROM ai_tag_batches b
         LEFT JOIN ai_tag_suggestions s ON s.batch_id = b.id
         GROUP BY b.id, b.kind
         ORDER BY b.created_at DESC
         LIMIT 20",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(batches))
}

#[derive(Debug, Deserialize)]
pub struct SuggestionsQuery {
    pub batch_id: Option<Uuid>,
    /// Defaults to 'pending' (only reviewable suggestions).
    pub status: Option<String>,
}

/// `GET /ai-tags/suggestions` — suggestions joined with their items.
pub async fn list_suggestions(
    State(state): State<AppState>,
    Query(q): Query<SuggestionsQuery>,
) -> Result<Json<Vec<SuggestionDetail>>, AppError> {
    let status = q.status.unwrap_or_else(|| "pending".to_string());
    let suggestions = list_suggestions_query(&state.pool, q.batch_id, &status).await?;
    Ok(Json(suggestions))
}

pub async fn list_suggestions_query(
    pool: &PgPool,
    batch_id: Option<Uuid>,
    status: &str,
) -> Result<Vec<SuggestionDetail>, sqlx::Error> {
    sqlx::query_as::<_, SuggestionDetail>(sqlx::AssertSqlSafe(
        "SELECT s.id, s.batch_id, b.status AS batch_status, b.kind AS batch_kind,
                s.item_id, s.suggested_tags, s.suggested_category,
                s.status, s.created_at,
                i.merchant, i.description, i.amount_cents, i.occurred_on,
                i.category_id, c.name AS category_name, i.tags, i.document_id
         FROM ai_tag_suggestions s
         JOIN ai_tag_batches b ON b.id = s.batch_id
         JOIN items i ON i.id = s.item_id
         LEFT JOIN categories c ON c.id = i.category_id
         WHERE ($1::uuid IS NULL OR s.batch_id = $1)
           AND s.status = $2
         ORDER BY s.created_at, s.item_id"
    ))
    .bind(batch_id)
    .bind(status)
    .fetch_all(pool)
    .await
}

#[derive(Debug, Deserialize)]
pub struct ApplyInput {
    /// Optional override: the final tag list to add (as edited by the user).
    /// Omitted = use the stored `suggested_tags`.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// `POST /ai-tags/suggestions/{id}/apply`
pub async fn apply(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<ApplyInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(
        ai_tags::apply_suggestion(&state.pool, id, input.tags).await?,
    ))
}

/// `POST /ai-tags/suggestions/{id}/dismiss`
pub async fn dismiss(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(ai_tags::dismiss_suggestion(&state.pool, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct BatchScope {
    /// Optional: restrict to a single batch; omitted = all pending.
    pub batch_id: Option<Uuid>,
}

/// `POST /ai-tags/suggestions/apply-all`
pub async fn apply_all(
    State(state): State<AppState>,
    Json(input): Json<BatchScope>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(ai_tags::apply_all(&state.pool, input.batch_id).await?))
}

/// `POST /ai-tags/suggestions/dismiss-all`
pub async fn dismiss_all(
    State(state): State<AppState>,
    Json(input): Json<BatchScope>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(
        ai_tags::dismiss_all(&state.pool, input.batch_id).await?,
    ))
}
