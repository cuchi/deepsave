//! Shared query-param helpers for the JSON API: comma-separated filter lists
//! and `YYYY-MM` parsing. Kept here so the items list and the dashboard
//! aggregations parse filters/periods identically.

use chrono::NaiveDate;

use crate::error::AppError;

/// Parse a comma-separated filter list (category ids or tags). Values are kept
/// as text so the special `__none` sentinel ("no category"/"no tags") can live
/// in the same array; the SQL compares via `category_id::text` / `tags &&`.
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

/// Parse a `YYYY-MM` string into `(first_day, first_day_of_next_month)`.
/// The end is **exclusive** — callers wanting an inclusive month end subtract a day.
pub(crate) fn parse_month(month: &str) -> Result<(NaiveDate, NaiveDate), AppError> {
    let (y, mo) = month
        .split_once('-')
        .ok_or_else(|| AppError::bad_request("month must be YYYY-MM"))?;
    let year: i32 = y.parse().map_err(|_| AppError::bad_request("invalid month"))?;
    let month_num: u32 = mo.parse().map_err(|_| AppError::bad_request("invalid month"))?;
    if !(1..=12).contains(&month_num) {
        return Err(AppError::bad_request("invalid month"));
    }
    let start = NaiveDate::from_ymd_opt(year, month_num, 1)
        .ok_or_else(|| AppError::bad_request("invalid month"))?;
    let (next_y, next_m) = if month_num == 12 {
        (year + 1, 1)
    } else {
        (year, month_num + 1)
    };
    let end = NaiveDate::from_ymd_opt(next_y, next_m, 1)
        .ok_or_else(|| AppError::bad_request("invalid month"))?;
    Ok((start, end))
}
