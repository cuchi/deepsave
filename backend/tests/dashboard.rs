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
        category_ids: None,
        kind: None,
        tags: None,
        bank: None,
        installments: None,
    }
}

fn trend_q() -> TrendQuery {
    TrendQuery {
        months: None,
        date_to: None,
        search: None,
        category_ids: None,
        kind: None,
        tags: None,
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
    insert_item(&pool, "Linha do recibo", -3000, "2026-07-10", "expense").await;
    insert_item(&pool, "Salário", 20000, "2026-07-05", "income").await;
    insert_item(&pool, "Lixo", -100, "2026-07-01", "expense").await;
    sqlx::query("UPDATE items SET status = 'rejected' WHERE description = 'Lixo'")
        .execute(&pool)
        .await
        .unwrap();

    let d = dashboard_data(&pool, &dash_q()).await.unwrap();
    // Rejected excluded; spend = 5000 + 3000 (both expenses), income = 20000.
    assert_eq!(d.total_spend_cents, 8000);
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

async fn daily_q() -> deepsave_backend::routes::dashboard::DailyQuery {
    deepsave_backend::routes::dashboard::DailyQuery {
        date_from: None,
        date_to: None,
        search: None,
        category_ids: None,
        kind: None,
        tags: None,
        bank: None,
        installments: None,
        stack_by: "category".to_string(),
    }
}

#[sqlx::test]
async fn daily_stacks_expenses_by_category(pool: PgPool) {
    use deepsave_backend::routes::dashboard::{daily_data, tags_data};
    common::migrate(&pool).await;

    let cat_a: sqlx::types::Uuid =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('Alimentação (teste)') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let cat_b: sqlx::types::Uuid =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('Transporte (teste)') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();

    // Two expenses in cat_a on 2026-07-01, one in cat_b, one income, one child.
    for (desc, amt, cat, kind) in [
        ("a1", -100, Some(cat_a), "expense"),
        ("a2", -200, Some(cat_a), "expense"),
        ("b1", -50, Some(cat_b), "expense"),
        ("inc", 900, None, "income"),
        ("child", -77, None, "expense"),
    ] {
        sqlx::query(
            "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents, category_id)
             VALUES ('manual', $1, 'confirmed', '2026-07-01', $2, $3, $4)",
        )
        .bind(kind)
        .bind(desc)
        .bind(amt)
        .bind(cat)
        .execute(&pool)
        .await
        .unwrap();
    }
    let q = daily_q().await;
    let rows = daily_data(&pool, &q).await.unwrap();
    // Expenses on that day: a1+a2 = 300, child = 50, b1 = 50. Income excluded.
    let by_key: std::collections::HashMap<_, _> = rows
        .iter()
        .map(|r| (r.key.clone().unwrap_or_default(), r.total_cents))
        .collect();
    assert_eq!(by_key.get("Alimentação (teste)"), Some(&300));
    assert_eq!(by_key.get("Transporte (teste)"), Some(&50));

    // stack_by=none → single per-day total (child included: 300 + 50 + 77).
    let mut q = daily_q().await;
    q.stack_by = "none".to_string();
    let rows = daily_data(&pool, &q).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].total_cents, 427);
    assert_eq!(rows[0].key, None);

    // Top tags: item with two tags counts in both.
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents, tags)
         VALUES ('manual', 'expense', 'confirmed', '2026-07-02', 'uber', -100, '{transporte,viagem}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let q = daily_q().await;
    let tags = tags_data(&pool, &q).await.unwrap();
    let by_tag: std::collections::HashMap<_, _> =
        tags.iter().map(|t| (t.tag.as_str(), t.total_cents)).collect();
    assert_eq!(by_tag.get("transporte"), Some(&100));
    assert_eq!(by_tag.get("viagem"), Some(&100));
}

#[sqlx::test]
async fn daily_respects_date_range_filter(pool: PgPool) {
    use deepsave_backend::routes::dashboard::daily_data;
    common::migrate(&pool).await;
    for (day, amt) in [("2026-07-01", -100), ("2026-08-15", -200)] {
        sqlx::query(
            "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
             VALUES ('manual', 'expense', 'confirmed', $1::date, 'x', $2)",
        )
        .bind(day)
        .bind(amt)
        .execute(&pool)
        .await
        .unwrap();
    }

    let mut q = daily_q().await;
    q.stack_by = "none".to_string();
    q.date_from = Some("2026-08-01".parse().unwrap());
    q.date_to = Some("2026-08-31".parse().unwrap());
    let rows = daily_data(&pool, &q).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].total_cents, 200);
}

#[sqlx::test]
async fn linked_refunds_net_against_their_charge(pool: PgPool) {
    common::migrate(&pool).await;
    insert_item(&pool, "Tarifa Anuidade", -9800, "2026-01-01", "expense").await;
    insert_item(&pool, "Estorno Tarifa", 9800, "2026-01-03", "refund").await;
    // An unlinked refund must NOT affect spend.
    insert_item(&pool, "Refund solto", 5000, "2026-01-05", "refund").await;
    insert_item(&pool, "Compra normal", -3000, "2026-01-10", "expense").await;

    // Link the estorno to its charge.
    let charge_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM items WHERE description = 'Tarifa Anuidade'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE items SET refunded_item_id = $1 WHERE description = 'Estorno Tarifa'")
        .bind(charge_id)
        .execute(&pool)
        .await
        .unwrap();

    let d = dashboard_data(&pool, &dash_q()).await.unwrap();
    // 9800 charge − 9800 linked refund = 0; the unlinked +5000 refund is not
    // counted anywhere; the -3000 expense remains.
    assert_eq!(d.total_spend_cents, 3000);
    assert_eq!(d.total_income_cents, 0);
}
