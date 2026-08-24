use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{Item, MatchDetail};
use crate::routes::items::ITEM_COLS;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct MatchQuery {
    /// Filter by status, optional (default: all).
    pub status: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<MatchQuery>,
) -> Result<Json<Vec<MatchDetail>>, AppError> {
    let rows: Vec<(Uuid, Uuid, Uuid, String, f32, String)> = sqlx::query_as(
        "SELECT id, parent_item_id, child_item_id, source, confidence, status
         FROM matches
         WHERE ($1::text IS NULL OR status = $1)
         ORDER BY confidence DESC, created_at DESC",
    )
    .bind(q.status)
    .fetch_all(&state.pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for (id, parent_item_id, child_item_id, source, confidence, status) in rows {
        let parent = fetch_item(&state.pool, parent_item_id).await?;
        let child = fetch_item(&state.pool, child_item_id).await?;
        out.push(MatchDetail {
            id,
            parent_item_id,
            child_item_id,
            source,
            confidence,
            status,
            parent,
            child,
        });
    }
    Ok(Json(out))
}

pub async fn accept(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        "UPDATE matches SET status = 'accepted'
         WHERE id = $1
         RETURNING parent_item_id, child_item_id",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((parent_id, child_id)) = row else {
        return Err(AppError::not_found("match not found"));
    };

    sqlx::query("UPDATE items SET parent_id = $1, updated_at = now() WHERE id = $2")
        .bind(parent_id)
        .bind(child_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn reject(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let res = sqlx::query("UPDATE matches SET status = 'rejected' WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("match not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Manually (re)run the linking heuristic for all unlinked receipt items.
pub async fn suggest(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let suggested = crate::services::linking::suggest_links_all(&state.pool).await?;
    Ok(Json(json!({ "suggested": suggested })))
}

async fn fetch_item(pool: &PgPool, id: Uuid) -> Result<Item, AppError> {
    sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(format!(
        "SELECT {ITEM_COLS} FROM items WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("item not found"))
}
