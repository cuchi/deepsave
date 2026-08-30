use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;

use crate::error::AppError;
use crate::models::{DiaryEntry, DiaryInput};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct DiaryQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

/// `GET /api/diary` — diary entries (newest first).
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<DiaryQuery>,
) -> Result<Json<Vec<DiaryEntry>>, AppError> {
    let limit = q.limit.clamp(1, 500);
    let rows: Vec<DiaryEntry> = sqlx::query_as(
        "SELECT id, entry_date, comment, created_at FROM diary_entries
         WHERE ($1::date IS NULL OR entry_date >= $1)
           AND ($2::date IS NULL OR entry_date <= $2)
         ORDER BY entry_date DESC LIMIT $3",
    )
    .bind(q.from)
    .bind(q.to)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

/// `POST /api/diary` — add an entry.
pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<DiaryInput>,
) -> Result<Json<DiaryEntry>, AppError> {
    if input.comment.trim().is_empty() {
        return Err(AppError::bad_request("comentário não pode ser vazio"));
    }
    let row: DiaryEntry = sqlx::query_as(
        "INSERT INTO diary_entries (entry_date, comment) VALUES ($1, $2)
         RETURNING id, entry_date, comment, created_at",
    )
    .bind(input.entry_date)
    .bind(input.comment.trim())
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

/// `PATCH /api/diary/{id}` — edit an entry.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<DiaryInput>,
) -> Result<Json<DiaryEntry>, AppError> {
    if input.comment.trim().is_empty() {
        return Err(AppError::bad_request("comentário não pode ser vazio"));
    }
    let row: Option<DiaryEntry> = sqlx::query_as(
        "UPDATE diary_entries SET entry_date = $1, comment = $2 WHERE id = $3
         RETURNING id, entry_date, comment, created_at",
    )
    .bind(input.entry_date)
    .bind(input.comment.trim())
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.ok_or_else(|| AppError::not_found("entry not found")).map(Json)
}

/// `DELETE /api/diary/{id}`.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let res = sqlx::query("DELETE FROM diary_entries WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("entry not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
