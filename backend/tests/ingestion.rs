//! Integration tests for the full ingestion flow, with a mocked DeepSeek API.

use deepsave_backend::services::ingest;
use serde_json::json;
use sqlx::PgPool;

mod common;

async fn count_items(pool: &PgPool, doc_id: uuid::Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM items WHERE document_id = $1")
        .bind(doc_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn doc_status(pool: &PgPool, doc_id: uuid::Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(doc_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn ai_calls_count(pool: &PgPool, doc_id: uuid::Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ai_calls WHERE document_id = $1")
        .bind(doc_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn csv_ingestion_creates_confirmed_items(pool: PgPool) {
    common::migrate(&pool).await;
    let ai = common::ai_client(pool.clone(), "http://127.0.0.1:1".into());
    let doc = common::insert_document(
        &pool,
        "bank_statement",
        "c6_bank.csv",
        "text/csv",
        &common::fixture("c6_bank.csv"),
    )
    .await;

    ingest::process_document(&pool, &doc, &ai).await.unwrap();

    assert_eq!(count_items(&pool, doc.id).await, 5);
    assert_eq!(doc_status(&pool, doc.id).await, "processed");

    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM items WHERE document_id = $1",
    )
    .bind(doc.id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(kinds.iter().any(|k| k == "internal"));
    assert!(kinds.iter().any(|k| k == "expense"));
    // Structured CSV never calls the AI.
    assert_eq!(ai_calls_count(&pool, doc.id).await, 0);
}

#[sqlx::test]
async fn pdf_text_ingestion_uses_mocked_ai(pool: PgPool) {
    common::migrate(&pool).await;
    let server = common::mount_deepseek_mock(json!([
        {"description": "Fake Grocery", "amount_cents": -5000, "category": "Supermercado",
         "tags": ["mercado"], "kind": "expense", "date": "2026-07-15",
         "installment": null, "installment_count": null}
    ]))
    .await;
    let ai = common::ai_client(pool.clone(), server.uri());
    let doc = common::insert_document(
        &pool,
        "bank_statement",
        "caixa_bank.pdf",
        "application/pdf",
        &common::fixture("caixa_bank.pdf"),
    )
    .await;

    ingest::process_document(&pool, &doc, &ai).await.unwrap();

    assert_eq!(count_items(&pool, doc.id).await, 1);
    assert_eq!(doc_status(&pool, doc.id).await, "needs_review");

    let (status, kind, amount): (String, String, i64) = sqlx::query_as(
        "SELECT status, kind, amount_cents FROM items WHERE document_id = $1",
    )
    .bind(doc.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "pending_review");
    assert_eq!(kind, "expense");
    assert_eq!(amount, -5000);

    // One DeepSeek call recorded.
    assert_eq!(ai_calls_count(&pool, doc.id).await, 1);
}

#[sqlx::test]
async fn caixa_card_pdf_parses_structurally(pool: PgPool) {
    common::migrate(&pool).await;
    let ai = common::ai_client(pool.clone(), "http://127.0.0.1:1".into());
    let doc = common::insert_document(
        &pool,
        "card_statement",
        "caixa_card.pdf",
        "application/pdf",
        &common::fixture("caixa_card.pdf"),
    )
    .await;

    ingest::process_document(&pool, &doc, &ai).await.unwrap();

    assert_eq!(count_items(&pool, doc.id).await, 1);
    assert_eq!(doc_status(&pool, doc.id).await, "processed");

    let (description, kind, amount): (String, String, i64) = sqlx::query_as(
        "SELECT description, kind, amount_cents FROM items WHERE document_id = $1",
    )
    .bind(doc.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kind, "expense");
    assert_eq!(amount, -1290);
    assert!(description.to_lowercase().contains("ifood"));
    // Structural parse: no AI calls.
    assert_eq!(ai_calls_count(&pool, doc.id).await, 0);
}

#[sqlx::test]
async fn image_ingestion_uses_mocked_vision(pool: PgPool) {
    common::migrate(&pool).await;
    let server = common::mount_deepseek_mock(json!([
        {"description": "Fake Receipt Item", "amount_cents": -990, "category": null,
         "tags": [], "kind": "expense", "date": null,
         "installment": null, "installment_count": null}
    ]))
    .await;
    let ai = common::ai_client(pool.clone(), server.uri());
    let doc = common::insert_document(
        &pool,
        "receipt",
        "receipt.jpg",
        "image/jpeg",
        &common::fixture("receipt.jpg"),
    )
    .await;

    ingest::process_document(&pool, &doc, &ai).await.unwrap();

    assert_eq!(count_items(&pool, doc.id).await, 1);
    assert_eq!(doc_status(&pool, doc.id).await, "needs_review");

    let (model, status): (String, String) = sqlx::query_as(
        "SELECT model, status FROM ai_calls WHERE document_id = $1",
    )
    .bind(doc.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(model, "deepseek-v4-flash-vision-exp");
    assert_eq!(status, "ok");
}

#[sqlx::test]
async fn reclassify_marks_equal_amount_transfer_pairs_internal(pool: PgPool) {
    common::migrate(&pool).await;

    // Own-account transfer pair: out (Nubank) + in (C6), same amount, same day.
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('bank_statement','expense','confirmed','2026-04-09',
                 'Transferência enviada pelo Pix - PAULO HENRIQUE CUCHI - BCO C6 S.A. Conta: 24894407-0', -10000)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('bank_statement','income','confirmed','2026-04-09',
                 'Pix recebido de Paulo Henrique Cuchi', 10000)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Same amount but a genuine third-party payment: must NOT be paired.
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('bank_statement','expense','confirmed','2026-04-09',
                 'Transferência enviada pelo Pix - IFOOD.COM', -10000)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Same amount + same day but DIFFERENT counterparty: not a pair.
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('bank_statement','expense','confirmed','2026-04-09',
                 'Transferência enviada pelo Pix - PEDRO DE LIMA JUNIOR', -10000)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // A reembolso (refund) is not an internal transfer either.
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('bank_statement','income','confirmed','2026-02-05',
                 'Reembolso recebido pelo Pix - CLINICA MOTIVATION MEDICINA', 16000)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('bank_statement','expense','confirmed','2026-02-05',
                 'Transferência enviada pelo Pix - CLINICA MOTIVATION NUTRICAO', -16000)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let updated = ingest::reclassify_pix_as_internal(&pool).await.unwrap();
    assert_eq!(updated, 2, "only the own-account pair changes");

    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM items")
        .fetch_all(&pool)
        .await
        .unwrap();
    let mut counts = [0usize; 3];
    for k in &kinds {
        match k.as_str() {
            "internal" => counts[0] += 1,
            "expense" => counts[1] += 1,
            "income" => counts[2] += 1,
            _ => panic!("unexpected kind {k}"),
        }
    }
    assert_eq!(counts, [2, 3, 1], "only the own-account pair becomes internal");

    // Both sides of the pair share a transfer group.
    let groups: Vec<Option<uuid::Uuid>> = sqlx::query_scalar(
        "SELECT transfer_group_id FROM items WHERE kind = 'internal' ORDER BY description",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0], groups[1]);
    assert!(groups[0].is_some());
}
