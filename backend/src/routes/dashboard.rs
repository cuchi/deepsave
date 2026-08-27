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
    /// Comma-separated category ids (OR).
    pub category_ids: Option<String>,
    pub kind: Option<String>,
    /// Comma-separated tags (OR: item carries any).
    pub tags: Option<String>,
    pub bank: Option<String>,
    pub installments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TrendQuery {
    pub months: Option<i32>,
    pub date_to: Option<NaiveDate>,
    pub search: Option<String>,
    /// Comma-separated category ids (OR).
    pub category_ids: Option<String>,
    pub kind: Option<String>,
    /// Comma-separated tags (OR: item carries any).
    pub tags: Option<String>,
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
/// date_to, search, category_ids, kind, tags, bank, installments. Unlike the
/// items list, rejected items are always excluded (they're not real activity).
const AGG_FILTERS: &str = "
    ($1::date IS NULL OR occurred_on >= $1)
    AND ($2::date IS NULL OR occurred_on <= $2)
    AND status != 'rejected'
    AND ($3::text IS NULL
         OR description ILIKE '%' || $3 || '%'
         OR COALESCE(merchant, '') ILIKE '%' || $3 || '%'
         OR array_to_string(tags, ' ') ILIKE '%' || $3 || '%')
    AND (cardinality($4) = 0
         OR category_id::text = ANY($4)
         OR ('__none' = ANY($4) AND category_id IS NULL))
    AND ($5::text IS NULL OR kind = $5)
    AND (cardinality($6) = 0
         OR tags && $6
         OR ('__none' = ANY($6) AND cardinality(tags) = 0))
    AND ($7::text IS NULL OR EXISTS (
          SELECT 1 FROM documents d
          JOIN sources s ON s.id = d.source_id
          WHERE d.id = items.document_id AND s.bank = $7))
    AND ($8::text IS NULL OR $8 = 'all'
         OR ($8 = 'first_only' AND NOT (COALESCE(installment_count, 0) > 1 AND COALESCE(installment, 0) > 1))
         OR ($8 = 'only' AND COALESCE(installment_count, 0) > 1))
";

/// When the installments filter is 'first_only', the first parcel stands in for
/// the whole purchase (parcel × count). References $8 (the installments param).
const AGG_AMOUNT_ADJ: &str = "
    CASE WHEN $8 = 'first_only' AND installment_count > 1 AND installment = 1
         THEN amount_cents * installment_count
         ELSE amount_cents END";

/// Resolve the aggregation window: explicit date range wins; else `month`;
/// else no date filter (all history). The UI pre-fills the last complete month
/// on first load, but the API itself must not — "cleared dates" means "tudo".
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

    let Some(m) = month else {
        return Ok((None, None, "tudo".to_string()));
    };
    let (y, mo) = m
        .split_once('-')
        .ok_or_else(|| AppError::bad_request("month must be YYYY-MM"))?;
    let year: i32 = y
        .parse()
        .map_err(|_| AppError::bad_request("invalid month"))?;
    let month_num: u32 = mo
        .parse()
        .map_err(|_| AppError::bad_request("invalid month"))?;
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
    .ok_or_else(|| AppError::bad_request("invalid month"))?
        - chrono::Duration::days(1);
    Ok((Some(start), Some(end), format!("{year:04}-{month_num:02}")))
}

/// Parse a comma-separated filter list (see `routes::items::split_filters`).
pub(crate) fn split_filters(s: &Option<String>) -> Vec<String> {
    s.as_deref()
        .map(|v| {
            v.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default()
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
    let category_ids = split_filters(&q.category_ids);
    let tags = split_filters(&q.tags);
    let (spend, income): (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT COALESCE(SUM(CASE WHEN kind = 'expense' THEN -{AGG_AMOUNT_ADJ} ELSE 0 END), 0)::bigint,
                COALESCE(SUM(CASE WHEN kind = 'income' THEN {AGG_AMOUNT_ADJ} ELSE 0 END), 0)::bigint
         FROM items
         WHERE parent_id IS NULL AND {AGG_FILTERS}"
    )))
    .bind(from)
    .bind(to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_one(pool)
    .await?;

    let by_category: Vec<CategoryTotal> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT c.id AS category_id, c.name AS name, c.color AS color,
                COALESCE(SUM(-{AGG_AMOUNT_ADJ}), 0)::bigint AS total_cents
         FROM items
         JOIN categories c ON c.id = items.category_id
         WHERE items.parent_id IS NULL AND items.kind = 'expense' AND {AGG_FILTERS}
         GROUP BY c.id, c.name, c.color
         ORDER BY total_cents DESC"
    )))
    .bind(from)
    .bind(to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
    .await?;

    let top_merchants: Vec<MerchantTotal> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT merchant, SUM(-{AGG_AMOUNT_ADJ})::bigint AS total_cents
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
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
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
    let category_ids = split_filters(&q.category_ids);
    let tags = split_filters(&q.tags);

    let today = chrono::Utc::now().date_naive();
    let end_month = match q.date_to {
        Some(d) => NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap(),
        None => NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(),
    };
    let start = end_month - Months::new((months - 1) as u32);
    let end = end_month + Months::new(1) - chrono::Duration::days(1); // inclusive last day

    let rows: Vec<(String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(occurred_on, 'YYYY-MM'),
                COALESCE(SUM(CASE WHEN kind = 'expense' THEN -{AGG_AMOUNT_ADJ} ELSE 0 END), 0)::bigint,
                COALESCE(SUM(CASE WHEN kind = 'income' THEN {AGG_AMOUNT_ADJ} ELSE 0 END), 0)::bigint
         FROM items
         WHERE parent_id IS NULL AND {AGG_FILTERS}
         GROUP BY 1
         ORDER BY 1"
    )))
    .bind(start)
    .bind(end)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
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

