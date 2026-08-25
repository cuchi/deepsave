//! Tests for the filterable dashboard/trend aggregation (pool-level functions).

use deepsave_backend::routes::dashboard::{dashboard_data, trend_data, DashboardQuery, TrendQuery};
use sqlx::PgPool;

mod common;

fn dash_q() -> DashboardQuery {
    DashboardQuery {
        month: None,
        date_from: None,
        date_to: None,
        search: None,
        category_id: None,
        kind: None,
        tag: None,
        bank: None,
        installments: None,
    }
}

fn trend_q() -> TrendQuery {
    TrendQuery {
        months: None,
        date_to: None,
        search: None,
        category_id: None,
        kind: None,
        tag: None,
        bank: None,
        installments: None,
    }
}

async fn insert_item(
    pool: &PgPool,
    description: &str,
    amount_cents: i64,
    occurred_on: &str,
    kind: &str,
) {
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('manual', $1, 'confirmed', $2::date, $3, $4)",
    )
    .bind(kind)
    .bind(occurred_on)
    .bind(description)
    .bind(amount_cents)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn dashboard_counts_roots_only_and_excludes_rejected(pool: PgPool) {
    common::migrate(&pool).await;
    insert_item(&pool, "Compra", -5000, "2026-07-10", "expense").await;
    // Receipt child (double-count guard): linked under the root.
    let root_id: sqlx::types::Uuid = sqlx::query_scalar(
        "SELECT id FROM items WHERE description = 'Compra'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    insert_item(&pool, "Linha do recibo", -3000, "2026-07-10", "expense").await;
    sqlx::query("UPDATE items SET parent_id = $1 WHERE description = 'Linha do recibo'")
        .bind(root_id)
        .execute(&pool)
        .await
        .unwrap();
    insert_item(&pool, "Salário", 20000, "2026-07-05", "income").await;
    insert_item(&pool, "Lixo", -100, "2026-07-01", "expense").await;
    sqlx::query("UPDATE items SET status = 'rejected' WHERE description = 'Lixo'")
        .execute(&pool)
        .await
        .unwrap();

    let d = dashboard_data(&pool, &dash_q()).await.unwrap();
    // Child + rejected excluded; spend = 5000 (root), income = 20000.
    assert_eq!(d.total_spend_cents, 5000);
    assert_eq!(d.total_income_cents, 20000);
}

#[sqlx::test]
async fn dashboard_respects_date_range_and_kind_filter(pool: PgPool) {
    common::migrate(&pool).await;
    insert_item(&pool, "Julho", -1000, "2026-07-15", "expense").await;
    insert_item(&pool, "Agosto", -2000, "2026-08-15", "expense").await;

    let mut q = dash_q();
    q.date_from = Some("2026-08-01".parse().unwrap());
    q.date_to = Some("2026-08-31".parse().unwrap());
    let d = dashboard_data(&pool, &q).await.unwrap();
    assert_eq!(d.total_spend_cents, 2000);

    // Kind filter: income only → spend 0.
    let mut q = dash_q();
    q.kind = Some("income".to_string());
    let d = dashboard_data(&pool, &q).await.unwrap();
    assert_eq!(d.total_spend_cents, 0);
    assert_eq!(d.total_income_cents, 0);
}

#[sqlx::test]
async fn trend_window_ends_at_date_to(pool: PgPool) {
    common::migrate(&pool).await;
    insert_item(&pool, "Julho", -1000, "2026-07-15", "expense").await;
    insert_item(&pool, "Agosto", -2000, "2026-08-15", "expense").await;

    // Window ending at July: August is outside → only July shows spend.
    let mut q = trend_q();
    q.date_to = Some("2026-07-31".parse().unwrap());
    let t = trend_data(&pool, &q).await.unwrap();
    assert_eq!(t.len(), 12);
    assert_eq!(t.last().unwrap().month, "2026-07");
    assert_eq!(t.last().unwrap().spend_cents, 1000);
    assert!(!t.iter().any(|p| p.month == "2026-08"));

    // Search filter applies to the trend (window ends at August).
    let mut q = trend_q();
    q.date_to = Some("2026-08-31".parse().unwrap());
    q.search = Some("agosto".to_string());
    let t = trend_data(&pool, &q).await.unwrap();
    let august = t.iter().find(|p| p.month == "2026-08").unwrap();
    assert_eq!(august.spend_cents, 2000);
}
