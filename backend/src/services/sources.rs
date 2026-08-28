use anyhow::Result;
use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

const MONTHS_PT: [&str; 12] = [
    "JAN", "FEV", "MAR", "ABR", "MAI", "JUN", "JUL", "AGO", "SET", "OUT", "NOV", "DEZ",
];

/// Extract the inclusive period a bank statement covers:
/// - **Nubank**: encoded in the filename (`NU_..._01JAN2026_31JAN2026.csv`)
/// - **C6 / Caixa**: a header date range ("Extrato de 24/08/2025 a 24/08/2026")
/// Returns `None` for documents without a detectable period (e.g. card faturas,
/// which are always complete).
pub fn extract_statement_period(filename: &str, text: &str) -> Option<(NaiveDate, NaiveDate)> {
    if let Some(range) = nubank_period_from_filename(filename) {
        return Some(range);
    }
    for line in text.lines().take(25) {
        if let Some((a, b)) = header_range_in_line(line) {
            return Some((a.min(b), a.max(b)));
        }
    }
    None
}

/// `NU_32530067_01JAN2026_31JAN2026.csv` → (2026-01-01, 2026-01-31).
/// A filename like `01AGO2026_22AGO2026` yields a partial August.
fn nubank_period_from_filename(filename: &str) -> Option<(NaiveDate, NaiveDate)> {
    let parse = |s: &str| -> Option<NaiveDate> {
        let s = s.trim();
        if s.len() != 9 {
            return None;
        }
        let day: u32 = s[0..2].parse().ok()?;
        let mon = MONTHS_PT.iter().position(|m| *m == &s[2..5])? as u32 + 1;
        let year: i32 = s[5..9].parse().ok()?;
        NaiveDate::from_ymd_opt(year, mon, day)
    };
    let mut dates: Vec<NaiveDate> = filename
        .to_uppercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(parse)
        .collect();
    dates.sort_unstable();
    match dates.len() {
        2 => Some((dates[0], dates[1])),
        _ => None,
    }
}

/// Find a "DD/MM/YYYY … a/à … DD/MM/YYYY" pair in one header line.
fn header_range_in_line(line: &str) -> Option<(NaiveDate, NaiveDate)> {
    let chars: Vec<char> = line.chars().collect();
    let mut dates: Vec<(usize, NaiveDate)> = Vec::new();
    if chars.len() < 10 {
        return None;
    }
    for i in 0..=chars.len() - 10 {
        let chunk: String = chars[i..i + 10].iter().collect();
        // Strict: chrono's %d/%Y are lenient (space padding, 3-digit years) —
        // only accept exact DD/MM/YYYY round-trips.
        if let Ok(d) = NaiveDate::parse_from_str(&chunk, "%d/%m/%Y") {
            if d.format("%d/%m/%Y").to_string() == chunk {
                dates.push((i, d));
            }
        }
    }
    if dates.len() < 2 {
        return None;
    }
    // The two dates of the range are separated by "a"/"à" (or "–").
    for w in dates.windows(2) {
        let between: String = chars[w[0].0 + 10..w[1].0].iter().collect();
        if between.contains('a') || between.contains('à') || between.contains('–') || between.contains('-') {
            return Some((w[0].1, w[1].1));
        }
    }
    // Fallback: min/max of the dates in the line.
    let mut ds: Vec<NaiveDate> = dates.iter().map(|(_, d)| *d).collect();
    ds.sort_unstable();
    Some((ds[0], *ds.last().unwrap()))
}

/// Detect `(bank, document_kind)` from raw document text.
/// `kind` is `'bank_statement' | 'card_statement'` (matching `documents.kind`).
pub fn detect_bank_kind(text: &str) -> Option<(&'static str, &'static str)> {
    let lower = text.to_lowercase();

    if lower.contains("data de compra") {
        return Some(("c6", "card_statement"));
    }
    if lower.contains("extrato de conta corrente") || lower.contains("data lançamento") {
        return Some(("c6", "bank_statement"));
    }
    if lower.contains("identificador") && lower.contains("descri") {
        return Some(("nubank", "bank_statement"));
    }
    if lower.contains("title") && lower.contains("amount") {
        return Some(("nubank", "card_statement"));
    }
    if lower.contains("cartões caixa")
        || lower.contains("cartoes caixa")
        || lower.contains("compras (cartão")
        || lower.contains("compras (cartao")
    {
        return Some(("caixa", "card_statement"));
    }
    if lower.contains("extrato por período") || lower.contains("extrato por periodo") {
        return Some(("caixa", "bank_statement"));
    }
    None
}

/// Look up the `sources.id` for a detected (bank, kind).
pub async fn source_id_for_text(pool: &PgPool, text: &str) -> Result<Option<Uuid>> {
    let Some((bank, kind)) = detect_bank_kind(text) else {
        return Ok(None);
    };
    let id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM sources WHERE bank = $1 AND kind = $2")
        .bind(bank)
        .bind(kind)
        .fetch_optional(pool)
        .await?;
    Ok(id)
}

/// Detect the source for an uploaded document (from its filename + bytes).
/// Images (receipts) have no fundamental source.
pub async fn detect_source(
    pool: &PgPool,
    filename: &str,
    content_type: &str,
    data: &[u8],
) -> Result<Option<Uuid>> {
    let lower = filename.to_lowercase();

    let text: Option<String> = if lower.ends_with(".csv") || content_type.contains("csv") {
        Some(String::from_utf8_lossy(data).to_string())
    } else if lower.ends_with(".pdf") || content_type.contains("pdf") {
        let bytes = data.to_vec();
        tokio::task::spawn_blocking(move || pdf_extract::extract_text_from_mem(&bytes).ok())
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let Some(text) = text else {
        return Ok(None);
    };
    source_id_for_text(pool, &text).await
}

/// Re-detect sources for documents that have none (e.g. uploaded before the
/// sources feature existed). Reads each file and updates `documents.source_id`.
pub async fn backfill_null_sources(pool: PgPool) -> Result<usize> {
    let docs: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, filename, content_type, file_path FROM documents WHERE source_id IS NULL",
    )
    .fetch_all(&pool)
    .await?;

    let mut updated = 0;
    for (id, filename, content_type, file_path) in docs {
        let Ok(data) = tokio::fs::read(&file_path).await else {
            continue;
        };
        if let Some(source_id) = detect_source(&pool, &filename, &content_type, &data).await? {
            sqlx::query("UPDATE documents SET source_id = $1 WHERE id = $2")
                .bind(source_id)
                .bind(id)
                .execute(&pool)
                .await?;
            updated += 1;
        }
    }
    Ok(updated)
}

/// Backfill `statement_start/statement_end` for bank statements uploaded before
/// the feature existed. Idempotent (only documents still missing the period).
pub async fn backfill_statement_periods(pool: PgPool) -> Result<usize> {
    let docs: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, filename, file_path FROM documents
         WHERE kind = 'bank_statement' AND statement_start IS NULL",
    )
    .fetch_all(&pool)
    .await?;

    let mut updated = 0;
    for (id, filename, file_path) in docs {
        let Ok(content) = tokio::fs::read_to_string(&file_path).await else {
            continue;
        };
        if let Some((start, end)) = extract_statement_period(&filename, &content) {
            sqlx::query(
                "UPDATE documents SET statement_start = $1, statement_end = $2 WHERE id = $3",
            )
            .bind(start)
            .bind(end)
            .bind(id)
            .execute(&pool)
            .await?;
            updated += 1;
        }
    }
    Ok(updated)
}
