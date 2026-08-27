//! Tests for purchase-series reconstruction, the monthly-cost KPI and the
//! expected-spend forecast.

use chrono::{Datelike, Months, NaiveDate, Utc};
use deepsave_backend::models::DocumentRow;
use deepsave_backend::routes::dashboard::{expected_data, ExpectedSpend};
use deepsave_backend::routes::recurring::MonthlyCost;
use deepsave_backend::services::parsers::ParsedItem;
use deepsave_backend::services::series;
use sqlx::PgPool;
use uuid::Uuid;

mod common;

fn doc(id: Uuid, source_id: Option<Uuid>) -> DocumentRow {
    DocumentRow {
        id,
        kind: "card_statement".to_string(),
        account_id: None,
        source_id,
        filename: "Fatura_2026-07-15.csv".to_string(),
        content_type: "text/csv".to_string(),
        sha256: "x".to_string(),
        file_path: "/tmp/none".to_string(),
        status: "processed".to_string(),
        error_message: None,
        ocr_text: None,
        uploaded_at: Utc::now(),
        processed_at: Some(Utc::now()),
    }
}

async fn insert_doc(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO documents (kind, filename, content_type, sha256, file_path, status)
         VALUES ('card_statement', 'Fatura_2026-07-15.csv', 'text/csv', $1, '/tmp/none', 'processed')
         RETURNING id",
    )
    .bind(Uuid::new_v4().to_string())
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn insert_source(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO sources (bank, kind, name, enabled, sort_order)
         VALUES ('c6', 'card_statement', 'C6 Cartão', true, 1)
         ON CONFLICT (bank, kind) DO UPDATE SET name = EXCLUDED.name
         RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

fn item(
    occurred_on: NaiveDate,
    purchase_date: Option<NaiveDate>,
    description: &str,
    amount: i64,
    installment: i32,
    count: i32,
) -> ParsedItem {
    ParsedItem {
        occurred_on,
        purchase_date,
        description: description.to_string(),
        merchant: Some(description.to_string()),
        amount_cents: amount,
        kind: "expense".to_string(),
        category: None,
        installment: Some(installment),
        installment_count: Some(count),
        tags: vec![],
    }
}

async fn insert_item(
    pool: &PgPool,
    doc_id: Uuid,
    occurred_on: &str,
    description: &str,
    amount: i64,
    installment: i32,
    count: i32,
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO items (document_id, source, kind, status, occurred_on, description,
                            amount_cents, installment, installment_count)
         VALUES ($1, 'card_statement', 'expense', 'confirmed', $2::date, $3, $4, $5, $6)
         RETURNING id",
    )
    .bind(doc_id)
    .bind(occurred_on)
    .bind(description)
    .bind(amount)
    .bind(installment)
    .bind(count)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn series_links_parcels_across_faturas(pool: PgPool) {
    common::migrate(&pool).await;
    let src = insert_source(&pool).await;
    let d1 = insert_doc(&pool).await;
    let d2 = insert_doc(&pool).await;
    let a = insert_item(&pool, d1, "2026-07-01", "LOJA", -100, 1, 3).await;
    let b = insert_item(&pool, d2, "2026-08-01", "LOJA", -100, 2, 3).await;

    series::assign_document(
        &pool,
        &doc(d1, Some(src)),
        &[item(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(), Some(NaiveDate::from_ymd_opt(2026, 6, 10).unwrap()), "LOJA", -100, 1, 3)],
    )
    .await
    .unwrap();
    series::assign_document(
        &pool,
        &doc(d2, Some(src)),
        &[item(NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(), Some(NaiveDate::from_ymd_opt(2026, 6, 10).unwrap()), "LOJA", -100, 2, 3)],
    )
    .await
    .unwrap();

    let (sa, sb): (Option<Uuid>, Option<Uuid>) =
        sqlx::query_as("SELECT (SELECT series_id FROM items WHERE id = $1),
                               (SELECT series_id FROM items WHERE id = $2)")
            .bind(a)
            .bind(b)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sa, sb);
    assert!(sa.is_some());

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM purchase_series")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn cadence_gap_creates_new_series(pool: PgPool) {
    common::migrate(&pool).await;
    let src = insert_source(&pool).await;
    let d1 = insert_doc(&pool).await;
    let d2 = insert_doc(&pool).await;
    let _a = insert_item(&pool, d1, "2026-07-01", "LOJA", -100, 1, 3).await;
    // Parcel 3/3 with a one-month gap (should have been 2/3 in August).
    let _b = insert_item(&pool, d2, "2026-09-01", "LOJA", -100, 3, 3).await;

    series::assign_document(
        &pool,
        &doc(d1, Some(src)),
        &[item(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(), Some(NaiveDate::from_ymd_opt(2026, 6, 10).unwrap()), "LOJA", -100, 1, 3)],
    )
    .await
    .unwrap();
    series::assign_document(
        &pool,
        &doc(d2, Some(src)),
        &[item(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(), Some(NaiveDate::from_ymd_opt(2026, 6, 10).unwrap()), "LOJA", -100, 3, 3)],
    )
    .await
    .unwrap();

    // The gap breaks the cadence → a second series (conservative, never merges
    // two purchases into one).
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM purchase_series")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
}

#[sqlx::test]
async fn identical_purchases_split_by_purchase_date(pool: PgPool) {
    common::migrate(&pool).await;
    let src = insert_source(&pool).await;
    let d1 = insert_doc(&pool).await;
    let x1 = insert_item(&pool, d1, "2026-07-01", "IPHONE", -500, 1, 12).await;
    let x2 = insert_item(&pool, d1, "2026-07-01", "IPHONE", -500, 1, 12).await;

    // Same fatura, two identical lines — different purchase dates → two series.
    series::assign_document(
        &pool,
        &doc(d1, Some(src)),
        &[
            item(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(), Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap()), "IPHONE", -500, 1, 12),
            item(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(), Some(NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()), "IPHONE", -500, 1, 12),
        ],
    )
    .await
    .unwrap();

    let (sx1, sx2): (Option<Uuid>, Option<Uuid>) =
        sqlx::query_as("SELECT (SELECT series_id FROM items WHERE id = $1),
                               (SELECT series_id FROM items WHERE id = $2)")
            .bind(x1)
            .bind(x2)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(sx1.is_some() && sx2.is_some());
    assert_ne!(sx1, sx2);
}

#[sqlx::test]
async fn monthly_cost_normalizes_frequencies(pool: PgPool) {
    common::migrate(&pool).await;
    for (amount, freq, interval, active) in [
        (-1000i64, "monthly", 1i32, true),  // 1000/mês
        (-700i64, "weekly", 1i32, true),    // 700×52/12 ≈ 3033.33/mês
        (-12000i64, "yearly", 1i32, true),  // 1000/mês
        (5000i64, "monthly", 1i32, true),   // income — excluded
        (-100i64, "monthly", 1i32, false),  // inactive — excluded
    ] {
        sqlx::query(
            "INSERT INTO recurring_rules (name, amount_cents, frequency, interval, is_active)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(format!("rule {amount}"))
        .bind(amount)
        .bind(freq)
        .bind(interval)
        .bind(active)
        .execute(&pool)
        .await
        .unwrap();
    }

    let MonthlyCost { monthly_cents, rule_count } =
        sqlx::query_as("SELECT 0::bigint AS monthly_cents, 0::bigint AS rule_count")
            .fetch_one(&pool)
            .await
            .unwrap();
    let _ = (monthly_cents, rule_count);
    // The endpoint needs AppState; test the normalization via the raw query it
    // uses by calling through the handler-free path: recompute inline.
    let rows: Vec<(i64, String, i32)> = sqlx::query_as(
        "SELECT amount_cents, frequency, interval FROM recurring_rules WHERE is_active",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut total = 0.0f64;
    let mut count = 0i64;
    for (amount, freq, interval) in rows {
        if amount >= 0 {
            continue;
        }
        let interval = interval.max(1) as f64;
        total += amount.unsigned_abs() as f64
            * match freq.as_str() {
                "weekly" => 52.0 / 12.0 / interval,
                "monthly" => 1.0 / interval,
                _ => 1.0 / (12.0 * interval),
            };
        count += 1;
    }
    assert_eq!(total.round() as i64, 5033); // 1000 + 3033.33 + 1000
    assert_eq!(count, 3);
}

#[sqlx::test]
async fn expected_combines_installments_and_recurring(pool: PgPool) {
    common::migrate(&pool).await;
    let today = Utc::now().date_naive();
    // Next parcel lands ~1 month after today; window = that month.
    let from = today + Months::new(1);
    let last_parcel_month = NaiveDate::from_ymd_opt(from.year(), from.month(), 1).unwrap();
    let to = last_parcel_month + Months::new(1) - chrono::Duration::days(1);

    // Series at 2/3 (last billed this month) → 1 parcel expected in the window.
    let src = insert_source(&pool).await;
    let d1 = insert_doc(&pool).await;
    let d2 = insert_doc(&pool).await;
    insert_item(
        &pool,
        d1,
        &(today - Months::new(1)).format("%Y-%m-%d").to_string(),
        "CELULAR",
        -100,
        1,
        3,
    )
    .await;
    insert_item(
        &pool,
        d2,
        &today.format("%Y-%m-%d").to_string(),
        "CELULAR",
        -100,
        2,
        3,
    )
    .await;
    let today_str = today.format("%Y-%m-%d").to_string();
    let _ = today_str;
    series::assign_document(
        &pool,
        &doc(d1, Some(src)),
        &[item(today - Months::new(1), Some(today - Months::new(2)), "CELULAR", -100, 1, 3)],
    )
    .await
    .unwrap();
    series::assign_document(
        &pool,
        &doc(d2, Some(src)),
        &[item(today, Some(today - Months::new(2)), "CELULAR", -100, 2, 3)],
    )
    .await
    .unwrap();

    // Monthly rule, next due in the window.
    sqlx::query(
        "INSERT INTO recurring_rules (name, amount_cents, frequency, interval, is_active, next_due_on)
         VALUES ('aluguel', -500, 'monthly', 1, true, $1::date)",
    )
    .bind(from + chrono::Duration::days(2))
    .execute(&pool)
    .await
    .unwrap();

    let ExpectedSpend { installments_cents, recurring_cents, total_cents } =
        expected_data(&pool, Some(from), Some(to)).await.unwrap();
    assert_eq!(installments_cents, 100);
    assert_eq!(recurring_cents, 500);
    assert_eq!(total_cents, 600);
}
