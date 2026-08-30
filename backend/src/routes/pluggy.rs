use axum::extract::{Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::services::pluggy;
use crate::AppState;

/// `GET /api/pluggy/status` — is the integration configured + what's connected.
pub async fn status(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let (items, accounts): (i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT count(*) FROM items WHERE source = 'pluggy'),
           (SELECT count(*) FROM pluggy_accounts)",
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(json!({
        "configured": state.pluggy.is_configured() && !state.pluggy_accounts.is_empty(),
        "auth": state.pluggy.auth_mode(),
        "items": items,
        "accounts": accounts,
    })))
}

/// `GET /api/pluggy/accounts` — configured accounts with import stats.
pub async fn accounts(State(state): State<AppState>) -> Result<Json<Vec<Value>>, AppError> {
    let rows: Vec<(String, String, Option<String>, Option<String>, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as(
            "SELECT pluggy_account_id, name, account_type, bank, last_sync_at
             FROM pluggy_accounts ORDER BY bank, name",
        )
        .fetch_all(&state.pool)
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for (pluggy_account_id, name, account_type, bank, last_sync_at) in rows {
        let stats: (i64, Option<chrono::NaiveDate>, Option<chrono::NaiveDate>) = sqlx::query_as(
            "SELECT count(*), min(occurred_on), max(occurred_on)
             FROM items i
             JOIN pluggy_accounts pa ON pa.account_id = i.account_id
             WHERE pa.pluggy_account_id = $1",
        )
        .bind(&pluggy_account_id)
        .fetch_one(&state.pool)
        .await?;

        out.push(json!({
            "pluggy_account_id": pluggy_account_id,
            "name": name,
            "account_type": account_type,
            "bank": bank,
            "last_sync_at": last_sync_at,
            "item_count": stats.0,
            "first_date": stats.1,
            "last_date": stats.2,
        }));
    }
    Ok(Json(out))
}

/// `POST /api/pluggy/sync?from=YYYY-MM-DD&to=YYYY-MM-DD` — refresh the account
/// list from `.env`, then import every account's transactions.
///
/// Without `from`/`to` the pull is **incremental** (only transactions newer
/// than the last import). Pass `from`/`to` to force a full re-pull of a
/// custom period (omit both on an empty account to pull full history).
#[derive(Debug, Deserialize)]
pub struct SyncQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

pub async fn sync(
    State(state): State<AppState>,
    Query(q): Query<SyncQuery>,
) -> Result<Json<Value>, AppError> {
    let client = state.pluggy.client().ok_or_else(pluggy_not_configured)?;

    // Pick up any .env changes without a restart.
    let seeded = pluggy::seed_configured_accounts(&state.pool, &state.pluggy_accounts).await?;
    let results = pluggy::sync_all_accounts(&state.pool, client, q.from, q.to).await?;
    // MCC → category rule (zero-cost, deterministic) for uncategorized card items.
    let mcc_categorized = crate::services::mcc::apply_mcc_categories(&state.pool).await?;
    // Assign installment items to purchase series (feeds the forecast).
    let series = pluggy::assign_installment_series(&state.pool).await?;
    // Link refunds to the charges they reverse (for graph netting).
    let linked_refunds = pluggy::link_refunds(&state.pool).await?;

    let total_new: usize = results.iter().map(|r| r.new).sum();
    Ok(Json(json!({
        "configured": seeded,
        "accounts": results,
        "new": total_new,
        "mcc_categorized": mcc_categorized,
        "series": series,
        "linked_refunds": linked_refunds,
    })))
}

fn pluggy_not_configured() -> AppError {
    AppError::bad_request(
        "Pluggy não configurado (faltam PLUGGY_API_KEY ou PLUGGY_CLIENT_ID / PLUGGY_CLIENT_SECRET e PLUGGY_ACCOUNTS)",
    )
}
