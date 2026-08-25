use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;

/// Parsed DeepSeek extraction output (validated in the retry loop).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AiItem {
    pub description: String,
    #[serde(default)]
    pub merchant: Option<String>,
    pub amount_cents: i64,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub installment: Option<i32>,
    #[serde(default)]
    pub installment_count: Option<i32>,
    #[serde(default)]
    pub date: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AiExtraction {
    #[serde(default)]
    pub document_type: Option<String>,
    #[serde(default)]
    pub merchant: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub items: Vec<AiItem>,
}

#[derive(Clone)]
pub struct AiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    vision_model: String,
    input_price_per_m: f64,
    cache_hit_price_per_m: f64,
    output_price_per_m: f64,
    pool: PgPool,
}

impl AiClient {
    pub fn new(config: &Config, pool: PgPool) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: config.deepseek_api_key.clone().unwrap_or_default(),
            base_url: config.deepseek_base_url.trim_end_matches('/').to_string(),
            model: config.deepseek_model.clone(),
            vision_model: config.deepseek_vision_model.clone(),
            input_price_per_m: config.deepseek_input_price_per_m,
            cache_hit_price_per_m: config.deepseek_cache_hit_price_per_m,
            output_price_per_m: config.deepseek_output_price_per_m,
            pool,
        }
    }

    pub fn enabled(&self) -> bool {
        !self.api_key.is_empty()
    }

    pub fn text_model(&self) -> &str {
        &self.model
    }

    pub fn vision_model(&self) -> &str {
        &self.vision_model
    }

    /// Build the user content for a vision request (image + optional prompt text).
    pub fn vision_user(image_bytes: &[u8], mime: &str, prompt: &str) -> Value {
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
        json!([
            { "type": "text", "text": prompt },
            { "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{b64}") } }
        ])
    }

    /// Call the chat completions API with JSON mode; returns the parsed JSON content.
    /// Every call is recorded into `ai_calls` (tokens, cache hits, estimated cost).
    pub async fn chat_json(
        &self,
        system: &str,
        user: Value,
        model: &str,
        document_id: Option<Uuid>,
        purpose: &str,
    ) -> Result<Value> {
        if !self.enabled() {
            return Err(anyhow!("DeepSeek API key not configured"));
        }

        let url = format!("{}/chat/completions", self.base_url);
        let body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
            "response_format": { "type": "json_object" },
            "temperature": 0.0,
            "stream": false,
        });

        let start = Instant::now();
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await;
        let duration_ms = start.elapsed().as_millis() as i32;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                self.record_call(
                    document_id, purpose, model, 0, 0, 0, 0, 0, 0.0, duration_ms, "error",
                    Some(e.to_string()),
                )
                .await;
                return Err(anyhow!("DeepSeek request failed: {e}"));
            }
        };

        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            self.record_call(
                document_id, purpose, model, 0, 0, 0, 0, 0, 0.0, duration_ms, "error",
                Some(truncate(&text, 500)),
            )
            .await;
            return Err(anyhow!(
                "DeepSeek API error {status}: {}",
                truncate(&text, 500)
            ));
        }

        let parsed: Value =
            serde_json::from_str(&text).context("failed to parse DeepSeek response body")?;
        let usage = &parsed["usage"];
        let prompt_tokens = usage["prompt_tokens"].as_i64().unwrap_or(0) as i32;
        let completion_tokens = usage["completion_tokens"].as_i64().unwrap_or(0) as i32;
        let total_tokens = usage["total_tokens"].as_i64().unwrap_or(0) as i32;
        let cache_hit = usage["prompt_cache_hit_tokens"].as_i64().unwrap_or(0) as i32;
        let cache_miss = usage["prompt_cache_miss_tokens"].as_i64().unwrap_or(0) as i32;
        let cost = (cache_miss as f64 / 1_000_000.0) * self.input_price_per_m
            + (cache_hit as f64 / 1_000_000.0) * self.cache_hit_price_per_m
            + (completion_tokens as f64 / 1_000_000.0) * self.output_price_per_m;

        let content = parsed["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Parse the model's JSON content. The API call itself succeeded, so record
        // it even if the content failed to parse (the ingest layer may retry).
        let content_json = serde_json::from_str::<Value>(&content);
        self.record_call(
            document_id,
            purpose,
            model,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cache_hit,
            cache_miss,
            cost,
            duration_ms,
            "ok",
            content_json.as_ref().err().map(|e| format!("invalid JSON: {e}")),
        )
        .await;

        content_json.context("AI returned invalid JSON")
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_call(
        &self,
        document_id: Option<Uuid>,
        purpose: &str,
        model: &str,
        prompt_tokens: i32,
        completion_tokens: i32,
        total_tokens: i32,
        cache_hit_tokens: i32,
        cache_miss_tokens: i32,
        cost_usd: f64,
        duration_ms: i32,
        status: &str,
        error_message: Option<String>,
    ) {
        let _ = sqlx::query(
            "INSERT INTO ai_calls
               (document_id, purpose, model, prompt_tokens, completion_tokens, total_tokens,
                cache_hit_tokens, cache_miss_tokens, cost_usd, duration_ms, status, error_message)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(document_id)
        .bind(purpose)
        .bind(model)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(total_tokens)
        .bind(cache_hit_tokens)
        .bind(cache_miss_tokens)
        .bind(cost_usd)
        .bind(duration_ms)
        .bind(status)
        .bind(error_message)
        .execute(&self.pool)
        .await;
    }
}

/// Build the (mostly) stable system prompt used for extraction.
///
/// The system prompt is a long, stable prefix (instructions + category list + memory)
/// so DeepSeek's context cache can serve it cheaply; the variable document goes in
/// the user message.
pub async fn build_extraction_system_prompt(pool: &PgPool) -> Result<String> {
    let cats: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM categories WHERE is_active ORDER BY name")
            .fetch_all(pool)
            .await?;
    let cat_list = cats
        .iter()
        .map(|c| c.0.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let memory: Vec<(String, String, Vec<String>)> = sqlx::query_as(
        "SELECT m.merchant, COALESCE(c.name, ''), m.tags
         FROM merchant_memory m
         LEFT JOIN categories c ON c.id = m.category_id
         WHERE m.confirm_count >= 2
         ORDER BY m.confirm_count DESC
         LIMIT 50",
    )
    .fetch_all(pool)
    .await?;
    let mem_lines = memory
        .iter()
        .filter(|(_, c, t)| !c.is_empty() || !t.is_empty())
        .map(|(m, c, t)| {
            if t.is_empty() {
                format!("- {m} → {c}")
            } else {
                format!("- {m} → {c} [{}]", t.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        r#"Você é um extrator de dados financeiros brasileiros. Responda APENAS com JSON válido (sem markdown, sem texto extra).

Esquema JSON exato:
{{
  "document_type": "receipt" | "bank_statement" | "card_statement" | "payment_slip",
  "merchant": string | null,
  "date": "YYYY-MM-DD" | null,
  "currency": "BRL",
  "items": [
    {{
      "description": string,
      "merchant": string | null,
      "amount_cents": integer,
      "category": string | null,
      "tags": [string],
      "kind": "expense" | "income" | "refund" | "internal",
      "installment": integer | null,
      "installment_count": integer | null,
      "date": "YYYY-MM-DD" | null
    }}
  ]
}}

Regras:
- amount_cents é um inteiro em centavos. NEGATIVO = despesa/saída. Positivo = receita/entrada.
- "kind" padrão é "expense". Use "income" para receitas (inclui Pix/TED recebidos), "refund" para estornos.
- Use "internal" para pagamentos da fatura do cartão de crédito (ex: "Pagamento Fatura"), investimentos (ex: emissão/resgate de CDB, impostos de fundos) e transferências entre contas do próprio usuário (ex: transferência da conta Nubank para a conta C6, Pix entre contas dele). Eles são rastreados mas NÃO contam como despesa/receita.
- Ignore cabeçalhos, saldos e linhas de total. Extraia apenas itens individuais de gasto/receita.
- Para parcelas (ex: 7/10), preencha installment=7 e installment_count=10.
- "date" de cada item é a data da transação; se o documento tiver uma data única, pode repeti-la.
- Use, quando possível, uma destas categorias: {cat_list}.

Memória de categorização (comerciante → categoria):
{mem_lines}
"#
    ))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
