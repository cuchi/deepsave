use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::DocumentRow;
use crate::services::ai::{self, AiClient, AiExtraction};
use crate::services::parsers::{caixa_card, csv, ParsedItem};
use crate::services::{extract, linking, tags};

/// Process a single document based on its file type.
///
/// - CSV → parse into items directly (bank data is already structured).
/// - PDF → extract text layer → DeepSeek extraction → items as `pending_review`.
/// - Image → DeepSeek vision (fallback: OCR → text model) → items as `pending_review`.
pub async fn process_document(pool: &PgPool, doc: &DocumentRow, ai: &AiClient) -> Result<()> {
    if is_csv(&doc.filename, &doc.content_type) {
        process_csv(pool, doc).await?;
    } else if is_image(&doc.filename, &doc.content_type) {
        process_image(pool, doc, ai).await?;
    } else {
        process_pdf(pool, doc, ai).await?;
    }

    // After any ingestion, try to link receipts to statement items.
    linking::suggest_links_all(pool).await?;
    Ok(())
}

fn is_csv(filename: &str, content_type: &str) -> bool {
    filename.to_lowercase().ends_with(".csv") || content_type.contains("csv")
}

fn is_image(filename: &str, content_type: &str) -> bool {
    let lower = filename.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || content_type.starts_with("image/")
}

// ---------- CSV (structured, no AI needed) ----------

async fn process_csv(pool: &PgPool, doc: &DocumentRow) -> Result<()> {
    let content = tokio::fs::read_to_string(&doc.file_path)
        .await
        .context("failed to read csv file")?;
    let billing_month = billing_month_from_filename(&doc.filename);
    let items = csv::parse_csv(&content, billing_month).context("failed to parse csv")?;

    let source = statement_source(doc);
    insert_parsed_items(pool, doc, source, &items).await?;

    sqlx::query("UPDATE documents SET status = 'processed', processed_at = now() WHERE id = $1")
        .bind(doc.id)
        .execute(pool)
        .await?;
    Ok(())
}

fn statement_source(doc: &DocumentRow) -> &'static str {
    if doc.kind == "bank_statement" {
        "bank_statement"
    } else {
        "card_statement"
    }
}

/// Insert structured (non-AI) items as `confirmed`, skipping duplicates.
async fn insert_parsed_items(
    pool: &PgPool,
    doc: &DocumentRow,
    source: &str,
    items: &[ParsedItem],
) -> Result<usize> {
    let categories = load_categories(pool).await?;
    let mut count = 0;
    for item in items {
        // Skip items already imported from another document (avoid double-counting
        // when the same statement is uploaded twice with a different file).
        if item_exists(pool, item.occurred_on, &item.description, item.amount_cents, &item.kind).await? {
            continue;
        }
        let category_id = item
            .category
            .as_deref()
            .and_then(|c| match_category(&categories, c));
        let tags = tags::normalize(&item.tags);
        sqlx::query(
            "INSERT INTO items
               (parent_id, document_id, source, kind, status, account_id,
                installment, installment_count, occurred_on, merchant, description,
                amount_cents, currency, category_id, tags, raw_line)
             VALUES (NULL, $1, $2, $3, 'confirmed', $4, $5, $6, $7, $8, $9, $10, 'BRL', $11, $12, NULL)",
        )
        .bind(doc.id)
        .bind(source)
        .bind(&item.kind)
        .bind(doc.account_id)
        .bind(item.installment)
        .bind(item.installment_count)
        .bind(item.occurred_on)
        .bind(&item.merchant)
        .bind(&item.description)
        .bind(item.amount_cents)
        .bind(category_id)
        .bind(&tags)
        .execute(pool)
        .await?;
        count += 1;
    }
    Ok(count)
}

// ---------- PDF / image (AI extraction) ----------

