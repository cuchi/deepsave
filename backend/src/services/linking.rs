use anyhow::Result;
use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

/// Run the linking heuristic for every receipt document.
pub async fn suggest_links_all(pool: &PgPool) -> Result<usize> {
    let docs: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM documents WHERE kind = 'receipt'")
        .fetch_all(pool)
        .await?;
    let mut total = 0;
    for (doc_id,) in docs {
        total += suggest_links_for_document(pool, doc_id).await?;
    }
    Ok(total)
}

/// Match a whole receipt (sum of its line items, dominant merchant, earliest date)
/// against statement items, then link every receipt line to the best statement item.
async fn suggest_links_for_document(pool: &PgPool, doc_id: Uuid) -> Result<usize> {
    let items: Vec<(Uuid, i64, Option<String>, NaiveDate)> = sqlx::query_as(
        "SELECT id, amount_cents, merchant, occurred_on
         FROM items
         WHERE document_id = $1 AND parent_id IS NULL",
    )
    .bind(doc_id)
    .fetch_all(pool)
    .await?;

    if items.is_empty() {
        return Ok(0);
    }

    let receipt_total: i64 = items.iter().map(|(_, a, _, _)| a.abs()).sum();
    let merchants: Vec<String> = items
        .iter()
        .filter_map(|(_, _, m, _)| m.clone())
        .collect();
    let merchant = most_common(&merchants);
    let date = items.iter().map(|(_, _, _, d)| *d).min().unwrap_or_else(|| {
        chrono::Utc::now().date_naive()
    });

    let statements: Vec<(Uuid, Option<String>, i64, NaiveDate)> = sqlx::query_as(
        "SELECT id, merchant, amount_cents, occurred_on
         FROM items
         WHERE source IN ('card_statement', 'bank_statement')
           AND kind = 'expense' AND status = 'confirmed'",
    )
    .fetch_all(pool)
    .await?;

    let mut best: Option<(Uuid, f32)> = None;
    for (sid, smerch, samt, sdate) in &statements {
        if let Some(conf) = match_confidence(&merchant, receipt_total, date, smerch, *samt, *sdate)
        {
            if best.as_ref().is_none_or(|(_, c)| conf > *c) {
                best = Some((*sid, conf));
            }
        }
    }

    let Some((statement_id, confidence)) = best else {
        return Ok(0);
    };

    let mut inserted = 0;
    for (item_id, _, _, _) in &items {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM matches
              WHERE parent_item_id = $1 AND child_item_id = $2 AND status = 'suggested')",
        )
        .bind(statement_id)
        .bind(item_id)
        .fetch_one(pool)
        .await?;
        if !exists {
            sqlx::query(
                "INSERT INTO matches (parent_item_id, child_item_id, source, confidence, status)
                 VALUES ($1, $2, 'heuristic', $3, 'suggested')",
            )
            .bind(statement_id)
            .bind(item_id)
            .bind(confidence)
            .execute(pool)
            .await?;
            inserted += 1;
        }
    }

    Ok(inserted)
}

pub fn match_confidence(
    receipt_merchant: &Option<String>,
    receipt_total: i64,
    receipt_date: NaiveDate,
    statement_merchant: &Option<String>,
    statement_amount: i64,
    statement_date: NaiveDate,
) -> Option<f32> {
    let samt = statement_amount.abs();
    if receipt_total == 0 || samt == 0 {
        return None;
    }
    // Receipt total should be close to the statement charge (5% or R$5 tolerance).
    if (receipt_total - samt).abs() > (samt as f64 * 0.05) as i64 + 500 {
        return None;
    }
    if (receipt_date - statement_date).num_days().abs() > 7 {
        return None;
    }
    let (Some(rm), Some(sm)) = (receipt_merchant, statement_merchant) else {
        return None;
    };
    let rn = normalize(rm);
    let sn = normalize(sm);
    if rn.is_empty() || sn.is_empty() {
        return None;
    }
    if rn == sn {
        Some(0.9)
    } else if rn.contains(&sn) || sn.contains(&rn) {
        Some(0.6)
    } else {
        None
    }
}

fn most_common(items: &[String]) -> Option<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for s in items {
        *counts.entry(s.as_str()).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(s, _)| s.to_string())
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}
