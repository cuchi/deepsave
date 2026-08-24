use axum::extract::{Path, State};
use axum::Json;
use chrono::{Datelike, Months, NaiveDate};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::Source;
use crate::AppState;

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Source>>, AppError> {
    let sources = sqlx::query_as::<_, Source>(
        "SELECT id, bank, kind, name, enabled, account_id, sort_order, created_at
         FROM sources ORDER BY sort_order",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(sources))
}

#[derive(Debug, Deserialize)]
pub struct SourceUpdate {
    pub name: Option<String>,
    pub enabled: Option<bool>,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<SourceUpdate>,
) -> Result<Json<Source>, AppError> {
    let source = sqlx::query_as::<_, Source>(
        "UPDATE sources
         SET name = COALESCE($1, name), enabled = COALESCE($2, enabled)
         WHERE id = $3
         RETURNING id, bank, kind, name, enabled, account_id, sort_order, created_at",
    )
    .bind(&input.name)
    .bind(input.enabled)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("source not found"))?;
    Ok(Json(source))
}

/// Coverage matrix: for each source, which of the last N months have data.
pub async fn coverage(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let n = state.coverage_months.max(1);
    let current = {
        let today = chrono::Utc::now().date_naive();
        NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap()
    };
    let start = current - Months::new(n as u32 - 1);

    let months: Vec<String> = (0..n)
        .rev()
        .map(|k| (current - Months::new(k)).format("%Y-%m").to_string())
        .collect();

    let sources: Vec<Source> = sqlx::query_as::<_, Source>(
        "SELECT id, bank, kind, name, enabled, account_id, sort_order, created_at
         FROM sources ORDER BY sort_order",
    )
    .fetch_all(&state.pool)
    .await?;

    let mut out = Vec::with_capacity(sources.len());
    for s in &sources {
        let present: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT to_char(i.occurred_on, 'YYYY-MM')
             FROM items i
             JOIN documents d ON d.id = i.document_id
             WHERE d.source_id = $1 AND i.occurred_on >= $2
             ORDER BY 1",
        )
        .bind(s.id)
        .bind(start)
        .fetch_all(&state.pool)
        .await?;

        let last_seen: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT max(d.uploaded_at) FROM documents d WHERE d.source_id = $1",
        )
        .bind(s.id)
        .fetch_one(&state.pool)
        .await?;

        out.push(json!({
            "id": s.id,
            "name": s.name,
            "bank": s.bank,
            "kind": s.kind,
            "enabled": s.enabled,
            "present": present,
            "last_seen": last_seen,
        }));
    }

    Ok(Json(json!({ "months": months, "sources": out })))
}
