//! AI-assisted bulk tagging & categorization.
//!
//! The user selects items, we create an `ai_tag_batches` row (kind = 'tags' |
//! 'categorize') with one `ai_tag_suggestions` row per item, and a background
//! worker calls DeepSeek with a compact payload. Suggestions are reviewed
//! inline in the list (apply = add tags / set category; dismiss = reject).

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
/// Unknown ids are silently skipped; errors if nothing exists. `kind` selects
/// the proposal flow: 'tags' or 'categorize'.
pub async fn enqueue_batch(
    pool: &PgPool,
    ids: Vec<Uuid>,
    kind: &str,
) -> Result<AiTagBatch, AppError> {
    if !matches!(kind, "tags" | "categorize" | "full") {
        return Err(AppError::bad_request("invalid kind (tags | categorize | full)"));
    }
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
        "INSERT INTO ai_tag_batches (status, kind) VALUES ('pending', $1) RETURNING id",
    )
    .bind(kind)
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
        kind: kind.to_string(),
    })
}

/// A batch claimed longer ago than this (minutes) is considered orphaned —
/// safe: 100+ item batches take ~3 minutes, so 10 is a generous margin.
const STALE_CLAIM_MINUTES: i32 = 10;

/// Background worker: claim batches and run the right processor. On startup it
/// resets any `processing` batch to `pending` — with a single worker instance,
/// one still marked processing can only be an orphan of a previous process.
pub async fn run_worker(pool: PgPool, ai: AiClient) {
    info!("ai-tag worker started");
    sqlx::query("UPDATE ai_tag_batches SET status = 'pending', claimed_at = NULL WHERE status = 'processing'")
        .execute(&pool)
        .await
        .ok();
    loop {
        match claim_next(&pool).await {
            Some(batch) => {
                info!(batch = %batch.id, items = batch.item_count, kind = %batch.kind, "processing ai batch");
                let res = match batch.kind.as_str() {
                    "categorize" => process_categorize_batch(&pool, &ai, batch.id).await,
                    "full" => process_full_batch(&pool, &ai, batch.id).await,
                    _ => process_batch(&pool, &ai, batch.id).await,
                };
                match res {
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

/// Claim the next batch: any `pending` one, or a `processing` one that was
/// claimed more than STALE_CLAIM_MINUTES ago (orphaned by a crashed worker or
/// a hung AI call). Stamps `claimed_at` so the watchdog can age them.
async fn claim_next(pool: &PgPool) -> Option<AiTagBatch> {
    sqlx::query_as::<_, AiTagBatch>(sqlx::AssertSqlSafe(
        "UPDATE ai_tag_batches SET status = 'processing', claimed_at = now()
         WHERE id = (SELECT id FROM ai_tag_batches
                     WHERE status = 'pending'
                        OR (status = 'processing'
                            AND claimed_at < now() - make_interval(mins => $1))
                     ORDER BY created_at LIMIT 1)
         RETURNING id, status, error_message, created_at, processed_at, kind,
                   0::bigint AS item_count"
    ))
    .bind(STALE_CLAIM_MINUTES)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Normalize an identity: strip accents, lowercase, drop non-alphanumerics.
fn norm_key(x: &str) -> String {
    crate::services::tags::strip_accents(x)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Normalized names of all active categories — used to (a) remove category
/// names from the AI's tag vocabulary and (b) drop suggested tags that are
/// really categories (deterministic post-filter).
async fn category_norms(pool: &PgPool) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM categories WHERE is_active")
        .fetch_all(pool)
        .await?;
    Ok(names.iter().map(|n| norm_key(n)).collect())
}

type BatchItemRow = (
    Uuid,
    Option<String>,
    String,
    i64,
    NaiveDate,
    Option<String>,
    Vec<String>,
    Option<String>, // pluggy_category
    Option<i64>,    // mcc
    Option<String>, // operation_type
    Option<String>, // payment_method
);
type ExampleRow = (String, i64, NaiveDate, Option<String>, Vec<String>);

/// Recent change-log entries per merchant identity (capped per key) — the
/// user's own history of category/tag decisions, for the AI to follow.
async fn load_change_history(
    pool: &PgPool,
    keys: &[String],
) -> Result<std::collections::HashMap<String, Vec<serde_json::Value>>> {
    if keys.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows: Vec<(String, Option<String>, Option<String>, Vec<String>, Vec<String>, String, NaiveDate, Option<NaiveDate>)> =
        sqlx::query_as(
            "SELECT cl.merchant_key, cb.name, ca.name, cl.tags_before, cl.tags_after, cl.source,
                    cl.created_at::date, COALESCE(cl.tx_date, i.occurred_on)
             FROM change_log cl
             LEFT JOIN items i ON i.id = cl.item_id
             LEFT JOIN categories cb ON cb.id = cl.category_before
             LEFT JOIN categories ca ON ca.id = cl.category_after
             WHERE cl.merchant_key = ANY($1)
             ORDER BY cl.created_at DESC",
        )
        .bind(keys)
        .fetch_all(pool)
        .await?;
    let mut out: std::collections::HashMap<String, Vec<serde_json::Value>> =
        std::collections::HashMap::new();
    for (key, cb, ca, tb, ta, source, date, tx_date) in rows {
        if out.get(&key).map_or(false, |v| v.len() >= 5) {
            continue;
        }
        out.entry(key).or_default().push(json!({
            "date": date,        // when the user made the change
            "t_date": tx_date,   // when the underlying transaction happened
            "cat_from": cb, "cat_to": ca,
            "tags_from": tb, "tags_to": ta,
            "src": source,
        }));
    }
    Ok(out)
}

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
        "SELECT i.id, i.merchant, i.description, i.amount_cents, i.occurred_on, c.name, i.tags,
                i.pluggy_category, i.mcc::bigint, i.operation_type, i.payment_method
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
    // Tags that are also categories are never suggested (the category covers it).
    let cat_norms = category_norms(pool).await?;
    let all_tags: Vec<String> = all_tags
        .into_iter()
        .filter(|t| !cat_norms.contains(&norm_key(t)))
        .collect();

    let merchants: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT merchant FROM items WHERE id = ANY($1) AND merchant IS NOT NULL",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;

    // Per merchant: the last `EXAMPLES_PER_MERCHANT` confirmed items that carry
    // tags — real ground truth for the AI to mirror.
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
    let mut keys: Vec<String> = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        if let Some((_, merchant, description, amount, date, cat, tags, pc, mcc, ot, pm)) = by_id.get(id) {
            let key = crate::services::change_log::merchant_key(merchant.as_deref(), description);
            keys.push(key);
            items.push(json!({
                "i": i,
                "desc": description,
                "merch": merchant,
                "amt": amount,
                "date": date.format("%Y-%m-%d").to_string(),
                "cat": cat,
                "tags": tags,
                "pc": pc, "mcc": mcc, "op": ot, "pay": pm,
            }));
        }
    }
    let history = load_change_history(pool, &keys).await?;
    for it in &mut items {
        let key = crate::services::change_log::merchant_key(
            it["merch"].as_str(),
            it["desc"].as_str().unwrap_or(""),
        );
        if let Some(h) = history.get(&key) {
            it["hist"] = json!(h);
        }
    }

    let tag_desc = tags::tag_descriptions(pool, &all_tags).await?;
    let diario = crate::services::diary::recent_diary(pool, 10).await?;
    let user = json!({ "all_tags": all_tags, "tag_desc": tag_desc, "diario": diario, "items": items, "ex": examples });
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
            let tags: Vec<String> = tags::normalize(&raw_tags)
                .into_iter()
                .filter(|t| !cat_norms.contains(&norm_key(t)))
                .collect();
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

/// Run the AI call for a **full** batch (kind = 'full'): one pass that suggests
/// BOTH a category and tags per item. Category names are removed from the tag
/// vocabulary and any suggested tag that collides with a category is dropped —
/// tags are situational, categories cover the "what".
async fn process_full_batch(pool: &PgPool, ai: &AiClient, batch_id: Uuid) -> Result<()> {
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
        "SELECT i.id, i.merchant, i.description, i.amount_cents, i.occurred_on, c.name, i.tags,
                i.pluggy_category, i.mcc::bigint, i.operation_type, i.payment_method
         FROM items i
         LEFT JOIN categories c ON c.id = i.category_id
         WHERE i.id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let by_id: HashMap<Uuid, _> = rows.into_iter().map(|r| (r.0, r)).collect();

    // Active categories (name + parent) for the AI to choose from, and their
    // normalized names for the tag→category collision filter.
    let categories: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT c.name, p.name FROM categories c
         LEFT JOIN categories p ON p.id = c.parent_id
         WHERE c.is_active ORDER BY c.name",
    )
    .fetch_all(pool)
    .await?;
    let cat_norms = category_norms(pool).await?;

    // Tag vocabulary: the user's existing tags MINUS category names.
    let all_tags: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT tag FROM items CROSS JOIN LATERAL unnest(tags) AS tag ORDER BY tag",
    )
    .fetch_all(pool)
    .await?;
    let all_tags: Vec<String> = all_tags
        .into_iter()
        .filter(|t| !cat_norms.contains(&norm_key(t)))
        .collect();

    let merchants: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT merchant FROM items WHERE id = ANY($1) AND merchant IS NOT NULL",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;

    // Per merchant: the last `EXAMPLES_PER_MERCHANT` confirmed items that carry
    // tags — real ground truth for the AI to mirror.
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

    // Compact item list, in batch (selection) order.
    let mut items = Vec::with_capacity(ids.len());
    let mut keys: Vec<String> = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        if let Some((_, merchant, description, amount, date, cat, tags, pc, mcc, ot, pm)) = by_id.get(id) {
            let key = crate::services::change_log::merchant_key(merchant.as_deref(), description);
            keys.push(key);
            items.push(json!({
                "i": i,
                "desc": description,
                "merch": merchant,
                "amt": amount,
                "date": date.format("%Y-%m-%d").to_string(),
                "cat": cat,
                "tags": tags,
                "pc": pc, "mcc": mcc, "op": ot, "pay": pm,
            }));
        }
    }
    let history = load_change_history(pool, &keys).await?;
    for it in &mut items {
        let key = crate::services::change_log::merchant_key(
            it["merch"].as_str(),
            it["desc"].as_str().unwrap_or(""),
        );
        if let Some(h) = history.get(&key) {
            it["hist"] = json!(h);
        }
    }

    let tag_desc = tags::tag_descriptions(pool, &all_tags).await?;
    let diario = crate::services::diary::recent_diary(pool, 10).await?;
    let user = json!({
        "categories": categories.iter().map(|(n, p)| match p { Some(p) => format!("{n} ({p})"), None => n.clone() }).collect::<Vec<_>>(),
        "all_tags": all_tags,
        "tag_desc": tag_desc,
        "diario": diario,
        "items": items,
        "ex": examples,
    });
    let system = build_full_system_prompt();
    let value = ai
        .chat_json(&system, json!(user.to_string()), ai.text_model(), None, "tag_batch")
        .await
        .context("AI tagging call failed")?;

    let mut seen = HashSet::new();
    let mut tx = pool.begin().await?;
    // Fallback: the model sometimes returns categories in a separate top-level
    // "categories" array (index → category) instead of inside "suggestions".
    let cats_by_index: std::collections::HashMap<usize, String> = value
        .get("categories")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let i = e.get("index").and_then(|x| x.as_u64())? as usize;
                    let c = e
                        .get("category")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    Some((i, c))
                })
                .collect()
        })
        .unwrap_or_default();
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
            // Deterministic collision filter: a tag that is also a category name
            // is dropped (the category suggestion covers it).
            let tags: Vec<String> = tags::normalize(&raw_tags)
                .into_iter()
                .filter(|t| !cat_norms.contains(&norm_key(t)))
                .collect();
            let mut category = entry
                .get("category")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if category.is_empty() {
                category = cats_by_index.get(&index).cloned().unwrap_or_default();
            }
            sqlx::query(
                "UPDATE ai_tag_suggestions SET suggested_tags = $1, suggested_category = $2
                 WHERE batch_id = $3 AND item_id = $4",
            )
            .bind(&tags)
            .bind(&category)
            .bind(batch_id)
            .bind(ids[index])
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Stable system prompt for 'full' batches: category + tags in one pass.
fn build_full_system_prompt() -> String {
    r#"Você é um especialista em finanças pessoais brasileiras. Para cada item, sugira uma CATEGORIA e TAGS. Responda APENAS com JSON válido (sem markdown, sem texto extra).

Esquema JSON exato:
{
  "suggestions": [
    { "index": integer, "category": string, "tags": [string] }
  ]
}

Regras:
- "index" é a posição (0-based) do item no array "items" da mensagem do usuário. NÃO invente índices; use apenas índices que existem em "items". Inclua um índice para CADA item — nenhum pode faltar.

CATEGORIA:
- Escolha entre as categorias de "categories" (formato "Nome (Grupo)"). Se nenhuma encaixar bem, proponha uma nova com o prefixo "nova: " (ex.: "nova: Pet shop"). Use isso com moderação.
- Categorize TODOS os itens, mesmo com pouca informação. Exceção — movimentos de dinheiro sem categoria (transferência entre contas próprias, pagamento de fatura de cartão, investimento, estorno de imposto): use "category": "".
- Guia: supermercado → "Supermercado"; restaurante/delivery/ifood → "Restaurantes"; posto/gasolina → "Transporte"; farmácia/saúde/clínica → "Saúde"; assinatura (streaming, apps, telefone) → "Assinaturas"; aluguel/condomínio/contas de casa → "Moradia"; loja/e-commerce/roupa → "Compras" (ou a categoria mais próxima existente); lazer/viagem/hotel → "Lazer"; imposto/taxa → "Outros"; compra internacional/foreign → "Outros" (ou "nova: " se fizer sentido).

TAGS:
- Sugira de 2 a 4 tags por item, curtas, em minúsculas e sem acentos, em português (ex: "viagem", "assinatura", "presente", "trabalho", "mercado"). Prefira 2-3 tags úteis (tema + contexto).
- TAGS SÃO SITUACIONAIS (contexto do gasto), NÃO a categoria. NUNCA sugira uma tag que seja igual a uma categoria de "categories" (ex.: se a categoria é "Compras", não sugira a tag "compras"; se é "Saúde", não use "saude") — a categoria já cobre isso.
- Se o item já tem tags, você pode mantê-las ou refiná-las.
- "all_tags" lista as tags que o usuário já usa (já exclui as que são categorias); "tag_desc" traz o significado de algumas delas — respeite esses significados.
- Prefira tags de "all_tags"; crie uma nova tag apenas se nenhuma existente encaixar bem.
- "ex" traz exemplos reais de itens já taggeados do mesmo comerciante — use-os como referência de estilo.
- "hist" traz o histórico das mudanças do usuário nesse comerciante — a decisão mais recente (sobre transação recente) reflete a preferência atual.
- "diario" traz anotações do usuário sobre a vida dele — use para interpretar contextos (ex.: uma mudança de cidade explica gastos com a tag "mudanca").
- Metadados por item: "pc" (categoria sugerida pelo banco), "mcc" (código do comerciante: 5817/5818/5968=assinatura, 5411=supermercado, 5812=restaurante), "op" (tipo de movimento: PIX, BOLETO, PORTABILIDADE_SALARIO), "pay" (PIX/TED/DOC) — use-os como evidência (ex.: "op":"PORTABILIDADE_SALARIO" → tag "salario"; "pay":"PIX" → tag "pix")."#
        .to_string()
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
- Sugira de 2 a 4 tags por item, curtas, em minúsculas e sem acentos, em português (ex: "viagem", "assinatura", "presente", "trabalho", "mercado"). Prefira 2-3 tags úteis (tema + contexto, ex.: "mercado" + "semanal", "combustivel" + "carro").
- Tags são SITUACIONAIS (o contexto do gasto), não a categoria. Se o item já tem tags, você pode mantê-las ou refiná-las.
- "all_tags" lista as tags que o usuário já usa; "tag_desc" traz o significado de algumas delas (escrito pelo usuário) — respeite esses significados ao escolher.
- Prefira tags de "all_tags" (e use os significados de "tag_desc"); crie uma nova tag apenas se nenhuma existente encaixar bem.
- "ex" traz exemplos reais de itens já taggeados do mesmo comerciante — use-os como referência de estilo.
- SEMPRE sugira tags — nenhum item deve ficar sem. Faça a melhor estimativa com descrição, comerciante, valor, data e categoria:
  * transferência/pix entre pessoas → "pix" (ou "pessoa" se recorrente para a mesma pessoa)
  * imposto/taxa (SEFAZ, IOF, IR, tarifa, anuidade) → "imposto" (ou "taxa")
  * estorno/refund → "estorno"
  * assinatura recorrente → "assinatura"
  * compra em loja/e-commerce genérico → "compras"
  * mercado/mercado/mercearia → "mercado"
  * refeição/delivery/restaurante → "delivery" ou "refeicao"
  * gasolina/posto → "combustivel"; farmácia → "farmacia"; pet → "pet"; saúde → "saude"
- Use as tags de "ex" como guia de estilo e vocabulário (mesmo nível de especificidade).
- "hist" traz o histórico das mudanças do usuário nesse comerciante (categoria/tags antes→depois, origem; "date" = quando mudou, "t_date" = quando a transação ocorreu). A decisão mais recente (sobre transação recente) reflete a preferência atual — use as tags mais recentes como referência.
- "diario" traz anotações do usuário sobre a vida dele — use para interpretar contextos (ex.: uma mudança de cidade explica gastos com a tag "mudanca").
- Metadados por item: "pc" (categoria sugerida pelo banco), "mcc" (código do comerciante: 5817/5818/5968=assinatura, 5411=supermercado, 5812=restaurante), "op" (tipo de movimento: PIX, BOLETO, PORTABILIDADE_SALARIO), "pay" (PIX/TED/DOC) — use-os como evidência para as tags (ex.: "op":"PORTABILIDADE_SALARIO" → tag "salario"; "pay":"PIX" → tag "pix")."#
        .to_string()
}