// ---------- Daily / tags (expenses only) ----------

/// Filters for the daily and top-tags aggregations (no month param — the
/// frontend passes an explicit range; None = unbounded).
#[derive(Debug, Deserialize)]
pub struct DailyQuery {
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
    pub search: Option<String>,
    pub category_ids: Option<String>,
    pub kind: Option<String>,
    pub tags: Option<String>,
    pub bank: Option<String>,
    pub installments: Option<String>,
    /// 'category' (per day per category) or 'none' (plain daily totals).
    #[serde(default = "default_stack_by")]
    pub stack_by: String,
}

fn default_stack_by() -> String {
    "category".to_string()
}

#[derive(Debug, Serialize, FromRow)]
pub struct DailyPoint {
    pub date: NaiveDate,
    /// Category name when `stack_by=category`, else NULL.
    pub key: Option<String>,
    pub total_cents: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct TagTotal {
    pub tag: String,
    pub total_cents: i64,
}

pub async fn daily(
    State(state): State<AppState>,
    Query(q): Query<DailyQuery>,
) -> Result<Json<Vec<DailyPoint>>, AppError> {
    Ok(Json(daily_data(&state.pool, &q).await?))
}

/// Daily expense totals (roots only, rejected excluded). `stack_by=category`
/// buckets each day by category name; `none` returns one row per day. The `kind`
/// filter still applies on top of the hard `kind = 'expense'` (so a kind=income
/// filter yields an empty chart, which is honest feedback).
pub async fn daily_data(pool: &PgPool, q: &DailyQuery) -> Result<Vec<DailyPoint>, AppError> {
    let category_ids = split_filters(&q.category_ids);
    let tags = split_filters(&q.tags);
    let rows = sqlx::query_as::<_, DailyPoint>(sqlx::AssertSqlSafe(format!(
        "SELECT items.occurred_on AS date,
                CASE WHEN $9 = 'category' THEN COALESCE(c.name, 'Sem categoria') END AS key,
                SUM(-{AGG_AMOUNT_ADJ})::bigint AS total_cents
         FROM items
         LEFT JOIN categories c ON c.id = items.category_id
         WHERE items.parent_id IS NULL AND items.kind = 'expense' AND {AGG_FILTERS}
         GROUP BY items.occurred_on, key
         ORDER BY items.occurred_on, key"
    )))
    .bind(q.date_from)
    .bind(q.date_to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .bind(&q.stack_by)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn tags(
    State(state): State<AppState>,
    Query(q): Query<DailyQuery>,
) -> Result<Json<Vec<TagTotal>>, AppError> {
    Ok(Json(tags_data(&state.pool, &q).await?))
}

/// Top tags by expense total. Each tag counts the FULL amount of its items
/// (overlap allowed — that's the point: "spend carrying this tag").
pub async fn tags_data(pool: &PgPool, q: &DailyQuery) -> Result<Vec<TagTotal>, AppError> {
    let category_ids = split_filters(&q.category_ids);
    let tags = split_filters(&q.tags);
    let rows = sqlx::query_as::<_, TagTotal>(sqlx::AssertSqlSafe(format!(
        "SELECT tag, SUM(-{AGG_AMOUNT_ADJ})::bigint AS total_cents
         FROM items CROSS JOIN LATERAL unnest(items.tags) AS tag
         WHERE items.parent_id IS NULL AND items.kind = 'expense' AND {AGG_FILTERS}
         GROUP BY tag
         ORDER BY total_cents DESC
         LIMIT 10"
    )))
    .bind(q.date_from)
    .bind(q.date_to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------- Expected spend (future forecast) ----------

#[derive(Debug, Deserialize)]
pub struct ExpectedQuery {
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct ExpectedSpend {
    pub installments_cents: i64,
    pub recurring_cents: i64,
    pub total_cents: i64,
}

/// `GET /dashboard/expected` — expected spend for a period: future installments
/// of in-progress purchase series + future occurrences of active recurring
/// rules. Only dated `>= today` (what's still expected); expenses only.
pub async fn expected(
    State(state): State<AppState>,
    Query(q): Query<ExpectedQuery>,
) -> Result<Json<ExpectedSpend>, AppError> {
    Ok(Json(expected_data(&state.pool, q.date_from, q.date_to).await?))
}

pub async fn expected_data(
    pool: &PgPool,
    date_from: Option<NaiveDate>,
    date_to: Option<NaiveDate>,
) -> Result<ExpectedSpend, AppError> {
    let today = chrono::Utc::now().date_naive();
    let from = date_from.map(|d| d.max(today)).unwrap_or(today);
    let to = date_to.unwrap_or(today);
    if to < from {
        return Ok(ExpectedSpend {
            installments_cents: 0,
            recurring_cents: 0,
            total_cents: 0,
        });
    }

    let installments_cents = expected_installments(pool, from, to).await?;
    let recurring_cents = expected_recurring(pool, from, to, today).await?;
    Ok(ExpectedSpend {
        installments_cents,
        recurring_cents,
        total_cents: installments_cents + recurring_cents,
    })
}

/// Future parcels of in-progress series: parcel k+1 in month M+1 after the
/// latest billed parcel (month M). Uses the latest parcel amount.
async fn expected_installments(pool: &PgPool, from: NaiveDate, to: NaiveDate) -> Result<i64, AppError> {
    let rows: Vec<(Uuid, i32, i32, NaiveDate, i64)> = sqlx::query_as(
        "SELECT s.id, s.installment_count, st.max_inst, st.last_date, p.amount_cents
         FROM purchase_series s
         JOIN LATERAL (
           SELECT MAX(i.installment)::int AS max_inst, MAX(i.occurred_on) AS last_date
           FROM items i WHERE i.series_id = s.id
         ) st ON true
         JOIN LATERAL (
           SELECT i.amount_cents FROM items i
           WHERE i.series_id = s.id ORDER BY i.installment DESC, i.occurred_on DESC LIMIT 1
         ) p ON true
         WHERE st.max_inst IS NOT NULL AND st.max_inst < s.installment_count",
    )
    .fetch_all(pool)
    .await?;

    let mut total = 0i64;
    for (_sid, count, max_inst, last_date, amount_cents) in rows {
        if amount_cents >= 0 {
            continue;
        }
        for k in (max_inst + 1)..=count {
            let months = (k - max_inst) as u32;
            let Some(d) = last_date.checked_add_months(chrono::Months::new(months)) else {
                continue;
            };
            if d >= from && d <= to {
                total += amount_cents.unsigned_abs() as i64;
            }
        }
    }
    Ok(total)
}

/// Future occurrences of active expense rules, anchored at `next_due_on`
/// (advanced to >= today), stepped by the rule's window.
async fn expected_recurring(pool: &PgPool, from: NaiveDate, to: NaiveDate, today: NaiveDate) -> Result<i64, AppError> {
    let rows: Vec<(i64, String, i32, NaiveDate)> = sqlx::query_as(
        "SELECT amount_cents, frequency, interval, next_due_on
         FROM recurring_rules WHERE is_active AND amount_cents < 0 AND next_due_on IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let mut total = 0i64;
    for (amount_cents, frequency, interval, next_due) in rows {
        let mut d = crate::services::recurring::advance_next_due(next_due, &frequency, interval, today);
        for _ in 0..1200 {
            if d > to {
                break;
            }
            if d >= from {
                total += amount_cents.unsigned_abs() as i64;
            }
            d = step_occurrence(d, &frequency, interval);
        }
    }
    Ok(total)
}

fn step_occurrence(d: NaiveDate, frequency: &str, interval: i32) -> NaiveDate {
    let interval = interval.max(1) as u32;
    match frequency {
        "weekly" => d + chrono::Duration::days(7 * interval as i64),
        "monthly" => d.checked_add_months(chrono::Months::new(interval)).unwrap_or(d),
        _ => d.checked_add_months(chrono::Months::new(interval * 12)).unwrap_or(d),
    }
}
