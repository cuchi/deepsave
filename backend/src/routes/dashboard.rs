use axum::extract::{Query, State};
use axum::Json;
use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct DashboardQuery {
    /// YYYY-MM, optional (defaults to the current month).
    pub month: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TrendQuery {
    pub months: Option<i32>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CategoryTotal {
    pub category_id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub total_cents: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct MerchantTotal {
    pub merchant: String,
    pub total_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct Dashboard {
    pub month: String,
    pub total_spend_cents: i64,
    pub total_income_cents: i64,
    pub by_category: Vec<CategoryTotal>,
    pub top_merchants: Vec<MerchantTotal>,
    pub pending_count: i64,
}

#[derive(Debug, Serialize)]
pub struct TrendPoint {
    pub month: String,
    pub spend_cents: i64,
    pub income_cents: i64,
}

pub async fn dashboard(
    State(state): State<AppState>,
    Query(q): Query<DashboardQuery>,
) -> Result<Json<Dashboard>, AppError> {
    let (start, end, month) = resolve_month(q.month.as_deref())?;

    let (spend, income): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(CASE WHEN kind = 'expense' THEN -amount_cents ELSE 0 END), 0)::bigint,
                COALESCE(SUM(CASE WHEN kind = 'income' THEN amount_cents ELSE 0 END), 0)::bigint
         FROM items
         WHERE status != 'rejected' AND occurred_on >= $1 AND occurred_on < $2",
    )
    .bind(start)
    .bind(end)
    .fetch_one(&state.pool)
    .await?;

    let by_category: Vec<CategoryTotal> = sqlx::query_as(
        "SELECT c.id AS category_id, c.name AS name, c.color AS color,
                COALESCE(SUM(-i.amount_cents), 0)::bigint AS total_cents
         FROM items i
         JOIN categories c ON c.id = i.category_id
         WHERE i.kind = 'expense' AND i.status != 'rejected'
           AND i.occurred_on >= $1 AND i.occurred_on < $2
         GROUP BY c.id, c.name, c.color
         ORDER BY total_cents DESC",
    )
    .bind(start)
    .bind(end)
    .fetch_all(&state.pool)
    .await?;

    let top_merchants: Vec<MerchantTotal> = sqlx::query_as(
        "SELECT merchant, SUM(-amount_cents)::bigint AS total_cents
         FROM items
         WHERE kind = 'expense' AND status != 'rejected' AND merchant IS NOT NULL
           AND occurred_on >= $1 AND occurred_on < $2
         GROUP BY merchant
         ORDER BY total_cents DESC
         LIMIT 10",
    )
    .bind(start)
    .bind(end)
    .fetch_all(&state.pool)
    .await?;

    let pending_count: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE status = 'pending_review'")
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(Dashboard {
        month,
        total_spend_cents: spend,
        total_income_cents: income,
        by_category,
        top_merchants,
        pending_count,
    }))
}

pub async fn trend(
    State(state): State<AppState>,
    Query(q): Query<TrendQuery>,
) -> Result<Json<Vec<TrendPoint>>, AppError> {
    let months = q.months.unwrap_or(12).clamp(1, 36);
    let start = month_start_months_ago(months - 1);

    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT to_char(occurred_on, 'YYYY-MM'),
                COALESCE(SUM(CASE WHEN kind = 'expense' THEN -amount_cents ELSE 0 END), 0)::bigint,
                COALESCE(SUM(CASE WHEN kind = 'income' THEN amount_cents ELSE 0 END), 0)::bigint
         FROM items
         WHERE status != 'rejected' AND occurred_on >= $1
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(start)
    .fetch_all(&state.pool)
    .await?;

    let mut map = std::collections::HashMap::new();
    for (m, s, i) in rows {
        map.insert(m, (s, i));
    }

    let mut out = Vec::with_capacity(months as usize);
    for k in 0..months {
        let d = start + Months::new(k as u32);
        let key = d.format("%Y-%m").to_string();
        let (s, i) = map.get(&key).copied().unwrap_or((0, 0));
        out.push(TrendPoint {
            month: key,
            spend_cents: s,
            income_cents: i,
        });
    }

    Ok(Json(out))
}

fn resolve_month(month: Option<&str>) -> Result<(NaiveDate, NaiveDate, String), AppError> {
    let now = chrono::Utc::now().date_naive();
    let (year, month_num) = match month {
        Some(m) => {
            let (y, mo) = m
                .split_once('-')
                .ok_or_else(|| AppError::bad_request("month must be YYYY-MM"))?;
            let y: i32 = y
                .parse()
                .map_err(|_| AppError::bad_request("invalid month"))?;
            let mo: u32 = mo
                .parse()
                .map_err(|_| AppError::bad_request("invalid month"))?;
            (y, mo)
        }
        None => (now.year(), now.month()),
    };
    if !(1..=12).contains(&month_num) {
        return Err(AppError::bad_request("invalid month"));
    }
    let start = NaiveDate::from_ymd_opt(year, month_num, 1)
        .ok_or_else(|| AppError::bad_request("invalid month"))?;
    let end = if month_num == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month_num + 1, 1)
    }
    .ok_or_else(|| AppError::bad_request("invalid month"))?;
    Ok((start, end, format!("{year:04}-{month_num:02}")))
}

fn month_start_months_ago(n: i32) -> NaiveDate {
    let today = chrono::Utc::now().date_naive();
    let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    start - Months::new(n as u32)
}
