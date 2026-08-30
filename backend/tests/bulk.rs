//! Integration tests for bulk item edits (`PATCH /items/bulk`).
//!
//! The core logic is exercised directly via `routes::items::bulk_update_items`
//! (no HTTP harness / AppState needed, matching the rest of the test suite).

use deepsave_backend::error::AppError;
use deepsave_backend::models::{BulkItemUpdate, TagsMode};
use deepsave_backend::routes::items::bulk_update_items;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

mod common;

async fn insert_item(
    pool: &PgPool,
    description: &str,
    kind: &str,
    category_id: Option<Uuid>,
    tags: &[&str],
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO items (kind, status, source, occurred_on, description, amount_cents, category_id, tags)
         VALUES ($1, 'confirmed', 'manual', '2026-07-01', $2, -1000, $3, $4)
         RETURNING id",
    )
    .bind(kind)
    .bind(description)
    .bind(category_id)
    .bind(tags.iter().map(|t| t.to_string()).collect::<Vec<_>>())
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn insert_category(pool: &PgPool, name: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO categories (name) VALUES ($1) RETURNING id")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn item_row(pool: &PgPool, id: Uuid) -> (String, Option<Uuid>, Vec<String>) {
    sqlx::query_as("SELECT kind, category_id, tags FROM items WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn updated_count(res: Result<Value, AppError>) -> i64 {
    res.unwrap()["updated"].as_i64().unwrap()
}

#[sqlx::test]
async fn kind_only_update_keeps_other_fields(pool: PgPool) {
    common::migrate(&pool).await;
    let cat = insert_category(&pool, "Mercado").await;
    let id = insert_item(&pool, "Padaria", "expense", Some(cat), &["pao"]).await;

    let res = bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id],
            kind: Some("income".into()),
            category_id: None,
            tags: None,
            tags_mode: None,
        },
    )
    .await;
    assert_eq!(updated_count(res).await, 1);

    let (kind, category_id, tags) = item_row(&pool, id).await;
    assert_eq!(kind, "income");
    assert_eq!(category_id, Some(cat));
    assert_eq!(tags, vec!["pao"]);
}

#[sqlx::test]
async fn category_set_and_clear(pool: PgPool) {
    common::migrate(&pool).await;
    let cat = insert_category(&pool, "Mercado").await;
    let id = insert_item(&pool, "Padaria", "expense", None, &[]).await;

    // Set.
    let res = bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id],
            kind: None,
            category_id: Some(Some(cat)),
            tags: None,
            tags_mode: None,
        },
    )
    .await;
    assert_eq!(updated_count(res).await, 1);
    assert_eq!(item_row(&pool, id).await.1, Some(cat));

    // Clear (`Some(None)`).
    let res = bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id],
            kind: None,
            category_id: Some(None),
            tags: None,
            tags_mode: None,
        },
    )
    .await;
    assert_eq!(updated_count(res).await, 1);
    assert_eq!(item_row(&pool, id).await.1, None);
}

#[sqlx::test]
async fn category_omitted_keeps_value(pool: PgPool) {
    common::migrate(&pool).await;
    let cat = insert_category(&pool, "Mercado").await;
    let id = insert_item(&pool, "Padaria", "expense", Some(cat), &[]).await;

    bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id],
            kind: Some("refund".into()),
            category_id: None,
            tags: None,
            tags_mode: None,
        },
    )
    .await
    .unwrap();

    let (kind, category_id, _) = item_row(&pool, id).await;
    assert_eq!(kind, "refund");
    assert_eq!(category_id, Some(cat));
}

#[sqlx::test]
async fn tags_replace_add_remove(pool: PgPool) {
    common::migrate(&pool).await;
    let id = insert_item(&pool, "Padaria", "expense", None, &["pao", "cafe"]).await;

    // Add: normalizes accents/case and dedupes against existing tags.
    let res = bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id],
            kind: None,
            category_id: None,
            tags: Some(vec![" Mercado ".into(), "CAFÉ".into()]),
            tags_mode: Some(TagsMode::Add),
        },
    )
    .await;
    assert_eq!(updated_count(res).await, 1);
    assert_eq!(item_row(&pool, id).await.2, vec!["pao", "cafe", "mercado"]);

    // Replace wholesale.
    bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id],
            kind: None,
            category_id: None,
            tags: Some(vec!["casa".into()]),
            tags_mode: Some(TagsMode::Replace),
        },
    )
    .await
    .unwrap();
    assert_eq!(item_row(&pool, id).await.2, vec!["casa"]);

    // Remove one of two.
    let id2 = insert_item(&pool, "Farmácia", "expense", None, &["saude", "urgente"]).await;
    bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id, id2],
            kind: None,
            category_id: None,
            tags: Some(vec!["saude".into()]),
            tags_mode: Some(TagsMode::Remove),
        },
    )
    .await
    .unwrap();
    assert_eq!(item_row(&pool, id).await.2, vec!["casa"]);
    assert_eq!(item_row(&pool, id2).await.2, vec!["urgente"]);
}

#[sqlx::test]
async fn unknown_ids_are_ignored(pool: PgPool) {
    common::migrate(&pool).await;
    let id = insert_item(&pool, "Padaria", "expense", None, &[]).await;

    let res = bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id, Uuid::new_v4()],
            kind: Some("income".into()),
            category_id: None,
            tags: None,
            tags_mode: None,
        },
    )
    .await;
    assert_eq!(updated_count(res).await, 1);
}

#[sqlx::test]
async fn validation_errors(pool: PgPool) {
    common::migrate(&pool).await;

    let empty = bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![],
            kind: None,
            category_id: None,
            tags: None,
            tags_mode: None,
        },
    )
    .await;
    match empty {
        Err(AppError::BadRequest(m)) => assert_eq!(m, "ids must not be empty"),
        other => panic!("expected BadRequest, got {other:?}"),
    }

    let bad_kind = bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![Uuid::new_v4()],
            kind: Some("nonsense".into()),
            category_id: None,
            tags: None,
            tags_mode: None,
        },
    )
    .await;
    match bad_kind {
        Err(AppError::BadRequest(m)) => assert_eq!(m, "invalid kind"),
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

#[sqlx::test]
async fn duplicate_ids_dedupe(pool: PgPool) {
    common::migrate(&pool).await;
    let id = insert_item(&pool, "Padaria", "expense", None, &[]).await;

    let res = bulk_update_items(
        &pool,
        BulkItemUpdate {
            ids: vec![id, id],
            kind: Some("income".into()),
            category_id: None,
            tags: None,
            tags_mode: None,
        },
    )
    .await;
    assert_eq!(updated_count(res).await, 1);
}


