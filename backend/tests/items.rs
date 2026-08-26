//! Tests for the items list filters (`exclude_installments`) and the filtered
//! summary (`/api/items/summary`), driven directly via the pool-level functions.

use deepsave_backend::routes::items::{list_items, summary, ListQuery};
use sqlx::PgPool;

mod common;

fn base_query() -> ListQuery {
    ListQuery {
        month: None,
        status: None,
        search: None,
        category_ids: None,
        kind: None,
        tags: None,
        bank: None,
        sort: None,
        limit: None,
        installments: None,
        date_from: None,
        date_to: None,
    }
}

async fn insert_item_on(
    pool: &PgPool,
    description: &str,
    amount_cents: i64,
    occurred_on: &str,
    installment: Option<i32>,
    installment_count: Option<i32>,
) {
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents,
                            installment, installment_count)
         VALUES ('manual', 'expense', 'confirmed', $1::date, $2, $3, $4, $5)",
    )
    .bind(occurred_on)
    .bind(description)
    .bind(amount_cents)
    .bind(installment)
    .bind(installment_count)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_item(
    pool: &PgPool,
    description: &str,
    amount_cents: i64,
    installment: Option<i32>,
    installment_count: Option<i32>,
) {
    insert_item_on(pool, description, amount_cents, "2026-07-01", installment, installment_count).await;
}

#[sqlx::test]
async fn exclude_installments_keeps_only_first_parcel(pool: PgPool) {
    common::migrate(&pool).await;
    insert_item(&pool, "sem parcela", -1000, None, None).await;
    insert_item(&pool, "parcela 1/3", -1000, Some(1), Some(3)).await;
    insert_item(&pool, "parcela 2/3", -1000, Some(2), Some(3)).await;
    insert_item(&pool, "parcela 3/3", -1000, Some(3), Some(3)).await;
    insert_item(&pool, "unica 1/1", -1000, Some(1), Some(1)).await;

    // Include (default) → everything.
    let all = list_items(&pool, &base_query()).await.unwrap();
    let got: Vec<String> = all.iter().map(|i| i.description.clone()).collect();
    assert!(got.contains(&"parcela 2/3".to_string()));
    assert_eq!(all.len(), 5);

    // Exclude (first_only) → only non-installments + the first parcel.
    let mut q = base_query();
    q.installments = Some("first_only".to_string());
    let filtered = list_items(&pool, &q).await.unwrap();
    let got: Vec<String> = filtered.iter().map(|i| i.description.clone()).collect();
    assert_eq!(got.len(), 3);
    assert!(got.contains(&"sem parcela".to_string()));
    assert!(got.contains(&"parcela 1/3".to_string()));
    assert!(got.contains(&"unica 1/1".to_string()));
    assert!(!got.contains(&"parcela 2/3".to_string()));
    assert!(!got.contains(&"parcela 3/3".to_string()));

    // Only installments → all parcels of parceled purchases, nothing else.
    let mut q = base_query();
    q.installments = Some("only".to_string());
    let only = list_items(&pool, &q).await.unwrap();
    let got: Vec<String> = only.iter().map(|i| i.description.clone()).collect();
    assert_eq!(got.len(), 3);
    assert!(got.contains(&"parcela 1/3".to_string()));
    assert!(got.contains(&"parcela 2/3".to_string()));
    assert!(got.contains(&"parcela 3/3".to_string()));
    assert!(!got.contains(&"sem parcela".to_string()));
    assert!(!got.contains(&"unica 1/1".to_string()));
}

#[sqlx::test]
async fn summary_sums_roots_only_and_respects_filters(pool: PgPool) {
    common::migrate(&pool).await;
    let root: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('manual', 'expense', 'confirmed', '2026-07-01', 'Compra mercado', -5000)
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    // A receipt child (linked → parent_id set): must NOT double-count.
    insert_item(&pool, "Linha do recibo", -3000, None, None).await;
    sqlx::query("UPDATE items SET parent_id = $1 WHERE description = 'Linha do recibo'")
        .bind(root)
        .execute(&pool)
        .await
        .unwrap();
    insert_item(&pool, "Receita pix", 1000, None, None).await;

    let s = summary(&pool, &base_query()).await.unwrap();
    assert_eq!(s.count, 2); // root + income (child excluded)
    assert_eq!(s.total_cents, -4000);

    // Search narrows it down.
    let mut q = base_query();
    q.search = Some("mercado".to_string());
    let s = summary(&pool, &q).await.unwrap();
    assert_eq!(s.count, 1);
    assert_eq!(s.total_cents, -5000);
}

