//! Tests for tag management: usage counts, rename cascade (with dedupe/merge) and delete.

use deepsave_backend::services::tags;
use sqlx::PgPool;

mod common;

async fn seed_item(pool: &PgPool, description: &str, item_tags: &[&str]) {
    let tags: Vec<String> = item_tags.iter().map(|s| s.to_string()).collect();
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents, tags)
         VALUES ('manual', 'expense', 'confirmed', '2025-01-01', $1, -100, $2)",
    )
    .bind(description)
    .bind(&tags)
    .execute(pool)
    .await
    .unwrap();
}

async fn item_tags(pool: &PgPool, description: &str) -> Vec<String> {
    sqlx::query_scalar::<_, Vec<String>>("SELECT tags FROM items WHERE description = $1")
        .bind(description)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn usage_counts_per_tag(pool: PgPool) {
    common::migrate(&pool).await;
    seed_item(&pool, "a", &["compras", "mercado"]).await;
    seed_item(&pool, "b", &["compras"]).await;
    seed_item(&pool, "c", &["mercado", "lazer"]).await;

    let usage = tags::usage(&pool).await.unwrap();
    let by_tag: std::collections::HashMap<_, _> =
        usage.iter().map(|u| (u.tag.as_str(), u.count)).collect();

    assert_eq!(by_tag.get("compras"), Some(&2));
    assert_eq!(by_tag.get("mercado"), Some(&2));
    assert_eq!(by_tag.get("lazer"), Some(&1));
    assert_eq!(usage.len(), 3);
}

#[sqlx::test]
async fn rename_cascades_and_dedupes(pool: PgPool) {
    common::migrate(&pool).await;
    seed_item(&pool, "a", &["compras", "mercado"]).await; // compras → mercado must dedupe
    seed_item(&pool, "b", &["compras", "lazer"]).await;

    // Recurring rules are NOT touched anymore: their tags are derived from linked
    // items, so a rename on items automatically reflects there.
    let res = tags::rename(&pool, "compras", "mercado").await.unwrap();
    assert_eq!(res.items_updated, 2);

    assert_eq!(item_tags(&pool, "a").await, vec!["mercado".to_string()]);
    assert_eq!(
        item_tags(&pool, "b").await,
        vec!["mercado".to_string(), "lazer".to_string()]
    );
}

#[sqlx::test]
async fn rename_unknown_tag_is_noop(pool: PgPool) {
    common::migrate(&pool).await;
    seed_item(&pool, "a", &["mercado"]).await;

    let res = tags::rename(&pool, "inexistente", "nova").await.unwrap();
    assert_eq!(res.items_updated, 0);
    assert_eq!(item_tags(&pool, "a").await, vec!["mercado".to_string()]);
}

#[sqlx::test]
async fn remove_tag_drops_it_everywhere(pool: PgPool) {
    common::migrate(&pool).await;
    seed_item(&pool, "a", &["compras", "mercado"]).await;
    seed_item(&pool, "b", &["compras"]).await;

    let res = tags::remove(&pool, "compras").await.unwrap();
    assert_eq!(res.items_updated, 2);

    assert_eq!(item_tags(&pool, "a").await, vec!["mercado".to_string()]);
    assert_eq!(item_tags(&pool, "b").await, Vec::<String>::new());
}

#[sqlx::test]
async fn normalize_one_handles_accents_and_case(_pool: PgPool) {
    let n = tags::normalize_one("  Compras Extra  ").unwrap();
    assert_eq!(n, "compras extra");
    assert!(tags::normalize_one("  ").is_none());
    assert!(tags::normalize_one("").is_none());
}
