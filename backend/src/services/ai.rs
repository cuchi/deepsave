use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;

#[derive(Clone)]
pub struct AiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    input_price_per_m: f64,
    cache_hit_price_per_m: f64,
    output_price_per_m: f64,
    pool: PgPool,
}

impl AiClient {
    pub fn new(config: &Config, pool: PgPool) -> Self {
        Self {
            // Generous call timeout: a 100+ item batch takes a few minutes.
            // Without one, a hung connection would freeze the worker forever.
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("failed to build HTTP client"),
            api_key: config.deepseek_api_key.clone().unwrap_or_default(),
            base_url: config.deepseek_base_url.trim_end_matches('/').to_string(),
            model: config.deepseek_model.clone(),
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
        let snippet: String = text.chars().take(500).collect();
        if !status.is_success() {
            self.record_call(
                document_id, purpose, model, 0, 0, 0, 0, 0, 0.0, duration_ms, "error",
                Some(snippet.clone()),
            )
            .await;
            return Err(anyhow!("DeepSeek API error {status}: {snippet}"));
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
