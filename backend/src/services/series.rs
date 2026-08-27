//! Purchase-series reconstruction.
//!
//! Links the parcels of one installment purchase across faturas. The items table
//! can't do this reliably (two identical purchases from the same merchant look
//! alike), so we reconstruct from the **source documents**: each fatura carries
//! exactly one parcel per series, the line's original purchase date is stable
//! across the series, and the monthly cadence (parcel k in month M, k+1 in M+1)
//! disambiguates interleaved series.
//!
//! - `assign_document` runs at ingest (fresh parsed lines + fresh items).
//! - `backfill` re-parses stored documents for legacy items without a series.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use chrono::{Datelike, Months, NaiveDate};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::DocumentRow;
use crate::services::parsers::ParsedItem;

/// Assign `series_id` to the installment items parsed from `doc`. Non-installment
/// lines are untouched. Called at ingest with the just-parsed lines; the parsed
/// lines are matched to the freshly inserted item rows by their exact tuple.
pub async fn assign_document(pool: &PgPool, doc: &DocumentRow, parsed: &[ParsedItem]) -> Result<()> {
    // Existing item rows of this document (id, tuple) for installment lines.
    let rows: Vec<(Uuid, NaiveDate, String, i64, i32, i32)> = sqlx::query_as(
        "SELECT id, occurred_on, description, amount_cents, installment, installment_count
         FROM items
         WHERE document_id = $1 AND installment IS NOT NULL AND installment_count > 1
         ORDER BY installment, occurred_on",
    )
    .bind(doc.id)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }

    // Index rows by (occurred_on, description, amount, installment, count).
    let mut by_tuple: HashMap<(NaiveDate, String, i64, i32, i32), Vec<Uuid>> = HashMap::new();
    for (id, occurred_on, description, amount, inst, count) in rows {
        by_tuple
            .entry((occurred_on, description, amount, inst, count))
            .or_default()
            .push(id);
    }

    // Group parsed installment lines into within-document series. One parcel per
    // series per fatura, so a group with >1 line is an identical purchase on the
    // same date — ambiguous, skip it (never guess wrong).
    let mut groups: HashMap<(Option<NaiveDate>, String, i32), Vec<&ParsedItem>> = HashMap::new();
    for item in parsed {
        if item.installment_count.unwrap_or(0) <= 1 || item.installment.is_none() {
            continue;
        }
        let desc = item.description.trim().to_string();
        let key = (item.purchase_date, desc, item.installment_count.unwrap());
        groups.entry(key).or_default().push(item);
    }

    let mut tx = pool.begin().await?;
    for ((purchase_date, desc, count), lines) in groups {
        if lines.len() > 1 {
            continue; // ambiguous
        }
        let line = lines[0];
        let parcel = line.installment.unwrap();
        let Some(item_id) = by_tuple
            .get_mut(&(line.occurred_on, desc.clone(), line.amount_cents, parcel, count))
            .and_then(|v| v.pop())
        else {
            continue;
        };
        let series_id = match_or_create(
            &mut tx,
            doc.source_id,
            &desc,
            count,
            purchase_date,
            parcel,
            line.occurred_on,
        )
        .await?;
        sqlx::query("UPDATE items SET series_id = $1 WHERE id = $2")
            .bind(series_id)
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Find the series this parcel continues, or create a new one.
/// Matching = same (source, description, count) + expected cadence (next parcel,
/// next month) + compatible purchase date.
async fn match_or_create(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source_id: Option<Uuid>,
    desc: &str,
    count: i32,
    purchase_date: Option<NaiveDate>,
    parcel: i32,
    occurred_on: NaiveDate,
) -> Result<Uuid> {
    let candidates: Vec<(Uuid, Option<NaiveDate>, Option<i32>, Option<NaiveDate>)> =
        sqlx::query_as(
            "SELECT s.id, s.purchase_date, MAX(i.installment)::int, MAX(i.occurred_on)
             FROM purchase_series s
             LEFT JOIN items i ON i.series_id = s.id
             WHERE s.description = $1 AND s.installment_count = $2
               AND (s.source_id IS NOT DISTINCT FROM $3)
             GROUP BY s.id",
        )
        .bind(desc)
        .bind(count)
        .bind(source_id)
        .fetch_all(&mut **tx)
        .await?;

    let item_ym = (occurred_on.year(), occurred_on.month());
    let mut matching: Vec<(Uuid, Option<NaiveDate>)> = Vec::new();
    for (sid, s_purchase, max_inst, last_date) in candidates {
        let expected_parcel = max_inst.map(|m| m + 1).unwrap_or(1);
        let expected_ym = match last_date {
            Some(d) => {
                let first = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap();
                let next = first.checked_add_months(Months::new(1)).unwrap();
                (next.year(), next.month())
            }
            None => item_ym,
        };
        if expected_parcel == parcel
            && expected_ym == item_ym
            && (s_purchase.is_none() || purchase_date.is_none() || s_purchase == purchase_date)
        {
            matching.push((sid, s_purchase));
        }
    }

    if matching.len() == 1 {
        return Ok(matching[0].0);
    }
    if matching.len() > 1 {
        // Prefer the candidate whose purchase date matches exactly.
        if let Some((sid, Some(_))) = matching
            .iter()
            .find(|(_, pd)| *pd == purchase_date)
        {
            return Ok(*sid);
        }
    }

    // No (unambiguous) match → new series.
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO purchase_series (source_id, description, installment_count, purchase_date)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(source_id)
    .bind(desc)
    .bind(count)
    .bind(purchase_date)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

/// One-time backfill for legacy data: re-parse every document that still has
/// installment items without a `series_id` and assign series. Idempotent — once
/// all items are linked it becomes a no-op, so it can run at every startup.
pub async fn backfill(pool: &PgPool, storage_dir: &std::path::Path) -> Result<usize> {
    let docs: Vec<DocumentRow> = sqlx::query_as(
        "SELECT d.id, d.kind, d.account_id, d.source_id, d.filename, d.content_type,
                d.sha256, d.file_path, d.status, d.error_message, d.ocr_text,
                d.uploaded_at, d.processed_at
         FROM documents d
         WHERE EXISTS (
           SELECT 1 FROM items i
           WHERE i.document_id = d.id AND i.installment_count > 1 AND i.series_id IS NULL)
         ORDER BY d.uploaded_at",
    )
    .fetch_all(pool)
    .await?;

    let mut processed = 0usize;
    for doc in &docs {
        match parse_document_file(doc, storage_dir).await {
            Ok(parsed) => {
                if let Err(e) = assign_document(pool, doc, &parsed).await {
                    tracing::warn!(document = %doc.id, "series backfill failed: {e:#}");
                } else {
                    processed += 1;
                }
            }
            Err(e) => tracing::warn!(document = %doc.id, "series backfill: {e:#}"),
        }
    }
    Ok(processed)
}

async fn parse_document_file(
    doc: &DocumentRow,
    storage_dir: &std::path::Path,
) -> Result<Vec<ParsedItem>> {
    // The file may be stored under an absolute path (container) or relative to
    // the storage dir (dev). Try both.
    let path = if std::path::Path::new(&doc.file_path).exists() {
        std::path::PathBuf::from(&doc.file_path)
    } else {
        storage_dir.join(&doc.file_path)
    };
    if !path.exists() {
        return Err(anyhow!("file missing: {}", path.display()));
    }
    let content = tokio::fs::read_to_string(&path)
        .await
        .context("failed to read stored document")?;
    let billing_month = billing_month_from_filename(&doc.filename);
    let parsed = crate::services::parsers::csv::parse_csv(&content, billing_month)
        .context("failed to re-parse stored document")?;
    Ok(parsed)
}

/// Extract the billing month from a fatura filename like `Fatura_2026-08-15.csv`.
fn billing_month_from_filename(filename: &str) -> Option<NaiveDate> {
    let name = filename.to_lowercase();
    let rest = name.strip_prefix("fatura_")?.trim_end_matches(".csv");
    let parts: Vec<&str> = rest.split('-').collect();
    if parts.len() < 2 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    NaiveDate::from_ymd_opt(year, month, 1)
}
