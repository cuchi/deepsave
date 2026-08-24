use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{Category, NewCategory, UpdateCategory};
use crate::AppState;

const CATEGORY_COLS: &str = "id, parent_id, name, color, icon, is_active";

// Note: the `format!` output is composed only of constant column lists (never user
// input), so wrapping in `AssertSqlSafe` is safe.
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Category>>, AppError> {
    let cats = sqlx::query_as::<_, Category>(sqlx::AssertSqlSafe(format!(
        "SELECT {CATEGORY_COLS} FROM categories ORDER BY name"
    )))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(cats))
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<NewCategory>,
) -> Result<Json<Category>, AppError> {
    let cat = sqlx::query_as::<_, Category>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO categories (name, parent_id, color, icon)
         VALUES ($1, $2, $3, $4)
         RETURNING {CATEGORY_COLS}"
    )))
    .bind(&input.name)
    .bind(input.parent_id)
    .bind(&input.color)
    .bind(&input.icon)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(cat))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateCategory>,
) -> Result<Json<Category>, AppError> {
    let cat = sqlx::query_as::<_, Category>(sqlx::AssertSqlSafe(format!(
        "UPDATE categories
         SET name = $1, parent_id = $2, color = $3, icon = $4, is_active = $5
         WHERE id = $6
         RETURNING {CATEGORY_COLS}"
    )))
    .bind(&input.name)
    .bind(input.parent_id)
    .bind(&input.color)
    .bind(&input.icon)
    .bind(input.is_active)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("category not found"))?;
    Ok(Json(cat))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let res = sqlx::query("DELETE FROM categories WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("category not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
