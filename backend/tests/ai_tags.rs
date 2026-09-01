//! Integration tests for AI-assisted bulk tagging (`/ai-tags/*`).
//!
//! Core logic is exercised directly via `services::ai_tags` (no HTTP harness),
//! with a mocked DeepSeek API, mirroring the rest of the suite.

use deepsave_backend::error::AppError;
use deepsave_backend::services::ai_tags;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;

async fn insert_item(
    pool: &PgPool,
    description: &str,
    merchant: Option<&str>,
    tags: &[&str],
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO items (kind, status, source, occurred_on, merchant, description, amount_cents, tags)
         VALUES ('expense', 'confirmed', 'manual', '2026-07-01', $1, $2, -1000, $3)
         RETURNING id",
    )
    .bind(merchant)
    .bind(description)
    .bind(tags.iter().map(|t| t.to_string()).collect::<Vec<_>>())
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn suggestion_tags(pool: &PgPool, batch_id: Uuid, item_id: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT suggested_tags FROM ai_tag_suggestions WHERE batch_id = $1 AND item_id = $2",
    )
    .bind(batch_id)
    .bind(item_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn suggestion_status(pool: &PgPool, batch_id: Uuid, item_id: Uuid) -> String {
    sqlx::query_scalar(
        "SELECT status FROM ai_tag_suggestions WHERE batch_id = $1 AND item_id = $2",
    )
    .bind(batch_id)
    .bind(item_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Mock DeepSeek returning a fixed tag-suggestion payload.
async fn mount_tag_mock(key: &str, payload: Value) -> MockServer {
    let server = MockServer::start().await;
    let content = json!({ key: payload }).to_string();

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

#[sqlx::test]
async fn enqueue_creates_batch_and_suggestions(pool: PgPool) {
    common::migrate(&pool).await;
    let a = insert_item(&pool, "Padaria", Some("Pão Quente"), &[]).await;
    let b = insert_item(&pool, "Farmácia", Some("Droga Raia"), &[]).await;

    let batch = ai_tags::enqueue_batch(&pool, vec![a, b], "tags").await.unwrap();

    assert_eq!(batch.status, "pending");
    assert_eq!(batch.item_count, 2);
    assert!(batch.error_message.is_none());

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_tag_suggestions WHERE batch_id = $1",
    )
    .bind(batch.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2);
}

#[sqlx::test]
async fn enqueue_validation_errors(pool: PgPool) {
    common::migrate(&pool).await;
    let a = insert_item(&pool, "Padaria", Some("Pão Quente"), &[]).await;

    match ai_tags::enqueue_batch(&pool, vec![], "tags").await {
        Err(AppError::BadRequest(m)) => assert_eq!(m, "ids must not be empty"),
        other => panic!("expected BadRequest, got {other:?}"),
    }

    let too_many: Vec<Uuid> = (0..=ai_tags::MAX_BATCH_ITEMS).map(|_| Uuid::new_v4()).collect();
    match ai_tags::enqueue_batch(&pool, too_many, "tags").await {
        Err(AppError::BadRequest(m)) => assert!(m.starts_with("too many ids")),
        other => panic!("expected BadRequest, got {other:?}"),
    }

    match ai_tags::enqueue_batch(&pool, vec![a, Uuid::new_v4()], "tags").await {
        // Unknown ids are skipped; at least one exists → still enqueues.
        Ok(batch) => assert_eq!(batch.item_count, 1),
        other => panic!("expected Ok, got {other:?}"),
    }

    match ai_tags::enqueue_batch(&pool, vec![Uuid::new_v4()], "tags").await {
        Err(AppError::BadRequest(m)) => assert_eq!(m, "none of the selected ids exist"),
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

#[sqlx::test]
async fn process_batch_fills_suggestions_from_ai(pool: PgPool) {
    common::migrate(&pool).await;
    let a = insert_item(&pool, "Padaria", Some("Pão Quente"), &[]).await;
    let b = insert_item(&pool, "Farmácia", Some("Droga Raia"), &[]).await;
    let c = insert_item(&pool, "Netflix", Some("NETFLIX"), &["streaming"]).await;
    // A tagged example for the same merchant (should be sent to the AI).
    insert_item(&pool, "Netflix assinatura", Some("NETFLIX"), &["streaming", "assinatura"]).await;

    let server = mount_tag_mock(
        "suggestions",
        json!([
            { "index": 0, "tags": [" padaria ", "Café da manhã"] },
            { "index": 1, "tags": ["saude", "remedio"] },
            // index 2 omitted by the model → stays empty.
            { "index": 99, "tags": ["fora do intervalo"] },   // invalid, skipped
            { "index": 0, "tags": ["duplicado"] },            // duplicate index, skipped
            { "tags": ["sem index"] }                          // invalid, skipped
        ]),
    )
    .await;
    let ai = common::ai_client(pool.clone(), server.uri());

    let batch = ai_tags::enqueue_batch(&pool, vec![a, b, c], "tags").await.unwrap();
    ai_tags::process_batch(&pool, &ai, batch.id).await.unwrap();

    assert_eq!(suggestion_tags(&pool, batch.id, a).await, vec!["padaria", "cafe da manha"]);
    // "saude" collides with the fixture's "Saúde" category — the collision
    // filter drops it (tags must not repeat categories).
    assert_eq!(suggestion_tags(&pool, batch.id, b).await, vec!["remedio"]);
    // Untouched by the model: stays empty.
    assert_eq!(suggestion_tags(&pool, batch.id, c).await, Vec::<String>::new());

    // The AI call was recorded (purpose = tag_batch, no document).
    let purpose: String = sqlx::query_scalar(
        "SELECT purpose FROM ai_calls WHERE document_id IS NULL ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(purpose, "tag_batch");
}

#[sqlx::test]
async fn process_batch_gracefully_handles_bad_ai_response(pool: PgPool) {
    common::migrate(&pool).await;
    let a = insert_item(&pool, "Padaria", Some("Pão Quente"), &[]).await;

    // Response without a valid "suggestions" array → batch completes, no tags.
    let server = mount_tag_mock("suggestions", json!([{ "index": "0", "tags": "padaria" }])).await;
    let ai = common::ai_client(pool.clone(), server.uri());

    let batch = ai_tags::enqueue_batch(&pool, vec![a], "tags").await.unwrap();
    ai_tags::process_batch(&pool, &ai, batch.id).await.unwrap();

    assert_eq!(suggestion_tags(&pool, batch.id, a).await, Vec::<String>::new());
}

#[sqlx::test]
async fn apply_adds_tags_and_marks_applied(pool: PgPool) {
    common::migrate(&pool).await;
    let a = insert_item(&pool, "Padaria", Some("Pão Quente"), &["pao"]).await;

    let batch = ai_tags::enqueue_batch(&pool, vec![a], "tags").await.unwrap();
    sqlx::query(
        "UPDATE ai_tag_suggestions SET suggested_tags = $1 WHERE batch_id = $2 AND item_id = $3",
    )
    .bind(vec!["cafe".to_string(), "PÃO".to_string()])
    .bind(batch.id)
    .bind(a)
    .execute(&pool)
    .await
    .unwrap();

    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM ai_tag_suggestions WHERE batch_id = $1 AND item_id = $2",
    )
    .bind(batch.id)
    .bind(a)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Apply with an edited tag list (user removed "PÃO" and added "fim de semana").
    let res = ai_tags::apply_suggestion(&pool, id, Some(vec!["cafe".into(), "Fim de Semana".into()]), None)
        .await
        .unwrap();
    assert_eq!(res["tags"].as_array().unwrap().len(), 2);
    assert_eq!(suggestion_status(&pool, batch.id, a).await, "applied");

    let tags: Vec<String> = sqlx::query_scalar("SELECT tags FROM items WHERE id = $1")
        .bind(a)
        .fetch_one(&pool)
        .await
        .unwrap();
    // Existing "pao" kept; added tags deduped + normalized.
    assert_eq!(tags, vec!["pao", "cafe", "fim de semana"]);

    // Applying again → conflict.
    match ai_tags::apply_suggestion(&pool, id, None, None).await {
        Err(AppError::Conflict(m)) => assert_eq!(m, "suggestion already reviewed"),
        other => panic!("expected Conflict, got {other:?}"),
    }
}

#[sqlx::test]
async fn dismiss_marks_dismissed(pool: PgPool) {
    common::migrate(&pool).await;
    let a = insert_item(&pool, "Padaria", Some("Pão Quente"), &[]).await;

    let batch = ai_tags::enqueue_batch(&pool, vec![a], "tags").await.unwrap();
    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM ai_tag_suggestions WHERE batch_id = $1 AND item_id = $2",
    )
    .bind(batch.id)
    .bind(a)
    .fetch_one(&pool)
    .await
    .unwrap();

    ai_tags::dismiss_suggestion(&pool, id).await.unwrap();
    assert_eq!(suggestion_status(&pool, batch.id, a).await, "dismissed");

    match ai_tags::dismiss_suggestion(&pool, id).await {
        Err(AppError::Conflict(_)) => {}
        other => panic!("expected Conflict, got {other:?}"),
    }
    match ai_tags::dismiss_suggestion(&pool, Uuid::new_v4()).await {
        Err(AppError::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[sqlx::test]
async fn apply_all_and_dismiss_all_scope_to_batch(pool: PgPool) {
    common::migrate(&pool).await;
    let a = insert_item(&pool, "Padaria", Some("Pão Quente"), &[]).await;
    let b = insert_item(&pool, "Farmácia", Some("Droga Raia"), &[]).await;
    let c = insert_item(&pool, "Café", Some("Starbucks"), &[]).await;

    let batch1 = ai_tags::enqueue_batch(&pool, vec![a, b], "tags").await.unwrap();
    let batch2 = ai_tags::enqueue_batch(&pool, vec![c], "tags").await.unwrap();
    for (batch, item, tags) in [
        (batch1.id, a, vec!["pao".to_string()]),
        (batch1.id, b, vec!["saude".to_string()]),
        (batch2.id, c, vec!["cafe".to_string()]),
    ] {
        sqlx::query(
            "UPDATE ai_tag_suggestions SET suggested_tags = $1 WHERE batch_id = $2 AND item_id = $3",
        )
        .bind(tags)
        .bind(batch)
        .bind(item)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Apply only batch1.
    let res = ai_tags::apply_all(&pool, Some(batch1.id)).await.unwrap();
    assert_eq!(res["applied"].as_i64().unwrap(), 2);
    assert_eq!(suggestion_status(&pool, batch1.id, a).await, "applied");
    assert_eq!(suggestion_status(&pool, batch1.id, b).await, "applied");
    // batch2 untouched.
    assert_eq!(suggestion_status(&pool, batch2.id, c).await, "pending");

    let tags_a: Vec<String> = sqlx::query_scalar("SELECT tags FROM items WHERE id = $1")
        .bind(a)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tags_a, vec!["pao"]);

    // Dismiss the rest (no batch scope → all pending).
    let res = ai_tags::dismiss_all(&pool, None).await.unwrap();
    assert_eq!(res["dismissed"].as_i64().unwrap(), 1);
    assert_eq!(suggestion_status(&pool, batch2.id, c).await, "dismissed");
}

#[sqlx::test]
async fn suggestions_list_joins_item_details(pool: PgPool) {
    common::migrate(&pool).await;
    let cat: Uuid =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('Saúde (teste)') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let a = insert_item(&pool, "Farmácia", Some("Droga Raia"), &[]).await;
    sqlx::query("UPDATE items SET category_id = $1 WHERE id = $2")
        .bind(cat)
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();

    let batch = ai_tags::enqueue_batch(&pool, vec![a], "tags").await.unwrap();
    sqlx::query(
        "UPDATE ai_tag_suggestions SET suggested_tags = $1 WHERE batch_id = $2",
    )
    .bind(vec!["saude".to_string()])
    .bind(batch.id)
    .execute(&pool)
    .await
    .unwrap();

    let list = deepsave_backend::routes::ai_tags::list_suggestions_query(&pool, Some(batch.id), "pending")
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    let s = &list[0];
    assert_eq!(s.batch_id, batch.id);
    assert_eq!(s.batch_status, "pending");
    assert_eq!(s.item_id, a);
    assert_eq!(s.suggested_tags, vec!["saude"]);
    assert_eq!(s.description, "Farmácia");
    assert_eq!(s.category_name.as_deref(), Some("Saúde (teste)"));
    assert_eq!(s.amount_cents, -1000);
}

#[sqlx::test]
async fn categorize_batch_proposes_and_applies_categories(pool: PgPool) {
    common::migrate(&pool).await;
    let a = insert_item(&pool, "Padaria", Some("Pão Quente"), &[]).await;
    let b = insert_item(&pool, "Netflix", Some("NETFLIX"), &[]).await;

    let server = mount_tag_mock(
        "categories",
        json!([
            { "index": 0, "category": "Restaurantes" },
            { "index": 1, "category": "nova: Streaming" }
        ]),
    )
    .await;
    let ai = common::ai_client(pool.clone(), server.uri());

    let batch = ai_tags::enqueue_batch(&pool, vec![a, b], "categorize").await.unwrap();
    assert_eq!(batch.kind, "categorize");
    ai_tags::process_categorize_batch(&pool, &ai, batch.id).await.unwrap();

    let cat_a: String = sqlx::query_scalar(
        "SELECT suggested_category FROM ai_tag_suggestions WHERE batch_id = $1 AND item_id = $2",
    )
    .bind(batch.id)
    .bind(a)
    .fetch_one(&pool)
    .await
    .unwrap();
    let cat_b: String = sqlx::query_scalar(
        "SELECT suggested_category FROM ai_tag_suggestions WHERE batch_id = $1 AND item_id = $2",
    )
    .bind(batch.id)
    .bind(b)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cat_a, "Restaurantes");
    assert_eq!(cat_b, "nova: Streaming");

    // Apply both: existing category matched, "nova:" creates it.
    let sug_a: Uuid = sqlx::query_scalar(
        "SELECT id FROM ai_tag_suggestions WHERE batch_id = $1 AND item_id = $2",
    )
    .bind(batch.id)
    .bind(a)
    .fetch_one(&pool)
    .await
    .unwrap();
    let sug_b: Uuid = sqlx::query_scalar(
        "SELECT id FROM ai_tag_suggestions WHERE batch_id = $1 AND item_id = $2",
    )
    .bind(batch.id)
    .bind(b)
    .fetch_one(&pool)
    .await
    .unwrap();
    ai_tags::apply_suggestion(&pool, sug_a, None, None).await.unwrap();
    ai_tags::apply_suggestion(&pool, sug_b, None, None).await.unwrap();

    let (cat_name_a, cat_name_b): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT c1.name, c2.name
         FROM items ia LEFT JOIN categories c1 ON c1.id = ia.category_id,
              items ib LEFT JOIN categories c2 ON c2.id = ib.category_id
         WHERE ia.id = $1 AND ib.id = $2",
    )
    .bind(a)
    .bind(b)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cat_name_a.as_deref(), Some("Restaurantes"));
    assert_eq!(cat_name_b.as_deref(), Some("Streaming"));
}
