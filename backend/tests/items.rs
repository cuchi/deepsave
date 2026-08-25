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
        category_id: None,
        kind: None,
        tag: None,
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
    assert_eq!(s.count, 1);
    assert_eq!(s.total_cents, -100);

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
