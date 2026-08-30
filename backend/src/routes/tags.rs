use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::error::AppError;
use crate::models::TagUsage;
use crate::services::tags;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct RenameInput {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
pub struct MergeInput {
    pub from: String,
    pub into: String,
}

fn normalize_tag(s: &str) -> Result<String, AppError> {
    tags::normalize_one(s).ok_or_else(|| AppError::bad_request("tag cannot be empty"))
}

/// Distinct normalized tags across all items (for autocomplete + filters).
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<String>>, AppError> {
    let tags: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT tag FROM items CROSS JOIN LATERAL unnest(tags) AS tag ORDER BY tag",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(tags))
}

/// Usage counts per tag (for the tag management page).
pub async fn usage(State(state): State<AppState>) -> Result<Json<Vec<TagUsage>>, AppError> {
    let usage = tags::usage(&state.pool).await?;
    Ok(Json(usage))
}

/// Registry entry: a tag with its user-written description ('' when none).
#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct TagRegistryEntry {
    pub name: String,
    pub description: String,
}

/// `GET /api/tags/registry` — every tag in use (items + merchant memory) with its
/// description, for the management UI and AI prompt context.
pub async fn registry(State(state): State<AppState>) -> Result<Json<Vec<TagRegistryEntry>>, AppError> {
    let rows: Vec<TagRegistryEntry> = sqlx::query_as::<_, TagRegistryEntry>(
        "WITH used AS (
           SELECT tag FROM items CROSS JOIN LATERAL unnest(tags) AS tag
         )
         SELECT u.tag AS name, COALESCE(t.description, '') AS description
         FROM used u LEFT JOIN tags t ON t.name = u.tag
         ORDER BY u.tag",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct DescriptionInput {
    pub description: String,
}

/// `PATCH /api/tags/{tag}` — set the description of a tag (created on demand).
pub async fn set_description(
    State(state): State<AppState>,
    Path(tag): Path<String>,
    Json(input): Json<DescriptionInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tag = normalize_tag(&tag)?;
    tags::upsert_description(&state.pool, &tag, input.description.trim()).await?;
    Ok(Json(json!({ "ok": true, "tag": tag, "description": input.description.trim() })))
}

async fn apply_rename(
    state: &AppState,
    from: &str,
    to: &str,
) -> Result<Json<serde_json::Value>, AppError> {
    let from = normalize_tag(from)?;
    let to = normalize_tag(to)?;
    if from == to {
        return Err(AppError::bad_request("new tag name must differ from the current one"));
    }
    let res = tags::rename(&state.pool, &from, &to).await?;
    Ok(Json(json!({
        "ok": true,
        "items_updated": res.items_updated,
    })))
}

/// Rename a tag everywhere; if the new name already exists on a row, tags merge.
pub async fn rename(
    State(state): State<AppState>,
    Json(input): Json<RenameInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    apply_rename(&state, &input.from, &input.to).await
}

/// Merge a tag into another (the source disappears everywhere).
pub async fn merge(
    State(state): State<AppState>,
    Json(input): Json<MergeInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    apply_rename(&state, &input.from, &input.into).await
}

/// Delete a tag from all items (and recurring rules / merchant memory).
pub async fn delete_tag(
    State(state): State<AppState>,
    Path(tag): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tag = normalize_tag(&tag)?;
    let res = tags::remove(&state.pool, &tag).await?;
    Ok(Json(json!({
        "ok": true,
        "items_updated": res.items_updated,
    })))
}
