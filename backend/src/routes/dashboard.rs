use axum::extract::{Query, State};
use axum::Json;
use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct DashboardQuery {
    /// YYYY-MM, optional (used only when no date range is given).
    pub month: Option<String>,
    // Same filters as the items list.
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
    pub search: Option<String>,
    pub category_id: Option<Uuid>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub bank: Option<String>,
    pub installments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TrendQuery {
    pub months: Option<i32>,
    pub date_to: Option<NaiveDate>,
    pub search: Option<String>,
    pub category_id: Option<Uuid>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub bank: Option<String>,
    pub installments: Option<String>,
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
    /// Range label ("YYYY-MM-DD..YYYY-MM-DD" or "YYYY-MM").
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

/// Shared WHERE for dashboard/trend aggregation. Params $1..$8: date_from,
/// date_to, search, category_id, kind, tag, bank, installments. Unlike the items
/// list, rejected items are always excluded (they're not real activity).
const AGG_FILTERS: &str = "
    ($1::date IS NULL OR occurred_on >= $1)
    AND ($2::date IS NULL OR occurred_on <= $2)
    AND status != 'rejected'
    AND ($3::text IS NULL
         OR description ILIKE '%' || $3 || '%'
         OR COALESCE(merchant, '') ILIKE '%' || $3 || '%'
         OR array_to_string(tags, ' ') ILIKE '%' || $3 || '%')
    AND ($4::uuid IS NULL OR category_id = $4)
    AND ($5::text IS NULL OR kind = $5)
    AND ($6::text IS NULL OR $6 = ANY(tags))
    AND ($7::text IS NULL OR EXISTS (
          SELECT 1 FROM documents d
          JOIN sources s ON s.id = d.source_id
          WHERE d.id = items.document_id AND s.bank = $7))
    AND ($8::text IS NULL OR $8 = 'all'
         OR ($8 = 'first_only' AND NOT (COALESCE(installment_count, 0) > 1 AND COALESCE(installment, 0) > 1))
         OR ($8 = 'only' AND COALESCE(installment_count, 0) > 1))
";

/// Resolve the aggregation window: explicit date range wins; else `month`;
/// else the last complete calendar month (matches the default in the UI).
fn resolve_range(
    date_from: Option<NaiveDate>,
    date_to: Option<NaiveDate>,
    month: Option<&str>,
) -> Result<(Option<NaiveDate>, Option<NaiveDate>, String), AppError> {
    if let (Some(from), Some(to)) = (date_from, date_to) {
        if from > to {
            return Err(AppError::bad_request("date_from must not be after date_to"));
        }
        return Ok((Some(from), Some(to), format!("{from}..{to}")));
    }

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
            if !(1..=12).contains(&mo) {
                return Err(AppError::bad_request("invalid month"));
            }
            (y, mo)
        }
        None => {
            let (start, _) = last_complete_month();
            (start.year(), start.month())
        }
    };

    let start = NaiveDate::from_ymd_opt(year, month_num, 1)
        .ok_or_else(|| AppError::bad_request("invalid month"))?;
    let end = if month_num == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month_num + 1, 1)
    }
    .ok_or_else(|| AppError::bad_request("invalid month"))?
        - chrono::Duration::days(1);
    Ok((Some(start), Some(end), format!("{year:04}-{month_num:02}")))
}

/// First and last day of the previous calendar month.
fn last_complete_month() -> (NaiveDate, NaiveDate) {
    let today = chrono::Utc::now().date_naive();
    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let start = first - Months::new(1);
    let end = first - chrono::Duration::days(1);
    (start, end)
}

pub async fn dashboard(
    State(state): State<AppState>,
    Query(q): Query<DashboardQuery>,
) -> Result<Json<Dashboard>, AppError> {
    Ok(Json(dashboard_data(&state.pool, &q).await?))
}

