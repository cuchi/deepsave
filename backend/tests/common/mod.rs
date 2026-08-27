//! Shared helpers for integration tests.
#![allow(dead_code)]

use deepsave_backend::config::Config;
use deepsave_backend::models::DocumentRow;
use deepsave_backend::services::ai::AiClient;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Apply all migrations to a `sqlx::test` database.
pub async fn migrate(pool: &PgPool) {
    sqlx::migrate!("./migrations").run(pool).await.unwrap();
}

/// Absolute path to a fixture file under `tests/fixtures/`.
pub fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/// A Config pointing at a mock DeepSeek base URL (for tests only).
pub fn test_config(base_url: String) -> Config {
    Config {
        database_url: String::new(),
        port: 0,
        session_secret: "test-secret-at-least-32-bytes".to_string(),
        password_hash: None,
        password_plain: None,
        cookie_secure: false,
        static_dir: String::new(),
        storage_dir: String::new(),
        deepseek_api_key: Some("test-key".to_string()),
        deepseek_base_url: base_url,
        deepseek_model: "deepseek-v4-flash".to_string(),
        deepseek_vision_model: "deepseek-v4-flash-vision-exp".to_string(),
        deepseek_pro_model: "deepseek-v4-pro".to_string(),
        deepseek_input_price_per_m: 0.27,
        deepseek_cache_hit_price_per_m: 0.07,
        deepseek_output_price_per_m: 1.10,
        coverage_months: 12,
    }
}

pub fn ai_client(pool: PgPool, base_url: String) -> AiClient {
    let config = test_config(base_url);
    AiClient::new(&config, pool)
}

/// Insert a `pending` document row pointing at a fixture file.
pub async fn insert_document(
    pool: &PgPool,
    kind: &str,
    filename: &str,
    content_type: &str,
    file_path: &str,
) -> DocumentRow {
    sqlx::query_as::<_, DocumentRow>(
        "INSERT INTO documents (kind, filename, content_type, sha256, file_path, status)
         VALUES ($1, $2, $3, $4, $5, 'pending')
         RETURNING id, kind, account_id, source_id, filename, content_type, sha256,
                   file_path, status, error_message, ocr_text, uploaded_at, processed_at",
    )
    .bind(kind)
    .bind(filename)
    .bind(content_type)
    .bind(Uuid::new_v4().to_string())
    .bind(file_path)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Start a mock DeepSeek server that returns a fixed extraction (with the given items).
pub async fn mount_deepseek_mock(items: Value) -> MockServer {
    let server = MockServer::start().await;
    let content = json!({
        "document_type": "bank_statement",
        "merchant": null,
        "date": null,
        "currency": "BRL",
        "items": items,
    })
    .to_string();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "mock",
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": content },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 10
            }
        })))
        .mount(&server)
        .await;

    server
}
