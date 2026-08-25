//! AI-assisted bulk tagging.
//!
//! The user selects items (Month view), we create an `ai_tag_batches` row with one
//! `ai_tag_suggestions` row per item, and a background worker (mirroring the
//! document worker) calls DeepSeek with a compact payload: every existing tag, the
//! selected items (description, merchant, amount, date, category, tags) and, per
//! distinct merchant, up to 3 recent already-tagged examples. The response fills
//! in the `suggested_tags`, then the human reviews them on the Revisar page
//! (apply = add to the item, dismiss = reject).

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde_json::json;
use sqlx::PgPool;
use tracing::{error, info};
use uuid::Uuid;

use crate::error::AppError;
use crate::models::AiTagBatch;
use crate::services::ai::AiClient;
use crate::services::tags;

/// Maximum items per batch — bounds the prompt size for a single AI call.
pub const MAX_BATCH_ITEMS: usize = 200;
/// Tagged examples per merchant sent to the AI.
pub const EXAMPLES_PER_MERCHANT: i64 = 3;

/// Create a `pending` batch with one suggestion row per existing item id.
/// Unknown ids are silently skipped; errors if nothing exists.
pub async fn enqueue_batch(pool: &PgPool, ids: Vec<Uuid>) -> Result<AiTagBatch, AppError> {
    let mut seen = HashSet::new();
    let ids: Vec<Uuid> = ids.into_iter().filter(|id| seen.insert(*id)).collect();
    if ids.is_empty() {
        return Err(AppError::bad_request("ids must not be empty"));
    }
    if ids.len() > MAX_BATCH_ITEMS {
        return Err(AppError::bad_request(format!(
            "too many ids (max {MAX_BATCH_ITEMS})"
        )));
    }

    let existing: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE id = ANY($1)")
        .bind(&ids)
        .fetch_one(pool)
        .await?;
    if existing == 0 {
        return Err(AppError::bad_request("none of the selected ids exist"));
    }

    let mut tx = pool.begin().await?;
    let batch_id: Uuid = sqlx::query_scalar(
        "INSERT INTO ai_tag_batches (status) VALUES ('pending') RETURNING id",
    )
    .fetch_one(&mut *tx)
    .await?;
    // One suggestion per existing item, in the order the user selected them.
    sqlx::query(
        "INSERT INTO ai_tag_suggestions (batch_id, item_id)
         SELECT $1, id FROM items WHERE id = ANY($2)",
    )
    .bind(batch_id)
    .bind(&ids)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(AiTagBatch {
        id: batch_id,
        status: "pending".to_string(),
        error_message: None,
        item_count: existing,
        created_at: chrono::Utc::now(),
        processed_at: None,
    })
}

/// Background worker: claim `pending` batches and run the AI tagging.
pub async fn run_worker(pool: PgPool, ai: AiClient) {
    info!("ai-tag worker started");
    loop {
        match claim_next(&pool).await {
            Some(batch) => {
                info!(batch = %batch.id, items = batch.item_count, "processing tag batch");
                match process_batch(&pool, &ai, batch.id).await {
                    Ok(()) => {
                        sqlx::query(
                            "UPDATE ai_tag_batches SET status = 'done', processed_at = now() WHERE id = $1",
                        )
                        .bind(batch.id)
                        .execute(&pool)
                        .await
                        .ok();
                        info!(batch = %batch.id, "tag batch done");
                    }
                    Err(e) => {
                        error!(batch = %batch.id, "tag batch failed: {e:#}");
                        sqlx::query(
                            "UPDATE ai_tag_batches SET status = 'failed', error_message = $1, processed_at = now() WHERE id = $2",
                        )
                        .bind(format!("{e:#}"))
                        .bind(batch.id)
                        .execute(&pool)
                        .await
                        .ok();
                    }
                }
            }
            None => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
        }
    }
}