/// Core aggregation, kept pool-level so integration tests can drive it without
/// an `AppState`. Sums **root** items only (receipt children are allocations of
/// their parent and would double-count).
pub async fn dashboard_data(pool: &PgPool, q: &DashboardQuery) -> Result<Dashboard, AppError> {
    let (from, to, label) = resolve_range(q.date_from, q.date_to, q.month.as_deref())?;

    let (spend, income): (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT COALESCE(SUM(CASE WHEN kind = 'expense' THEN -amount_cents ELSE 0 END), 0)::bigint,
                COALESCE(SUM(CASE WHEN kind = 'income' THEN amount_cents ELSE 0 END), 0)::bigint
         FROM items
         WHERE parent_id IS NULL AND {AGG_FILTERS}"
    )))
    .bind(from)
    .bind(to)
    .bind(&q.search)
    .bind(q.category_id)
    .bind(&q.kind)
    .bind(&q.tag)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_one(pool)
    .await?;

    let by_category: Vec<CategoryTotal> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT c.id AS category_id, c.name AS name, c.color AS color,
                COALESCE(SUM(-items.amount_cents), 0)::bigint AS total_cents
         FROM items
         JOIN categories c ON c.id = items.category_id
         WHERE items.parent_id IS NULL AND items.kind = 'expense' AND {AGG_FILTERS}
         GROUP BY c.id, c.name, c.color
         ORDER BY total_cents DESC"
    )))
    .bind(from)
    .bind(to)
    .bind(&q.search)
    .bind(q.category_id)
    .bind(&q.kind)
    .bind(&q.tag)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
    .await?;

    let top_merchants: Vec<MerchantTotal> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT merchant, SUM(-items.amount_cents)::bigint AS total_cents
         FROM items
         WHERE items.parent_id IS NULL AND items.kind = 'expense' AND merchant IS NOT NULL
           AND {AGG_FILTERS}
         GROUP BY merchant
         ORDER BY total_cents DESC
         LIMIT 10"
    )))
    .bind(from)
    .bind(to)
    .bind(&q.search)
    .bind(q.category_id)
    .bind(&q.kind)
    .bind(&q.tag)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
    .await?;

    let pending_count: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE status = 'pending_review'")
        .fetch_one(pool)
        .await?;

    Ok(Dashboard {
        month: label,
        total_spend_cents: spend,
        total_income_cents: income,
        by_category,
        top_merchants,
        pending_count,
    })
}

pub async fn trend(
    State(state): State<AppState>,
    Query(q): Query<TrendQuery>,
) -> Result<Json<Vec<TrendPoint>>, AppError> {
    Ok(Json(trend_data(&state.pool, &q).await?))
}

/// 12-month (configurable) spend/income history. The window ends at the month of
/// `date_to` (default: current month) and respects the non-date filters. `from`
/// is NOT used — the trend is always a rolling window, so it shows history even
/// when the range is a single month.
pub async fn trend_data(pool: &PgPool, q: &TrendQuery) -> Result<Vec<TrendPoint>, AppError> {
    let months = q.months.unwrap_or(12).clamp(1, 36);

    let today = chrono::Utc::now().date_naive();
    let end_month = match q.date_to {
        Some(d) => NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap(),
        None => NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(),
    };
    let start = end_month - Months::new((months - 1) as u32);
    let end = end_month + Months::new(1) - chrono::Duration::days(1); // inclusive last day

    let rows: Vec<(String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(occurred_on, 'YYYY-MM'),
                COALESCE(SUM(CASE WHEN kind = 'expense' THEN -amount_cents ELSE 0 END), 0)::bigint,
                COALESCE(SUM(CASE WHEN kind = 'income' THEN amount_cents ELSE 0 END), 0)::bigint
         FROM items
         WHERE parent_id IS NULL AND {AGG_FILTERS}
         GROUP BY 1
         ORDER BY 1"
    )))
    .bind(start)
    .bind(end)
    .bind(&q.search)
    .bind(q.category_id)
    .bind(&q.kind)
    .bind(&q.tag)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
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

    Ok(out)
}
