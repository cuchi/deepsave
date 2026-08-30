use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ChangeLogQuery {
    /// Substring filter on the merchant identity (accent/case-insensitive).
    pub merchant: Option<String>,
    /// 'item_edit' | 'bulk' | 'memory_apply' | 'ai_apply'
    pub source: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

/// `GET /api/change-log` — the user's category/tag change history (most recent
/// first), for the "Histórico" tab.
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ChangeLogQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let merchant = q
        .merchant
        .as_deref()
        .map(|m| crate::services::change_log::merchant_key(Some(m), ""))
        .filter(|k| !k.is_empty());
    let limit = q.limit.clamp(1, 500);

    #[derive(sqlx::FromRow)]
    struct Row {
        created_at: chrono::DateTime<chrono::Utc>,
        merchant_key: String,
        category_before: Option<String>,
        category_after: Option<String>,
        description: Option<String>,
        tags_before: Vec<String>,
        tags_after: Vec<String>,
        source: String,
        merchant: Option<String>,
        tx_date: Option<chrono::NaiveDate>,
        amount_cents: Option<i64>,
        kind: Option<String>,
        bank: Option<String>,
        mcc: Option<i32>,
        pluggy_category: Option<String>,
        operation_type: Option<String>,
        current_category: Option<String>,
        current_tags: Vec<String>,
    }

    let rows: Vec<Row> = sqlx::query_as::<_, Row>(
        "SELECT cl.created_at, cl.merchant_key,
                cb.name AS category_before, ca.name AS category_after, i.description,
                cl.tags_before, cl.tags_after, cl.source, i.merchant, cl.tx_date,
                i.amount_cents, i.kind, pa.bank, i.mcc, i.pluggy_category, i.operation_type,
                ci.name AS current_category, COALESCE(i.tags, '{}') AS current_tags
         FROM change_log cl
         LEFT JOIN items i ON i.id = cl.item_id
         LEFT JOIN pluggy_accounts pa ON pa.account_id = i.account_id
         LEFT JOIN categories cb ON cb.id = cl.category_before
         LEFT JOIN categories ca ON ca.id = cl.category_after
         LEFT JOIN categories ci ON ci.id = i.category_id
         WHERE ($1::text IS NULL OR cl.merchant_key ILIKE '%' || $1 || '%')
           AND ($2::text IS NULL OR cl.source = $2)
         ORDER BY cl.created_at DESC
         LIMIT $3",
    )
    .bind(merchant)
    .bind(&q.source)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| {
                json!({
                    "created_at": r.created_at,
                    "merchant": r.merchant.clone().or_else(|| r.description.clone()),
                    "merchant_key": r.merchant_key,
                    "category_before": r.category_before,
                    "category_after": r.category_after,
                    "tags_before": r.tags_before,
                    "tags_after": r.tags_after,
                    "source": r.source,
                    "description": r.description,
                    "tx_date": r.tx_date,
                    "amount_cents": r.amount_cents,
                    "kind": r.kind,
                    "bank": r.bank,
                    "mcc": r.mcc,
                    "pluggy_category": r.pluggy_category,
                    "operation_type": r.operation_type,
                    "current_category": r.current_category,
                    "current_tags": r.current_tags,
                })
            })
            .collect::<Vec<_>>(),
    ))
}