async fn claim_next(pool: &PgPool) -> Option<AiTagBatch> {
    sqlx::query_as::<_, AiTagBatch>(sqlx::AssertSqlSafe(
        "UPDATE ai_tag_batches SET status = 'processing'
         WHERE id = (SELECT id FROM ai_tag_batches WHERE status = 'pending' ORDER BY created_at LIMIT 1)
         RETURNING id, status, error_message, created_at, processed_at,
                   0::bigint AS item_count"
    ))
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

type BatchItemRow = (
    Uuid,
    Option<String>,
    String,
    i64,
    NaiveDate,
    Option<String>,
    Vec<String>,
);
type ExampleRow = (String, i64, NaiveDate, Option<String>, Vec<String>);

/// Run the AI call for a batch and store per-item suggestions. Exposed (pub) so
/// integration tests can drive it directly, like `bulk_update_items`.
pub async fn process_batch(pool: &PgPool, ai: &AiClient, batch_id: Uuid) -> Result<()> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT item_id FROM ai_tag_suggestions WHERE batch_id = $1 ORDER BY created_at",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;
    if ids.is_empty() {
        return Ok(());
    }

    let rows: Vec<BatchItemRow> = sqlx::query_as(
        "SELECT i.id, i.merchant, i.description, i.amount_cents, i.occurred_on, c.name, i.tags
         FROM items i
         LEFT JOIN categories c ON c.id = i.category_id
         WHERE i.id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let by_id: HashMap<Uuid, _> = rows.into_iter().map(|r| (r.0, r)).collect();

    let all_tags: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT tag FROM items CROSS JOIN LATERAL unnest(tags) AS tag ORDER BY tag",
    )
    .fetch_all(pool)
    .await?;

    let merchants: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT merchant FROM items WHERE id = ANY($1) AND merchant IS NOT NULL",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;

    // Per merchant: the last `EXAMPLES_PER_MERCHANT` confirmed items that carry
    // tags — real ground truth for the AI to mirror (compact: same fields as the
    // items, plus tags).
    let mut examples: Vec<serde_json::Value> = Vec::new();
    for merchant in &merchants {
        let ex: Vec<ExampleRow> = sqlx::query_as(
            "SELECT i.description, i.amount_cents, i.occurred_on, c.name, i.tags
             FROM items i
             LEFT JOIN categories c ON c.id = i.category_id
             WHERE i.merchant = $1 AND cardinality(i.tags) > 0 AND i.status = 'confirmed'
             ORDER BY i.occurred_on DESC, i.created_at DESC
             LIMIT $2",
        )
        .bind(merchant)
        .bind(EXAMPLES_PER_MERCHANT)
        .fetch_all(pool)
        .await?;
        if !ex.is_empty() {
            let list: Vec<serde_json::Value> = ex
                .into_iter()
                .map(|(desc, amt, date, cat, tags)| {
                    json!({ "desc": desc, "amt": amt, "date": date, "cat": cat, "tags": tags })
                })
                .collect();
            examples.push(json!({ "merch": merchant, "n": list }));
        }
    }

    // Compact item list, in batch (selection) order. `i` is the 0-based index the
    // AI must echo back (avoids hallucinating uuids).
    let mut items = Vec::with_capacity(ids.len());
    for (i, id) in ids.iter().enumerate() {
        if let Some((_, merchant, description, amount, date, cat, tags)) = by_id.get(id) {
            items.push(json!({
                "i": i,
                "desc": description,
                "merch": merchant,
                "amt": amount,
                "date": date.format("%Y-%m-%d").to_string(),
                "cat": cat,
                "tags": tags,
            }));
        }
    }

    // DeepSeek's `content` field must be a string (or a list for vision) — the
    // chat_json helper forwards the Value as-is, so wrap the payload in a string.
    let user = json!({ "all_tags": all_tags, "items": items, "ex": examples });
    let system = build_tag_system_prompt();
    let value = ai
        .chat_json(&system, json!(user.to_string()), ai.text_model(), None, "tag_batch")
        .await
        .context("AI tagging call failed")?;

    let mut seen = HashSet::new();
    let mut tx = pool.begin().await?;
    if let Some(arr) = value.get("suggestions").and_then(|v| v.as_array()) {
        for entry in arr {
            let Some(index) = entry.get("index").and_then(|x| x.as_u64()) else {
                continue;
            };
            let index = index as usize;
            if index >= ids.len() || !seen.insert(index) {
                continue;
            }
            let raw_tags: Vec<String> = entry
                .get("tags")
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let tags = tags::normalize(&raw_tags);
            sqlx::query(
                "UPDATE ai_tag_suggestions SET suggested_tags = $1
                 WHERE batch_id = $2 AND item_id = $3",
            )
            .bind(&tags)
            .bind(batch_id)
            .bind(ids[index])
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Stable system prompt (kept constant so DeepSeek's context cache can serve it);
/// the variable data (tags, items, examples) goes in the user message.
fn build_tag_system_prompt() -> String {
    r#"Você é um especialista em finanças pessoais brasileiras. Sua tarefa é sugerir TAGS para itens de um controle financeiro. Responda APENAS com JSON válido (sem markdown, sem texto extra).

Esquema JSON exato:
{
  "suggestions": [
    { "index": integer, "tags": [string] }
  ]
}

Regras:
- "index" é a posição (0-based) do item no array "items" da mensagem do usuário. NÃO invente índices; use apenas índices que existem em "items".
- Sugira de 1 a 4 tags por item, curtas, em minúsculas e sem acentos, em português (ex: "viagem", "assinatura", "presente", "trabalho", "mercado").
- Tags são SITUACIONAIS (o contexto do gasto), não a categoria. Se o item já tem tags, você pode mantê-las ou refiná-las.
- Prefira tags da lista "all_tags"; crie uma nova apenas se nenhuma existente encaixar bem.
- "ex" traz exemplos reais de itens já taggeados do mesmo comerciante — use-os como referência de estilo.
- Itens sem informação suficiente ou que claramente não merecem tag devem ser OMITIDOS da resposta (não inclua o índice)."#
        .to_string()
}

/// Add tags to an item (merge + dedupe, keeping first-occurrence order), then mark
/// the suggestion as applied. `tags` are normalized server-side.
pub async fn apply_suggestion(
    pool: &PgPool,
    suggestion_id: Uuid,
    tags: Option<Vec<String>>,
) -> Result<serde_json::Value, AppError> {
    let row: Option<(Uuid, Vec<String>, String)> = sqlx::query_as(
        "SELECT item_id, suggested_tags, status FROM ai_tag_suggestions WHERE id = $1",
    )
    .bind(suggestion_id)
    .fetch_optional(pool)
    .await?;
    let Some((item_id, stored, status)) = row else {
        return Err(AppError::not_found("suggestion not found"));
    };
    if status != "pending" {
        return Err(AppError::conflict("suggestion already reviewed"));
    }
    let tags = match tags {
        Some(t) => tags::normalize(&t),
        None => stored,
    };

    let mut tx = pool.begin().await?;
    if !tags.is_empty() {
        add_tags_to_item(&mut tx, item_id, &tags).await?;
    }
    sqlx::query("UPDATE ai_tag_suggestions SET status = 'applied' WHERE id = $1")
        .bind(suggestion_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(json!({ "ok": true, "item_id": item_id, "tags": tags }))
}

pub async fn dismiss_suggestion(pool: &PgPool, suggestion_id: Uuid) -> Result<serde_json::Value, AppError> {
    let res = sqlx::query(
        "UPDATE ai_tag_suggestions SET status = 'dismissed' WHERE id = $1 AND status = 'pending'",
    )
    .bind(suggestion_id)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM ai_tag_suggestions WHERE id = $1)",
        )
        .bind(suggestion_id)
        .fetch_one(pool)
        .await?;
        return if exists {
            Err(AppError::conflict("suggestion already reviewed"))
        } else {
            Err(AppError::not_found("suggestion not found"))
        };
    }
    Ok(json!({ "ok": true }))
}

/// Apply every pending suggestion (optionally scoped to a batch) — "Aplicar tudo".
pub async fn apply_all(pool: &PgPool, batch_id: Option<Uuid>) -> Result<serde_json::Value, AppError> {
    let rows: Vec<(Uuid, Uuid, Vec<String>)> = sqlx::query_as(
        "SELECT id, item_id, suggested_tags FROM ai_tag_suggestions
         WHERE status = 'pending' AND ($1::uuid IS NULL OR batch_id = $1)",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    let mut tx = pool.begin().await?;
    for (id, item_id, tags) in &rows {
        if !tags.is_empty() {
            add_tags_to_item(&mut tx, *item_id, tags).await?;
        }
        sqlx::query("UPDATE ai_tag_suggestions SET status = 'applied' WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(json!({ "ok": true, "applied": rows.len() }))
}

/// Dismiss every pending suggestion (optionally scoped to a batch).
pub async fn dismiss_all(
    pool: &PgPool,
    batch_id: Option<Uuid>,
) -> Result<serde_json::Value, AppError> {
    let res = sqlx::query(
        "UPDATE ai_tag_suggestions SET status = 'dismissed'
         WHERE status = 'pending' AND ($1::uuid IS NULL OR batch_id = $1)",
    )
    .bind(batch_id)
    .execute(pool)
    .await?;
    Ok(json!({ "ok": true, "dismissed": res.rows_affected() }))
}

/// Merge `tags` into the item's existing tags (unique, first-occurrence order).
async fn add_tags_to_item(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item_id: Uuid,
    tags: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE items SET tags = (\
           SELECT array_agg(t ORDER BY ord) \
           FROM (SELECT t, min(ord) AS ord \
                 FROM unnest(tags || $1) WITH ORDINALITY AS u(t, ord) \
                 GROUP BY t) s \
         ), updated_at = now() WHERE id = $2",
    )
    .bind(tags)
    .bind(item_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
