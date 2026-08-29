//! Integration tests for the system overview (`routes::system`).

use deepsave_backend::routes::system::system_data;
use sqlx::PgPool;
use std::path::Path;

mod common;

#[sqlx::test]
async fn system_info_reports_sizes_and_table_counts(pool: PgPool) {
    common::migrate(&pool).await;

    let info = system_data(&pool, Path::new("/nonexistent-storage-dir"))
        .await
        .unwrap();

    // Database size is reported (migrations create real relations).
    assert!(info.db_size_bytes > 0);

    // Missing storage dir → zeroes, not an error.
    assert_eq!(info.storage_size_bytes, 0);
    assert_eq!(info.storage_file_count, 0);

    // Every migration-tracked table is listed.
    let tables: Vec<&str> = info.table_counts.iter().map(|t| t.table.as_str()).collect();
    for expected in [
        "accounts",
        "ai_calls",
        "ai_tag_batches",
        "ai_tag_suggestions",
        "categories",
        "documents",
        "items",
        "matches",
        "merchant_memory",
        "recurring_aliases",
        "recurring_rules",
        "sources",
        "users",
    ] {
        assert!(tables.contains(&expected), "missing table {expected}");
    }

    // Fresh DB: zero rows everywhere, but relation sizes are still positive.
    let items = info.table_counts.iter().find(|t| t.table == "items").unwrap();
    assert_eq!(items.count, 0);
    assert!(items.size_bytes > 0);

    // Status breakdowns exist (may be empty on a fresh DB).
    assert!(info.items_by_status.is_empty());
}

#[sqlx::test]
async fn system_info_counts_rows_and_groups_by_status(pool: PgPool) {
    common::migrate(&pool).await;

    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('pluggy', 'expense', 'confirmed', '2026-07-01', 'Teste', -1000)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents)
         VALUES ('pluggy', 'expense', 'confirmed', '2026-07-02', 'Teste 2', -2000)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let info = system_data(&pool, Path::new("/nonexistent-storage-dir"))
        .await
        .unwrap();

    let items = info.table_counts.iter().find(|t| t.table == "items").unwrap();
    assert_eq!(items.count, 2);

    let confirmed = info
        .items_by_status
        .iter()
        .find(|s| s.status == "confirmed")
        .unwrap();
    assert_eq!(confirmed.count, 2);
}
