use axum::extract::{Path, State};
use axum::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::RecurringRule;
use crate::services::recurring;
use crate::AppState;

const RECURRING_COLS: &str = "r.id, r.merchant, r.description, r.amount_cents, r.currency, \
     r.category_id, c.name AS category_name, r.frequency, r.interval, r.day_of_month, \
     r.next_due_on, r.is_active, r.source, r.created_at, r.updated_at";

const RECURRING_RETURNING: &str = "id, merchant, description, amount_cents, currency, \
     category_id, NULL::text AS category_name, frequency, interval, day_of_month, \
     next_due_on, is_active, source, created_at, updated_at";

fn default_interval() -> i32 {
    1
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct RecurringInput {
    #[serde(default)]
    pub merchant: Option<String>,
    pub description: String,
    /// Signed cents (negative = expense).
    pub amount_cents: i64,
    pub category_id: Option<Uuid>,
    /// 'weekly' | 'monthly' | 'yearly'
    pub frequency: String,
    #[serde(default = "default_interval")]
    pub interval: i32,
    pub day_of_month: Option<i32>,
    pub next_due_on: Option<NaiveDate>,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<RecurringRule>>, AppError> {
    let rules = sqlx::query_as::<_, RecurringRule>(sqlx::AssertSqlSafe(format!(
        "SELECT {RECURRING_COLS} FROM recurring_rules r
         LEFT JOIN categories c ON c.id = r.category_id
         ORDER BY r.is_active DESC, r.next_due_on ASC NULLS LAST, r.description"
    )))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rules))
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<RecurringInput>,
) -> Result<Json<RecurringRule>, AppError> {
    let rule = sqlx::query_as::<_, RecurringRule>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO recurring_rules
           (merchant, description, amount_cents, currency, category_id, frequency, interval,
            day_of_month, next_due_on, is_active, source)
         VALUES ($1, $2, $3, 'BRL', $4, $5, $6, $7, $8, $9, 'manual')
         RETURNING {RECURRING_RETURNING}"
    )))
    .bind(&input.merchant)
    .bind(&input.description)
    .bind(input.amount_cents)
    .bind(input.category_id)
    .bind(&input.frequency)
    .bind(input.interval)
    .bind(input.day_of_month)
    .bind(input.next_due_on)
    .bind(input.is_active)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(rule))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<RecurringInput>,
) -> Result<Json<RecurringRule>, AppError> {
    let rule = sqlx::query_as::<_, RecurringRule>(sqlx::AssertSqlSafe(format!(
        "UPDATE recurring_rules
         SET merchant = $1, description = $2, amount_cents = $3, category_id = $4,
             frequency = $5, interval = $6, day_of_month = $7, next_due_on = $8,
             is_active = $9, updated_at = now()
         WHERE id = $10
         RETURNING {RECURRING_RETURNING}"
    )))
    .bind(&input.merchant)
    .bind(&input.description)
    .bind(input.amount_cents)
    .bind(input.category_id)
    .bind(&input.frequency)
    .bind(input.interval)
    .bind(input.day_of_month)
    .bind(input.next_due_on)
    .bind(input.is_active)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("recurring rule not found"))?;
    Ok(Json(rule))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let res = sqlx::query("DELETE FROM recurring_rules WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("recurring rule not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Upcoming occurrences for active rules (within the next N months).
pub async fn upcoming(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let rows: Vec<(Uuid, Option<String>, String, i64, String, NaiveDate)> = sqlx::query_as(
        "SELECT id, merchant, description, amount_cents, frequency, next_due_on
         FROM recurring_rules
         WHERE is_active AND next_due_on IS NOT NULL
         ORDER BY next_due_on",
    )
    .fetch_all(&state.pool)
    .await?;

    let today = chrono::Utc::now().date_naive();
    let out = rows
        .into_iter()
        .map(|(id, merchant, description, amount_cents, frequency, next_due_on)| {
            json!({
                "id": id,
                "merchant": merchant,
                "description": description,
                "amount_cents": amount_cents,
                "frequency": frequency,
                "next_due_on": next_due_on,
                "days_until": (next_due_on - today).num_days(),
            })
        })
        .collect();
    Ok(Json(out))
}

/// Detection suggestions (not persisted).
pub async fn suggestions(
    State(state): State<AppState>,
) -> Result<Json<Vec<recurring::Suggestion>>, AppError> {
    Ok(Json(recurring::suggest(&state.pool).await?))
}
