//! System overview: database + storage sizes, per-table row counts.

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use sqlx::PgPool;
use std::path::Path;

use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct TableCount {
    pub table: String,
    pub count: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct SystemInfo {
    /// Disk space used by the current database (relations + indexes + TOAST).
    pub db_size_bytes: i64,
    /// Total size of uploaded files under STORAGE_DIR.
    pub storage_size_bytes: u64,
    /// Number of files under STORAGE_DIR.
    pub storage_file_count: u64,
    pub table_counts: Vec<TableCount>,
    pub items_by_status: Vec<StatusCount>,
}

pub async fn system(State(state): State<AppState>) -> Result<Json<SystemInfo>, AppError> {
    Ok(Json(system_data(&state.pool, &state.storage_dir).await?))
}

/// Pool-level core so integration tests can drive it without an `AppState`.
pub async fn system_data(pool: &PgPool, storage_dir: &Path) -> Result<SystemInfo, AppError> {
    let db_size_bytes: i64 = sqlx::query_scalar(
        "SELECT pg_database_size(current_database())::bigint",
    )
    .fetch_one(pool)
    .await?;

    let table_counts = table_counts(pool).await?;
    let items_by_status = status_counts(pool, "items").await?;

    let (storage_size_bytes, storage_file_count) = storage_size(storage_dir).await;

    Ok(SystemInfo {
        db_size_bytes,
        storage_size_bytes,
        storage_file_count,
        table_counts,
        items_by_status,
    })
}

/// Row counts + on-disk size for every table in the `public` schema.
/// Table names come from `information_schema`; each identifier is re-validated
/// before being interpolated (only ever lowercase [a-z0-9_]).
async fn table_counts(pool: &PgPool) -> Result<Vec<TableCount>, AppError> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
         ORDER BY table_name",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(tables.len());
    for table in tables {
        if !is_safe_ident(&table) {
            continue;
        }
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*)::bigint FROM {table}"
        )))
        .fetch_one(pool)
        .await?;
        let size_bytes: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COALESCE(pg_total_relation_size('{table}'::regclass), 0)::bigint"
        )))
        .fetch_one(pool)
        .await?;
        out.push(TableCount {
            table,
            count,
            size_bytes,
        });
    }
    Ok(out)
}

fn is_safe_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `SELECT status, count(*) FROM <table> GROUP BY status` — `table` must be a
/// static, trusted identifier ("items" / "documents").
async fn status_counts(pool: &PgPool, table: &'static str) -> Result<Vec<StatusCount>, AppError> {
    Ok(sqlx::query_as::<_, StatusCount>(sqlx::AssertSqlSafe(format!(
        "SELECT status, count(*)::bigint FROM {table} GROUP BY status ORDER BY status"
    )))
    .fetch_all(pool)
    .await?)
}

/// Recursive size + file count of a directory (missing/unreadable dirs → 0s).
async fn storage_size(dir: &Path) -> (u64, u64) {
    let mut total = 0u64;
    let mut files = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(mut rd) = tokio::fs::read_dir(&d).await else {
            continue;
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
                files += 1;
            }
        }
    }
    (total, files)
}
