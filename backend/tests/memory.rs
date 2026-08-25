//! Tests for merchant memory: tags accumulate on confirmation, and the
//! preview-before-apply flow only touches the items the user selects.

use deepsave_backend::services::memory;
use sqlx::PgPool;
use uuid::Uuid;

mod common;

async fn seed_item(
    pool: &PgPool,
    merchant: &str,
    description: &str,
    category_id: Option<Uuid>,
    item_tags: &[&str],
) -> Uuid {
    let tags: Vec<String> = item_tags.iter().map(|s| s.to_string()).collect();
    sqlx::query_scalar(
        "INSERT INTO items (source, kind, status, occurred_on, merchant, description,
                            amount_cents, category_id, tags)
         VALUES ('manual', 'expense', 'confirmed', '2025-01-01', $1, $2, -100, $3, $4)
         RETURNING id",
    )
    .bind(merchant)
    .bind(description)
    .bind(category_id)
    .bind(&tags)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_category(pool: &PgPool, name: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO categories (name) VALUES ($1) RETURNING id")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn memory_tags(pool: &PgPool, merchant: &str) -> Vec<String> {
    sqlx::query_scalar::<_, Vec<String>>("SELECT tags FROM merchant_memory WHERE merchant = $1")
        .bind(merchant)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn item_row(pool: &PgPool, id: Uuid) -> (Option<Uuid>, Vec<String>) {
    sqlx::query_as("SELECT category_id, tags FROM items WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

// ---------------------------------------------------------------------------
// record_confirmation
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn confirmation_accumulates_tags_and_keeps_category(pool: PgPool) {
    common::migrate(&pool).await;
    let cat = seed_category(&pool, "Mercado").await;

    memory::record_confirmation(&pool, "Pão Quente", Some(cat), &["mercado".into()])
        .await
        .unwrap();
    // Merchant is normalized (accents stripped, lowercased).
    assert_eq!(memory_tags(&pool, "pao quente").await, vec!["mercado"]);

    // Second confirmation with different tags → union, not replace.
    memory::record_confirmation(&pool, "Pão Quente", None, &["lanche".into()])
        .await
        .unwrap();
    assert_eq!(memory_tags(&pool, "pao quente").await, vec!["mercado", "lanche"]);

    // Category is NOT cleared by a confirmation without one.
    let (category_id, _): (Option<Uuid>, Vec<String>) =
        sqlx::query_as("SELECT category_id, tags FROM merchant_memory WHERE merchant = 'pao quente'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(category_id, Some(cat));

    // Re-confirming the same tags keeps them unique.
    memory::record_confirmation(&pool, "Pão Quente", Some(cat), &["mercado".into()])
        .await
        .unwrap();
    assert_eq!(memory_tags(&pool, "pao quente").await, vec!["mercado", "lanche"]);
}

#[sqlx::test]
async fn empty_or_blank_merchant_is_ignored(pool: PgPool) {
    common::migrate(&pool).await;
    memory::record_confirmation(&pool, "   ", None, &["x".into()]).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM merchant_memory")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// ---------------------------------------------------------------------------
// preview_candidates + apply_selected
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn preview_lists_only_items_that_would_change(pool: PgPool) {
    common::migrate(&pool).await;
    let cat = seed_category(&pool, "Mercado").await;
    let other = seed_category(&pool, "Outra Cat").await;
    let id_other = other;

    // Memory: "mercado x" → Mercado + tags [compras, mercado].
    sqlx::query(
        "INSERT INTO merchant_memory (merchant, category_id, tags, confirm_count)
         VALUES ('mercado x', $1, $2, 3)",
    )
    .bind(cat)
    .bind(&vec!["compras".to_string(), "mercado".to_string()])
    .execute(&pool)
    .await
    .unwrap();

    // Uncategorized, no tags → both changes.
    let a = seed_item(&pool, "Mercado X", "item a", None, &[]).await;
    // Has category, missing tags → only tags.
    let b = seed_item(&pool, "Mercado X", "item b", Some(cat), &[]).await;
    // Has both → not in preview.
    let c = seed_item(&pool, "Mercado X", "item c", Some(cat), &["compras", "mercado"]).await;
    // Different merchant → not in preview.
    let d = seed_item(&pool, "Outra Loja", "item d", None, &[]).await;
    // Different category → category change proposed.
    let e = seed_item(&pool, "Mercado X", "item e", Some(id_other), &[]).await;

    let preview = memory::preview_candidates(&pool, None).await.unwrap();
    let ids: Vec<Uuid> = preview.iter().map(|p| p.item_id).collect();
    assert!(ids.contains(&a));
    assert!(ids.contains(&b));
    assert!(!ids.contains(&c));
    assert!(!ids.contains(&d));
    assert!(ids.contains(&e));

    let pa = preview.iter().find(|p| p.item_id == a).unwrap();
    assert_eq!(pa.changes, vec!["category", "tags"]);
    assert_eq!(pa.proposed_category.as_deref(), Some("Mercado"));
    assert_eq!(pa.tags_to_add, vec!["compras", "mercado"]);

    let pb = preview.iter().find(|p| p.item_id == b).unwrap();
    assert_eq!(pb.changes, vec!["tags"]);
    assert_eq!(pb.current_category.as_deref(), Some("Mercado"));
    assert_eq!(pb.proposed_category, None);

    let pe = preview.iter().find(|p| p.item_id == e).unwrap();
    assert_eq!(pe.changes, vec!["category", "tags"]);
    assert_eq!(pe.proposed_category.as_deref(), Some("Mercado"));

    // Merchant-scoped preview.
    let scoped = memory::preview_candidates(&pool, Some("mercado x")).await.unwrap();
    assert_eq!(scoped.len(), preview.len());
    let none = memory::preview_candidates(&pool, Some("outra loja")).await.unwrap();
    assert!(none.is_empty());
}

#[sqlx::test]
async fn apply_only_touches_selected_ids(pool: PgPool) {
    common::migrate(&pool).await;
    let cat = seed_category(&pool, "Mercado").await;
    sqlx::query(
        "INSERT INTO merchant_memory (merchant, category_id, tags, confirm_count)
         VALUES ('mercado x', $1, $2, 3)",
    )
    .bind(cat)
    .bind(&vec!["compras".to_string()])
    .execute(&pool)
    .await
    .unwrap();

    let a = seed_item(&pool, "Mercado X", "item a", None, &["viagem"]).await;
    let b = seed_item(&pool, "Mercado X", "item b", None, &[]).await;

    let updated = memory::apply_selected(&pool, None, &[a]).await.unwrap();
    assert_eq!(updated, 1);

    // `a` got the category and the tag (kept its own situational tag too).
    let (cat_a, tags_a) = item_row(&pool, a).await;
    assert_eq!(cat_a, Some(cat));
    assert_eq!(tags_a, vec!["viagem", "compras"]);

    // `b` untouched.
    let (cat_b, tags_b) = item_row(&pool, b).await;
    assert_eq!(cat_b, None);
    assert!(tags_b.is_empty());

    // Re-applying is idempotent but still reports the row.
    let again = memory::apply_selected(&pool, None, &[a]).await.unwrap();
    assert_eq!(again, 1);

    // Unknown ids are ignored.
    let bogus = memory::apply_selected(&pool, None, &[Uuid::new_v4()]).await.unwrap();
    assert_eq!(bogus, 0);
}