/// Run the AI **categorization** call for a batch and store per-item category
/// proposals in `suggested_category` (an existing category name, "nova: <nome>",
/// or '' when the AI can't classify — transfers etc.).
pub async fn process_categorize_batch(pool: &PgPool, ai: &AiClient, batch_id: Uuid) -> Result<()> {
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
        "SELECT i.id, i.merchant, i.description, i.amount_cents, i.occurred_on, c.name, i.tags,
                i.pluggy_category, i.mcc::bigint, i.operation_type, i.payment_method
         FROM items i
         LEFT JOIN categories c ON c.id = i.category_id
         WHERE i.id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let by_id: HashMap<Uuid, _> = rows.into_iter().map(|r| (r.0, r)).collect();

    // Active categories (name + parent) for the AI to choose from.
    let categories: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT c.name, p.name FROM categories c
         LEFT JOIN categories p ON p.id = c.parent_id
         WHERE c.is_active ORDER BY c.name",
    )
    .fetch_all(pool)
    .await?;

    // Normalize an identity: strip accents, lowercase, drop non-alphanumerics.
    let norm_key = |x: &str| -> String {
        tags::strip_accents(x)
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect()
    };

    // The user's own tagged examples (description, merchant, amount, date,
    // category, tags) — ground truth for the AI to mirror. Indexed by
    // merchant-or-description identity.
    let ex_rows: Vec<(String, Option<String>, i64, NaiveDate, Option<String>, Vec<String>)> =
        sqlx::query_as(
            "SELECT i.description, i.merchant, i.amount_cents, i.occurred_on, c.name, i.tags
             FROM items i LEFT JOIN categories c ON c.id = i.category_id
             WHERE cardinality(i.tags) > 0 AND i.status = 'confirmed'
             ORDER BY i.occurred_on DESC, i.created_at DESC LIMIT 500",
        )
        .fetch_all(pool)
        .await?;
    let mut ex_groups: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
    for (desc, merchant, amt, date, cat, tags) in ex_rows {
        let key = merchant
            .as_deref()
            .map(|m| norm_key(m))
            .unwrap_or_else(|| norm_key(&desc));
        let v = json!({ "desc": desc, "amt": amt, "date": date, "cat": cat, "tags": tags });
        match ex_groups.iter_mut().find(|(k, _)| *k == key) {
            Some(g) => {
                if g.1.len() < 3 {
                    g.1.push(v);
                }
            }
            None => ex_groups.push((key, vec![v])),
        }
    }

    // Identity match: equal, or one contains the other (min 4 chars).
    let identity_matches = |a: &str, b: &str| -> bool {
        let a = norm_key(a);
        let b = norm_key(b);
        a == b || (a.len() >= 4 && b.len() >= 4 && (a.contains(&b) || b.contains(&a)))
    };

    let all_tags: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT tag FROM items CROSS JOIN LATERAL unnest(tags) AS tag ORDER BY tag",
    )
    .fetch_all(pool)
    .await?;
    let tag_desc = tags::tag_descriptions(pool, &all_tags).await?;

    let mut items = Vec::with_capacity(ids.len());
    let mut keys: Vec<String> = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let Some((_, merchant, description, amount, date, cat, tags, pc, mcc, ot, pm)) = by_id.get(id) else {
            continue;
        };
        let identity = merchant
            .as_deref()
            .map(|m| norm_key(m))
            .unwrap_or_else(|| norm_key(description));
        let ex: Vec<serde_json::Value> = ex_groups
            .iter()
            .filter(|(k, _)| identity_matches(&identity, k))
            .flat_map(|(_, v)| v.iter().cloned())
            .take(4)
            .collect();
        keys.push(identity);
        items.push(json!({
            "i": i,
            "desc": description,
            "merch": merchant,
            "amt": amount,
            "date": date.format("%Y-%m-%d").to_string(),
            "cat": cat,
            "tags": tags,
            "pc": pc, "mcc": mcc, "op": ot, "pay": pm,
            "ex": ex,
        }));
    }
    let history = load_change_history(pool, &keys).await?;
    for it in &mut items {
        let key = crate::services::change_log::merchant_key(
            it["merch"].as_str(),
            it["desc"].as_str().unwrap_or(""),
        );
        if let Some(h) = history.get(&key) {
            it["hist"] = json!(h);
        }
    }

    let diario = crate::services::diary::recent_diary(pool, 10).await?;
    let user = json!({
        "categories": categories.iter().map(|(n, p)| match p { Some(p) => format!("{n} ({p})"), None => n.clone() }).collect::<Vec<_>>(),
        "tag_desc": tag_desc,
        "diario": diario,
        "items": items,
    });
    let system = build_categorize_system_prompt();
    let value = ai
        .chat_json(&system, json!(user.to_string()), ai.text_model(), None, "categorize")
        .await
        .context("AI categorization call failed")?;

    let mut seen = HashSet::new();
    let mut tx = pool.begin().await?;
    if let Some(arr) = value.get("categories").and_then(|v| v.as_array()) {
        for entry in arr {
            let Some(index) = entry.get("index").and_then(|x| x.as_u64()) else {
                continue;
            };
            let index = index as usize;
            if index >= ids.len() || !seen.insert(index) {
                continue;
            }
            let category = entry
                .get("category")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if category.is_empty() {
                continue;
            }
            sqlx::query(
                "UPDATE ai_tag_suggestions SET suggested_category = $1
                 WHERE batch_id = $2 AND item_id = $3",
            )
            .bind(&category)
            .bind(batch_id)
            .bind(ids[index])
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

fn build_categorize_system_prompt() -> String {
    r#"Você é um especialista em finanças pessoais brasileiras. Sua tarefa é sugerir a CATEGORIA de itens de um controle financeiro. Responda APENAS com JSON válido (sem markdown, sem texto extra).

Esquema JSON exato:
{
  "categories": [
    { "index": integer, "category": string }
  ]
}

Regras:
- "index" é a posição (0-based) do item no array "items" da mensagem do usuário. NÃO invente índices.
- "category" deve ser EXATAMENTE uma das categorias da lista "categories" (pode incluir o subgrupo entre parênteses quando houver, ex.: "Restaurantes", "Moradia (Casa)").
- Se nenhuma categoria existente encaixar bem, proponha uma nova com o prefixo "nova: " (ex.: "nova: Pet shop"). Use isso com moderação.
- "hist" traz o histórico das mudanças que o usuário fez nesse comerciante (categoria/tags antes→depois, origem: item_edit | bulk | memory_apply | ai_apply). "date" = quando o usuário mudou; "t_date" = quando a transação aconteceu. A decisão MAIS RECENTE (e sobre transação mais recente) é a preferência atual — siga-a; mudanças em compras antigas valem menos. Se o histórico mostra trocas frequentes, seja conservador (proponha a decisão mais recente).
- "ex" de cada item traz exemplos reais do usuário (mesma loja/descrição) já taggeados e categorizados — use a categoria e as tags deles como referência de estilo e de decisão.
- "tag_desc" explica o significado das tags do usuário — use para interpretar o contexto do gasto (ex.: tag "advogado" = pagamentos para o advogado, então compras nessa tag são pessoais, não restaurante).
- Metadados por item: "pc" = categoria sugerida pelo banco (ex.: "Food delivery", "Supermarkets"); "mcc" = código de categoria do comerciante (5411=supermercado, 5812=restaurante, 5541/5542=posto, 5912=farmácia, 5817/5818/5968=assinatura, 7011=hotel); "op" = tipo de movimento (PORTABILIDADE_SALARIO=salário, RESGATE_APLIC_FINANCEIRA=investimento, PIX, BOLETO, TARIFA_SERVICOS, ENCARGOS_JUROS); "pay" = método (PIX/TED/DOC). Use-os como evidência — mcc e pc são fortes pistas da categoria.
- Categorize TODOS os itens de "items" — inclua um índice para cada um. Mesmo com pouca informação, escolha a melhor categoria (ou proponha "nova: ").
- Guia: supermercado → "Supermercado"; restaurante/delivery/ifood → "Restaurantes"; posto/gasolina → "Transporte"; farmácia/saúde/clínica → "Saúde"; assinatura (streaming, apps, telefone) → "Assinaturas"; aluguel/condomínio/contas de casa → "Moradia"; loja/e-commerce/roupa → "Compras" (ou a categoria mais próxima existente); lazer/viagem/hotel → "Lazer"; imposto/taxa → "Outros"; compra internacional/foreign → "Outros" (ou "nova: " se fizer sentido).
- Exceção: apenas movimentos de dinheiro sem categoria (transferência entre contas próprias, pagamento de fatura de cartão, investimento, estorno de imposto) podem ser omitidos — não inclua o índice deles."#
        .to_string()
}

/// Add tags to an item (merge + dedupe, keeping first-occurrence order), then mark
/// the suggestion as applied. Kind-aware: 'categorize' batches apply the category
/// instead of tags. `tags` are normalized server-side.
pub async fn apply_suggestion(
    pool: &PgPool,
    suggestion_id: Uuid,
    tags: Option<Vec<String>>,
    category: Option<String>,
) -> Result<serde_json::Value, AppError> {
    let row: Option<(Uuid, Vec<String>, String, String, String)> = sqlx::query_as(
        "SELECT s.item_id, s.suggested_tags, s.status, s.suggested_category, b.kind
         FROM ai_tag_suggestions s JOIN ai_tag_batches b ON b.id = s.batch_id
         WHERE s.id = $1",
    )
    .bind(suggestion_id)
    .fetch_optional(pool)
    .await?;
    let Some((item_id, stored_tags, status, stored_category, kind)) = row else {
        return Err(AppError::not_found("suggestion not found"));
    };
    if status != "pending" {
        return Err(AppError::conflict("suggestion already reviewed"));
    }

    // Categorize batches: apply the category (find-or-create) instead of tags.
    if kind == "categorize" {
        return apply_category_suggestion(pool, suggestion_id, item_id, &stored_category).await;
    }

    // 'tags' and 'full' batches share the per-field apply: `None` = leave that
    // field untouched; both `None` = apply everything that was suggested. A
    // category override works on 'tags' batches too (the review modal lets the
    // user pick a category even when the batch only proposed tags).
    apply_full_suggestion(pool, suggestion_id, item_id, &stored_tags, &stored_category, tags, category).await
}

/// Sentinel category value: the caller wants to CLEAR the item's category.
const CLEAR_CATEGORY: &str = "__none__";

/// Apply a suggestion with per-field semantics ('tags' and 'full' batches):
/// only the fields the caller sent (Some) are touched; if both are None, apply
/// both suggestions. `category` may be an existing name, "nova: <nome>", '' to
/// skip, or "__none__" to clear the item's category.
async fn apply_full_suggestion(
    pool: &PgPool,
    suggestion_id: Uuid,
    item_id: Uuid,
    stored_tags: &[String],
    stored_category: &str,
    tags: Option<Vec<String>>,
    category: Option<String>,
) -> Result<serde_json::Value, AppError> {
    let apply_tags: Option<Vec<String>> = match &tags {
        Some(t) => Some(tags::normalize(t)),
        None if category.is_none() => Some(stored_tags.to_vec()),
        None => None,
    };
    let apply_cat: Option<String> = match &category {
        Some(c) if c.trim().is_empty() => None, // explicit "skip category"
        Some(c) => Some(c.trim().to_string()),
        None if tags.is_none() => {
            if stored_category.trim().is_empty() {
                None
            } else {
                Some(stored_category.trim().to_string())
            }
        }
        None => None,
    };
    if apply_tags.is_none() && apply_cat.is_none() {
        return Err(AppError::bad_request("nothing to apply"));
    }

    let before: (Option<Uuid>, Vec<String>) =
        sqlx::query_as("SELECT category_id, tags FROM items WHERE id = $1")
            .bind(item_id)
            .fetch_one(pool)
            .await?;

    let mut tx = pool.begin().await?;
    if let Some(c) = &apply_cat {
        if c == CLEAR_CATEGORY {
            sqlx::query("UPDATE items SET category_id = NULL, updated_at = now() WHERE id = $1")
                .bind(item_id)
                .execute(&mut *tx)
                .await?;
        } else {
            let cat_id = resolve_category_id(pool, c).await?;
            sqlx::query("UPDATE items SET category_id = $1, updated_at = now() WHERE id = $2")
                .bind(cat_id)
                .bind(item_id)
                .execute(&mut *tx)
                .await?;
        }
    }
    if let Some(t) = &apply_tags {
        if !t.is_empty() {
            add_tags_to_item(&mut tx, item_id, t).await?;
        }
    }
    sqlx::query("UPDATE ai_tag_suggestions SET status = 'applied' WHERE id = $1")
        .bind(suggestion_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let after: (Option<Uuid>, Vec<String>) =
        sqlx::query_as("SELECT category_id, tags FROM items WHERE id = $1")
            .bind(item_id)
            .fetch_one(pool)
            .await?;
    let _ = crate::services::change_log::log_item_change(
        pool,
        item_id,
        before.0,
        after.0,
        &before.1,
        &after.1,
        "ai_apply",
    )
    .await;

    Ok(json!({
        "ok": true,
        "item_id": item_id,
        "category": apply_cat,
        "tags": apply_tags,
    }))
}

/// Resolve a category proposal to a category id: "nova: <nome>" creates the
/// category; otherwise match an existing one by normalized name (accent/case-
/// insensitive), falling back to the base name before "(" (the AI may add the
/// parent group). If nothing matches, create it — the user explicitly applied.
async fn resolve_category_id(pool: &PgPool, proposal: &str) -> Result<Option<Uuid>, AppError> {
    let proposal = proposal.trim();
    if proposal.is_empty() {
        return Err(AppError::bad_request("suggestion has no category"));
    }
    let name = proposal
        .split_once(':')
        .map(|(_, n)| n.trim().to_string())
        .unwrap_or_else(|| proposal.to_string());
    let normalized = tags::strip_accents(&name).to_lowercase();
    let base = normalized.split('(').next().map(str::trim).unwrap_or(&normalized);

    // Match in Rust (Postgres has no strip_accents).
    let cats: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM categories WHERE is_active")
            .fetch_all(pool)
            .await?;
    let find = |target: &str| {
        cats.iter()
            .find(|(_, n)| tags::strip_accents(n).to_lowercase() == target)
            .map(|(id, _)| *id)
    };
    let category_id = find(&normalized).or_else(|| find(base));
    Ok(match category_id {
        Some(id) => Some(id),
        None => {
            let (id,): (Uuid,) = sqlx::query_as(
                "INSERT INTO categories (name) VALUES ($1) RETURNING id",
            )
            .bind(&name)
            .fetch_one(pool)
            .await?;
            Some(id)
        }
    })
}

/// Apply a category proposal: sets the item's category and feeds merchant memory.
async fn apply_category_suggestion(
    pool: &PgPool,
    suggestion_id: Uuid,
    item_id: Uuid,
    proposal: &str,
) -> Result<serde_json::Value, AppError> {
    let category_id = resolve_category_id(pool, proposal).await?;
    let name = proposal
        .split_once(':')
        .map(|(_, n)| n.trim().to_string())
        .unwrap_or_else(|| proposal.trim().to_string());

    let before: (Option<Uuid>, Vec<String>) =
        sqlx::query_as("SELECT category_id, tags FROM items WHERE id = $1")
            .bind(item_id)
            .fetch_one(pool)
            .await?;

    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE items SET category_id = $1, suggested_category = NULL, updated_at = now()
         WHERE id = $2",
    )
    .bind(category_id)
    .bind(item_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE ai_tag_suggestions SET status = 'applied' WHERE id = $1")
        .bind(suggestion_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    // Log the change (durable history for the AI).
    let after: (Option<Uuid>, Vec<String>) =
        sqlx::query_as("SELECT category_id, tags FROM items WHERE id = $1")
            .bind(item_id)
            .fetch_one(pool)
            .await?;
    let _ = crate::services::change_log::log_item_change(
        pool,
        item_id,
        before.0,
        after.0,
        &before.1,
        &after.1,
        "ai_apply",
    )
    .await;

    Ok(json!({ "ok": true, "item_id": item_id, "category_id": category_id, "category": name }))
}

pub async fn dismiss_suggestion(
    pool: &PgPool,
    suggestion_id: Uuid,
) -> Result<serde_json::Value, AppError> {
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
/// Kind-aware: categorize suggestions set the category.
pub async fn apply_all(pool: &PgPool, batch_id: Option<Uuid>) -> Result<serde_json::Value, AppError> {
    let rows: Vec<(Uuid, Uuid, Vec<String>, String, String)> = sqlx::query_as(
        "SELECT s.id, s.item_id, s.suggested_tags, s.suggested_category, b.kind
         FROM ai_tag_suggestions s JOIN ai_tag_batches b ON b.id = s.batch_id
         WHERE s.status = 'pending' AND ($1::uuid IS NULL OR s.batch_id = $1)",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    let mut tx = pool.begin().await?;
    let mut pending_log: Vec<(Uuid, (Option<Uuid>, Vec<String>))> = Vec::new();
    for (id, item_id, tags, category, kind) in &rows {
        let before: (Option<Uuid>, Vec<String>) =
            sqlx::query_as("SELECT category_id, tags FROM items WHERE id = $1")
                .bind(item_id)
                .fetch_one(pool)
                .await?;
        if kind == "categorize" {
            if !category.is_empty() {
                let cat_id = resolve_category_id(pool, category).await?;
                sqlx::query("UPDATE items SET category_id = $1, updated_at = now() WHERE id = $2")
                    .bind(cat_id)
                    .bind(item_id)
                    .execute(&mut *tx)
                    .await?;
            }
        } else if kind == "full" {
            if !category.is_empty() {
                let cat_id = resolve_category_id(pool, category).await?;
                sqlx::query("UPDATE items SET category_id = $1, updated_at = now() WHERE id = $2")
                    .bind(cat_id)
                    .bind(item_id)
                    .execute(&mut *tx)
                    .await?;
            }
            if !tags.is_empty() {
                add_tags_to_item(&mut tx, *item_id, tags).await?;
            }
        } else if !tags.is_empty() {
            add_tags_to_item(&mut tx, *item_id, tags).await?;
        }
        sqlx::query("UPDATE ai_tag_suggestions SET status = 'applied' WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        pending_log.push((*item_id, before));
    }
    tx.commit().await?;

    // Log after commit (the "after" read must see the committed changes).
    for (item_id, before) in pending_log {
        let after: (Option<Uuid>, Vec<String>) =
            sqlx::query_as("SELECT category_id, tags FROM items WHERE id = $1")
                .bind(item_id)
                .fetch_one(pool)
                .await?;
        let _ = crate::services::change_log::log_item_change(
            pool,
            item_id,
            before.0,
            after.0,
            &before.1,
            &after.1,
            "ai_apply",
        )
        .await;
    }
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