async fn process_pdf(pool: &PgPool, doc: &DocumentRow, ai: &AiClient) -> Result<()> {
    let text = extract::extract_raw_text(&doc.file_path, &doc.content_type).await?;
    if text.trim().is_empty() {
        return Err(anyhow!("no text layer in PDF (scanned PDFs not yet supported)"));
    }

    // Caixa credit-card fatura has a structured table we can parse directly.
    if caixa_card::is_caixa_card_fatura(&text) {
        let (_billing_month, items) = caixa_card::parse(&text)?;
        insert_parsed_items(pool, doc, "card_statement", &items).await?;
        sqlx::query(
            "UPDATE documents SET status = 'processed', ocr_text = $1, processed_at = now() WHERE id = $2",
        )
        .bind(&text)
        .bind(doc.id)
        .execute(pool)
        .await?;
        return Ok(());
    }

    let extraction = ai_extract_text(ai, pool, doc, &text).await?;
    persist_ai_items(pool, doc, &extraction).await?;
    sqlx::query(
        "UPDATE documents SET status = 'needs_review', ocr_text = $1, processed_at = now() WHERE id = $2",
    )
    .bind(&text)
    .bind(doc.id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn process_image(pool: &PgPool, doc: &DocumentRow, ai: &AiClient) -> Result<()> {
    let bytes = tokio::fs::read(&doc.file_path)
        .await
        .context("failed to read image")?;
    let mime = if doc.content_type.starts_with("image/png")
        || doc.filename.to_lowercase().ends_with(".png")
    {
        "image/png"
    } else {
        "image/jpeg"
    };

    match ai_extract_vision(ai, pool, doc, &bytes, mime).await {
        Ok(extraction) => {
            persist_ai_items(pool, doc, &extraction).await?;
        }
        Err(e) => {
            tracing::warn!(document = %doc.id, "vision extraction failed ({e:#}); falling back to OCR + text model");
            let text = extract::extract_raw_text(&doc.file_path, &doc.content_type).await?;
            let extraction = ai_extract_text(ai, pool, doc, &text).await?;
            persist_ai_items(pool, doc, &extraction).await?;
        }
    }

    sqlx::query("UPDATE documents SET status = 'needs_review', processed_at = now() WHERE id = $1")
        .bind(doc.id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn ai_extract_text(
    ai: &AiClient,
    pool: &PgPool,
    doc: &DocumentRow,
    text: &str,
) -> Result<AiExtraction> {
    let system = ai::build_extraction_system_prompt(pool).await?;
    let base_user = truncate_text(text);
    let mut last_err = "extraction failed".to_string();

    for attempt in 0..3 {
        let user = if attempt == 0 {
            json!(base_user)
        } else {
            json!(format!(
                "{base_user}\n\nCorreção: sua resposta anterior não era JSON válido ou não tinha itens. Responda SOMENTE com o JSON do esquema solicitado."
            ))
        };
        let value = ai
            .chat_json(&system, user, ai.text_model(), Some(doc.id), "extract")
            .await?;
        match serde_json::from_value::<AiExtraction>(value) {
            Ok(ext) if !ext.items.is_empty() => return Ok(ext),
            Ok(_) => last_err = "AI returned no items".to_string(),
            Err(e) => last_err = format!("AI JSON validation failed: {e}"),
        }
    }
    Err(anyhow!(last_err))
}

async fn ai_extract_vision(
    ai: &AiClient,
    pool: &PgPool,
    doc: &DocumentRow,
    bytes: &[u8],
    mime: &str,
) -> Result<AiExtraction> {
    let system = ai::build_extraction_system_prompt(pool).await?;
    let base_prompt = "Extraia os dados desta imagem conforme o esquema JSON.";
    let mut last_err = "vision extraction failed".to_string();

    for attempt in 0..3 {
        let prompt = if attempt == 0 {
            base_prompt.to_string()
        } else {
            format!("{base_prompt} Sua resposta anterior não era JSON válido ou não tinha itens. Responda SOMENTE com JSON.")
        };
        let user = AiClient::vision_user(bytes, mime, &prompt);
        let value = ai
            .chat_json(&system, user, ai.vision_model(), Some(doc.id), "extract")
            .await?;
        match serde_json::from_value::<AiExtraction>(value) {
            Ok(ext) if !ext.items.is_empty() => return Ok(ext),
            Ok(_) => last_err = "AI returned no items".to_string(),
            Err(e) => last_err = format!("AI JSON validation failed: {e}"),
        }
    }
    Err(anyhow!(last_err))
}

async fn persist_ai_items(
    pool: &PgPool,
    doc: &DocumentRow,
    extraction: &AiExtraction,
) -> Result<Vec<Uuid>> {
    let categories = load_categories(pool).await?;
    let source = match doc.kind.as_str() {
        "bank_statement" => "bank_statement",
        "receipt" => "receipt",
        "payment_slip" => "payment_slip",
        _ => "card_statement",
    };
    let doc_date = extraction.date.as_deref().and_then(parse_iso_date);
    let fallback_date = doc.uploaded_at.date_naive();
    let mut ids = Vec::new();

    for item in &extraction.items {
        let occurred_on = item
            .date
            .as_deref()
            .and_then(parse_iso_date)
            .or(doc_date)
            .unwrap_or(fallback_date);
        let kind = normalize_kind(item.kind.as_deref(), item.amount_cents);
        let category_id = item
            .category
            .as_deref()
            .and_then(|c| match_category(&categories, c));
        let suggested_category = match (&item.category, category_id) {
            (Some(name), None) => Some(name.trim().to_string()),
            _ => None,
        };
        let merchant = item.merchant.clone().or_else(|| extraction.merchant.clone());
        let tags = tags::normalize(&item.tags);

        if item_exists(pool, occurred_on, &item.description, item.amount_cents, &kind).await? {
            continue;
        }

        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO items
               (parent_id, document_id, source, kind, status, account_id,
                installment, installment_count, occurred_on, merchant, description,
                amount_cents, currency, category_id, suggested_category, tags, raw_line)
             VALUES (NULL, $1, $2, $3, 'pending_review', $4, $5, $6, $7, $8, $9, $10, 'BRL', $11, $12, $13, NULL)
             RETURNING id",
        )
        .bind(doc.id)
        .bind(source)
        .bind(&kind)
        .bind(doc.account_id)
        .bind(item.installment)
        .bind(item.installment_count)
        .bind(occurred_on)
        .bind(&merchant)
        .bind(&item.description)
        .bind(item.amount_cents)
        .bind(category_id)
        .bind(&suggested_category)
        .bind(&tags)
        .fetch_one(pool)
        .await?;
        ids.push(row.0);
    }

    Ok(ids)
}

// ---------- helpers ----------

async fn load_categories(pool: &PgPool) -> Result<Vec<(Uuid, String)>> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM categories WHERE is_active")
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

fn match_category(categories: &[(Uuid, String)], ai_category: &str) -> Option<Uuid> {
    let target = alias_category(&tags::strip_accents(ai_category.trim()).to_lowercase());
    if target.is_empty() {
        return None;
    }
    categories
        .iter()
        .find(|(_, n)| tags::strip_accents(n).to_lowercase() == target)
        .or_else(|| {
            categories.iter().find(|(_, n)| {
                let nl = tags::strip_accents(n).to_lowercase();
                target.contains(&nl) || nl.contains(&target)
            })
        })
        .map(|(id, _)| *id)
}

/// Map common category synonyms to our canonical (accent-stripped) names.
fn alias_category(s: &str) -> String {
    match s {
        "mercado" | "mercados" | "supermercados" => "supermercado".to_string(),
        "restaurante" => "restaurantes".to_string(),
        "alimentacao" | "comida" => "restaurantes".to_string(),
        "farmacia" | "consultas" | "medico" | "medica" => "saude".to_string(),
        "combustivel" | "gasolina" | "uber" | "taxi" | "posto" => "transporte".to_string(),
        _ => s.to_string(),
    }
}

fn normalize_kind(kind: Option<&str>, amount: i64) -> String {
    const KINDS: &[&str] = &[
        "expense",
        "income",
        "refund",
        "card_payment",
        "investment",
    ];
    match kind {
        Some(k) if KINDS.contains(&k) => k.to_string(),
        _ => {
            if amount > 0 {
                "income".to_string()
            } else {
                "expense".to_string()
            }
        }
    }
}

fn parse_iso_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

async fn item_exists(
    pool: &PgPool,
    occurred_on: NaiveDate,
    description: &str,
    amount_cents: i64,
    kind: &str,
) -> Result<bool> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM items
          WHERE occurred_on = $1 AND description = $2 AND amount_cents = $3 AND kind = $4)",
    )
    .bind(occurred_on)
    .bind(description)
    .bind(amount_cents)
    .bind(kind)
    .fetch_one(pool)
    .await?;
    Ok(exists)
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

fn truncate_text(s: &str) -> String {
    const MAX: usize = 30_000;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…\n[truncado]", &s[..MAX])
    }
}
