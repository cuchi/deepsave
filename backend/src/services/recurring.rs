use anyhow::Result;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::PgPool;

use crate::services::tags;

/// A suggested recurring rule (not yet persisted), from detection.
#[derive(Debug, Serialize)]
pub struct Suggestion {
    pub merchant: Option<String>,
    pub description: String,
    pub amount_cents: i64,
    pub frequency: String,
    pub interval: i32,
    pub count: i32,
    pub last_seen: NaiveDate,
}

/// Scan confirmed, unrecurred expenses and suggest recurring rules
/// (same merchant + similar amount + regular interval, ≥ 2 occurrences).
///
/// Recurring rules are analytics-only: they never create items.
pub async fn suggest(pool: &PgPool) -> Result<Vec<Suggestion>> {
    let rows: Vec<(Option<String>, String, i64, NaiveDate)> = sqlx::query_as(
        "SELECT merchant, description, amount_cents, occurred_on
         FROM items
         WHERE status = 'confirmed' AND kind = 'expense' AND recurring_id IS NULL
           AND (installment_count IS NULL OR installment_count <= 1)",
    )
    .fetch_all(pool)
    .await?;

    let mut groups: std::collections::HashMap<String, Vec<(String, i64, NaiveDate)>> =
        std::collections::HashMap::new();
    for (merchant, description, amount, date) in rows {
        let key = tags::strip_accents(merchant.as_deref().unwrap_or(&description)).to_lowercase();
        groups
            .entry(key)
            .or_default()
            .push((description, amount, date));
    }

    let mut out = Vec::new();
    for (_key, mut items) in groups {
        if items.len() < 2 {
            continue;
        }
        items.sort_by_key(|(_, _, d)| *d);

        // Amounts must be similar (within 5%).
        let amounts: Vec<i64> = items.iter().map(|(_, a, _)| a.abs()).collect();
        let first = amounts[0];
        if amounts.iter().any(|a| (*a as f64 - first as f64).abs() > first as f64 * 0.05) {
            continue;
        }

        // Regular interval from median day-gap.
        let gaps: Vec<i64> = items
            .windows(2)
            .map(|w| (w[1].2 - w[0].2).num_days())
            .collect();
        let Some(median_gap) = median(&gaps) else {
            continue;
        };
        let frequency = if (6..=8).contains(&median_gap) {
            ("weekly", 1)
        } else if (27..=31).contains(&median_gap) {
            ("monthly", 1)
        } else if (56..=62).contains(&median_gap) {
            ("monthly", 2)
        } else if (340..=380).contains(&median_gap) {
            ("yearly", 1)
        } else {
            continue;
        };

        let (description, _, last) = items.last().unwrap();
        out.push(Suggestion {
            merchant: None,
            description: description.clone(),
            amount_cents: -first,
            frequency: frequency.0.to_string(),
            interval: frequency.1,
            count: items.len() as i32,
            last_seen: *last,
        });
    }

    out.sort_by(|a, b| b.count.cmp(&a.count));
    Ok(out)
}

fn median(vals: &[i64]) -> Option<i64> {
    if vals.is_empty() {
        return None;
    }
    let mut v = vals.to_vec();
    v.sort_unstable();
    Some(v[v.len() / 2])
}