#[sqlx::test]
async fn summary_respects_exclude_installments(pool: PgPool) {
    common::migrate(&pool).await;
    insert_item(&pool, "p1/3", -100, Some(1), Some(3)).await;
    insert_item(&pool, "p2/3", -100, Some(2), Some(3)).await;
    insert_item(&pool, "p3/3", -100, Some(3), Some(3)).await;

    let s = summary(&pool, &base_query()).await.unwrap();
    assert_eq!(s.count, 3);
    assert_eq!(s.total_cents, -300);

    let mut q = base_query();
    q.installments = Some("first_only".to_string());
    let s = summary(&pool, &q).await.unwrap();
    // First parcel stands in for the whole purchase: full price = parcel × count.
    assert_eq!(s.count, 1);
    assert_eq!(s.total_cents, -300);

    // 'only' → the whole series.
    let mut q = base_query();
    q.installments = Some("only".to_string());
    let s = summary(&pool, &q).await.unwrap();
    assert_eq!(s.count, 3);
    assert_eq!(s.total_cents, -300);
}

#[sqlx::test]
async fn date_range_filters_list_and_summary(pool: PgPool) {
    common::migrate(&pool).await;
    insert_item_on(&pool, "julho", -100, "2026-07-15", None, None).await;
    insert_item_on(&pool, "agosto", -200, "2026-08-15", None, None).await;
    insert_item_on(&pool, "setembro", -300, "2026-09-30", None, None).await;

    // Both bounds (inclusive end).
    let mut q = base_query();
    q.date_from = Some("2026-08-01".parse().unwrap());
    q.date_to = Some("2026-08-31".parse().unwrap());
    let items = list_items(&pool, &q).await.unwrap();
    let got: Vec<String> = items.iter().map(|i| i.description.clone()).collect();
    assert_eq!(got, vec!["agosto"]);
    let s = summary(&pool, &q).await.unwrap();
    assert_eq!(s.count, 1);
    assert_eq!(s.total_cents, -200);

    // Open-ended: only from.
    let mut q = base_query();
    q.date_from = Some("2026-09-01".parse().unwrap());
    let items = list_items(&pool, &q).await.unwrap();
    let got: Vec<String> = items.iter().map(|i| i.description.clone()).collect();
    assert_eq!(got, vec!["setembro"]);

    // Inclusive end date: 2026-09-30 is included.
    let mut q = base_query();
    q.date_to = Some("2026-09-30".parse().unwrap());
    let s = summary(&pool, &q).await.unwrap();
    assert_eq!(s.count, 3);
}

#[test]
fn item_input_update_memory_defaults_to_on() {
    use deepsave_backend::models::ItemInput;

    // Omitted → feeds memory (a single edit is a correction).
    let input: ItemInput = serde_json::from_str(
        r#"{"occurred_on": "2026-01-01", "description": "x", "amount_cents": -100}"#,
    )
    .unwrap();
    assert!(input.update_memory);

    // Explicit opt-out.
    let input: ItemInput = serde_json::from_str(
        r#"{"occurred_on": "2026-01-01", "description": "x", "amount_cents": -100, "update_memory": false}"#,
    )
    .unwrap();
    assert!(!input.update_memory);
}

#[sqlx::test]
async fn multi_category_and_multi_tag_filters(pool: PgPool) {
    common::migrate(&pool).await;
    let cat_a: sqlx::types::Uuid =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('CatA') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let cat_b: sqlx::types::Uuid =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('CatB') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let cat_c: sqlx::types::Uuid =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('CatC') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();

    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents, category_id, tags)
         VALUES ('manual', 'expense', 'confirmed', '2026-07-01', 'em A', -100, $1, '{x}'),
                ('manual', 'expense', 'confirmed', '2026-07-01', 'em B', -200, $2, '{y}'),
                ('manual', 'expense', 'confirmed', '2026-07-01', 'em C', -300, $3, '{x,y}')",
    )
    .bind(cat_a)
    .bind(cat_b)
    .bind(cat_c)
    .execute(&pool)
    .await
    .unwrap();

    let names = |items: Vec<deepsave_backend::models::Item>| {
        let mut v: Vec<String> = items.into_iter().map(|i| i.description).collect();
        v.sort();
        v
    };

    // Multi-category: OR semantics (comma-separated).
    let mut q = base_query();
    q.category_ids = Some(format!("{cat_a},{cat_b}"));
    let got = names(list_items(&pool, &q).await.unwrap());
    assert_eq!(got, vec!["em A", "em B"]);

    // Multi-tag: OR semantics (item carries any of the tags).
    let mut q = base_query();
    q.tags = Some("x".to_string());
    assert_eq!(names(list_items(&pool, &q).await.unwrap()), vec!["em A", "em C"]);
    let mut q = base_query();
    q.tags = Some("x,y".to_string());
    assert_eq!(names(list_items(&pool, &q).await.unwrap()), vec!["em A", "em B", "em C"]);

    // Combined + summary honors them.
    let mut q = base_query();
    q.category_ids = Some(cat_c.to_string());
    q.tags = Some("x".to_string());
    let s = summary(&pool, &q).await.unwrap();
    assert_eq!(s.count, 1);
    assert_eq!(s.total_cents, -300);
}

