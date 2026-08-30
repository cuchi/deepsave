//! Diary: the user's life notes (date + comment) injected into AI prompts so
//! the models can interpret spending context ("2025-09-01 - Me divorciei" →
//! why the "daiana" purchases make sense).

use anyhow::Result;
use chrono::NaiveDate;
use serde_json::Value;
use sqlx::PgPool;

/// Diary entries within a date range (for the digest: around the month).
pub async fn diary_range(
    pool: &PgPool,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<Vec<Value>> {
    let rows: Vec<(NaiveDate, String)> = sqlx::query_as(
        "SELECT entry_date, comment FROM diary_entries
         WHERE ($1::date IS NULL OR entry_date >= $1)
           AND ($2::date IS NULL OR entry_date <= $2)
         ORDER BY entry_date",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(d, c)| serde_json::json!({ "data": d, "nota": c }))
        .collect())
}

/// Most recent diary entries (for the tagging/categorization prompts).
pub async fn recent_diary(pool: &PgPool, limit: i64) -> Result<Vec<Value>> {
    let rows: Vec<(NaiveDate, String)> = sqlx::query_as(
        "SELECT entry_date, comment FROM diary_entries ORDER BY entry_date DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(d, c)| serde_json::json!({ "data": d, "nota": c }))
        .collect())
}