#[sqlx::test]
async fn none_sentinel_filters_uncategorized_and_untagged(pool: PgPool) {
    common::migrate(&pool).await;
    let cat_a: sqlx::types::Uuid =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('Com Alimentação (teste)') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();

    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents, category_id, tags)
         VALUES ('manual', 'expense', 'confirmed', '2026-07-01', 'com categoria', -100, $1, '{x}'),
                ('manual', 'expense', 'confirmed', '2026-07-01', 'sem categoria', -200, NULL, '{y}'),
                ('manual', 'expense', 'confirmed', '2026-07-01', 'sem tags', -300, $1, '{}')",
    )
    .bind(cat_a)
    .execute(&pool)
    .await
    .unwrap();

    let names = |items: Vec<deepsave_backend::models::Item>| {
        let mut v: Vec<String> = items.into_iter().map(|i| i.description).collect();
        v.sort();
        v
    };

    // "__none" alone → only uncategorized.
    let mut q = base_query();
    q.category_ids = Some("__none".to_string());
    assert_eq!(
        names(list_items(&pool, &q).await.unwrap()),
        vec!["sem categoria"]
    );

    // "__none" + a real id → uncategorized OR that category.
    let mut q = base_query();
    q.category_ids = Some(format!("__none,{cat_a}"));
    assert_eq!(
        names(list_items(&pool, &q).await.unwrap()),
        vec!["com categoria", "sem categoria", "sem tags"]
    );

    // Tags "__none" → only items without tags.
    let mut q = base_query();
    q.tags = Some("__none".to_string());
    assert_eq!(names(list_items(&pool, &q).await.unwrap()), vec!["sem tags"]);

    // Tags "__none,y" → untagged OR carrying tag y.
    let mut q = base_query();
    q.tags = Some("__none,y".to_string());
    assert_eq!(
        names(list_items(&pool, &q).await.unwrap()),
        vec!["sem categoria", "sem tags"]
    );

    // Summary honors it too.
    let mut q = base_query();
    q.category_ids = Some("__none".to_string());
    let s = summary(&pool, &q).await.unwrap();
    assert_eq!(s.count, 1);
    assert_eq!(s.total_cents, -200);
}

#[sqlx::test]
async fn first_only_shows_full_price_in_list_and_summary(pool: PgPool) {
    common::migrate(&pool).await;
    insert_item(&pool, "celular 1/10", -1500, Some(1), Some(10)).await;
    insert_item(&pool, "celular 2/10", -1500, Some(2), Some(10)).await;
    insert_item(&pool, "padaria", -50, None, None).await;

    // Default: every parcel at its own amount.
    let all = list_items(&pool, &base_query()).await.unwrap();
    assert_eq!(all.len(), 3);

    // first_only: only the first parcel remains, at the FULL price (1500 × 10).
    let mut q = base_query();
    q.installments = Some("first_only".to_string());
    let items = list_items(&pool, &q).await.unwrap();
    assert_eq!(items.len(), 2);
    let first = items.iter().find(|i| i.description == "celular 1/10").unwrap();
    assert_eq!(first.amount_cents, -15000);
    let padaria = items.iter().find(|i| i.description == "padaria").unwrap();
    assert_eq!(padaria.amount_cents, -50);

    let s = summary(&pool, &q).await.unwrap();
    assert_eq!(s.count, 2);
    assert_eq!(s.total_cents, -15050);
}
