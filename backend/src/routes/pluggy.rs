use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use crate::error::AppError;
use crate::services::pluggy::{
    self, CreateItemRequest, LocalPluggyAccount, LocalPluggyItem, PluggyClient,
};
use crate::AppState;

/// `GET /api/pluggy/status` — is the integration configured + what's connected.
pub async fn status(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let (items, accounts): (i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT count(*) FROM pluggy_items),
           (SELECT count(*) FROM pluggy_accounts)",
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(json!({
        "configured": state.pluggy.is_configured(),
        "auth": state.pluggy.auth_mode(),
        "items": items,
        "accounts": accounts,
    })))
}

/// `GET /api/pluggy/connectors` — proxy Pluggy's connector catalog (BR).
pub async fn connectors(State(state): State<AppState>) -> Result<Json<Vec<Value>>, AppError> {
    let client = state.pluggy.client().ok_or_else(pluggy_not_configured)?;
    let list = client.list_connectors(300).await?;
    let out: Vec<Value> = list
        .into_iter()
        .filter(|c| c.country.as_deref() == Some("BR"))
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "kind": c.kind,
                "oauth": c.oauth.unwrap_or(false),
                "mfa": c.has_mfa.unwrap_or(false),
                "open_finance": c.is_open_finance.unwrap_or(false),
                "image_url": c.image_url,
                "credentials": c.credentials.unwrap_or_default().iter().map(|cr| json!({
                    "name": cr.name,
                    "label": cr.label,
                    "type": cr.kind,
                    "optional": cr.optional.unwrap_or(false),
                    "placeholder": cr.placeholder,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(out))
}

fn pluggy_not_configured() -> AppError {
    AppError::bad_request(
        "Pluggy não configurado (faltam PLUGGY_API_KEY ou PLUGGY_CLIENT_ID / PLUGGY_CLIENT_SECRET)",
    )
}

/// `GET /api/pluggy/items` — local items with their accounts (balances, types).
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Value>>, AppError> {
    let items: Vec<LocalPluggyItem> = sqlx::query_as(
        "SELECT id, pluggy_id, connector_id, connector_name, status, error,
                last_sync_at, created_at
         FROM pluggy_items ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;

    let mut out = Vec::with_capacity(items.len());
    for it in items {
        let accounts: Vec<LocalPluggyAccount> = sqlx::query_as(
            "SELECT id, pluggy_account_id, pluggy_item_id, account_id, name, account_type,
                    subtype, currency, balance::float8 AS balance, credit_limit::float8 AS credit_limit,
                    due_date, close_date, last_sync_at
             FROM pluggy_accounts WHERE pluggy_item_id = $1 ORDER BY account_type, name",
        )
        .bind(it.id)
        .fetch_all(&state.pool)
        .await?;

        let item_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM items i
             JOIN pluggy_accounts pa ON pa.account_id = i.account_id
             WHERE pa.pluggy_item_id = $1",
        )
        .bind(it.id)
        .fetch_one(&state.pool)
        .await?;

        out.push(json!({
            "id": it.id,
            "pluggy_id": it.pluggy_id,
            "connector_id": it.connector_id,
            "connector_name": it.connector_name,
            "status": it.status,
            "error": it.error,
            "last_sync_at": it.last_sync_at,
            "created_at": it.created_at,
            "item_count": item_count,
            "accounts": accounts.iter().map(|a| json!({
                "id": a.id,
                "pluggy_account_id": a.pluggy_account_id,
                "account_id": a.account_id,
                "name": a.name,
                "account_type": a.account_type,
                "subtype": a.subtype,
                "currency": a.currency,
                "balance": a.balance,
                "credit_limit": a.credit_limit,
                "due_date": a.due_date,
                "close_date": a.close_date,
                "last_sync_at": a.last_sync_at,
            })).collect::<Vec<_>>(),
        }));
    }
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
pub struct CreateItemInput {
    pub connector_id: i32,
    #[serde(default)]
    pub parameters: Option<Value>,
    #[serde(default)]
    pub client_user_id: Option<String>,
}

/// `POST /api/pluggy/items` — connect a new bank. For OAuth connectors the
/// response carries `oauth_url` (open in a new tab; Pluggy handles the
/// callback itself) and the item stays `WAITING_USER_INPUT` until done.
pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateItemInput>,
) -> Result<Json<Value>, AppError> {
    let client = state.pluggy.client().ok_or_else(pluggy_not_configured)?;
    let req = CreateItemRequest {
        item_id: None,
        connector_id: input.connector_id,
        // Pluggy rejects items without a `parameters` object (even empty).
        parameters: Some(input.parameters.unwrap_or_else(|| json!({}))),
        client_user_id: input.client_user_id,
    };
    let item = client.create_item(&req).await?;
    let local_id = pluggy::upsert_pluggy_item(&state.pool, &item).await?;

    let mut out = pluggy_item_json(local_id, &item);
    out["oauth_url"] = json!(oauth_url(&item));
    Ok(Json(out))
}

/// `POST /api/pluggy/items/{id}/refresh` — re-fetch the Pluggy item status
/// (used to poll after OAuth / during sync).
pub async fn refresh(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let local = fetch_local_item(&state, id).await?;
    let client = state.pluggy.client().ok_or_else(pluggy_not_configured)?;
    let item = client.get_item(&local.pluggy_id).await?;
    let local_id = pluggy::upsert_pluggy_item(&state.pool, &item).await?;
    let mut out = pluggy_item_json(local_id, &item);
    out["oauth_url"] = json!(oauth_url(&item));
    Ok(Json(out))
}

/// `POST /api/pluggy/items/{id}/sync` — trigger a sync and poll briefly. When
/// the item reaches a terminal success state the accounts + transactions are
/// imported. Long syncs return `status: "UPDATING"` — poll `/refresh` then
/// call `/import`.
pub async fn sync(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let local = fetch_local_item(&state, id).await?;
    let client = state.pluggy.client().ok_or_else(pluggy_not_configured)?;

    // A fresh connection may be waiting for OAuth/MFA input — nothing to sync yet.
    if local.status == "WAITING_USER_INPUT" || local.status == "WAITING_USER_ACTION" {
        return Ok(Json(json!({ "status": local.status, "imported": 0 })));
    }

    let (status, imported) = sync_one(&state, client, &local).await?;
    let terminal = status == "UPDATED" || status == "LOGIN_DONE";
    Ok(Json(json!({
        "status": status,
        "imported": imported,
        "pending": !terminal && status != "ERROR" && status != "PARTIAL_SUCCESS",
    })))
}

/// `POST /api/pluggy/items/{id}/import` — fetch accounts + transactions and
/// import new items. Idempotent (keyed by Pluggy transaction id).
pub async fn import(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let local = fetch_local_item(&state, id).await?;
    let client = state.pluggy.client().ok_or_else(pluggy_not_configured)?;
    let imported = finish_import(&state, client, &local).await?;
    Ok(Json(json!({ "status": local.status, "imported": imported })))
}

/// `POST /api/pluggy/sync-all` — trigger sync + import for every item.
pub async fn sync_all(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let client = state.pluggy.client().ok_or_else(pluggy_not_configured)?;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM pluggy_items ORDER BY created_at")
        .fetch_all(&state.pool)
        .await?;
    let mut imported_total = 0;
    let mut done = 0;
    for id in ids {
        let local = fetch_local_item(&state, id).await?;
        if local.status == "WAITING_USER_INPUT" || local.status == "WAITING_USER_ACTION" {
            continue;
        }
        match sync_one(&state, client, &local).await {
            Ok((status, imported)) => {
                if status == "UPDATED" || status == "LOGIN_DONE" {
                    imported_total += imported;
                    done += 1;
                }
            }
            Err(e) => tracing::warn!(item = %id, "pluggy sync failed: {e:?}"),
        }
    }
    Ok(Json(json!({ "done": done, "imported": imported_total })))
}

/// `DELETE /api/pluggy/items/{id}` — delete the local item (cascades to
/// accounts) and the Pluggy item.
pub async fn delete(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let local = fetch_local_item(&state, id).await?;
    if let Some(client) = state.pluggy.client() {
        // Best-effort: the item may already be gone on Pluggy's side.
        let _ = client.delete_item(&local.pluggy_id).await;
    }
    sqlx::query("DELETE FROM pluggy_items WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- helpers ----------

async fn fetch_local_item(state: &AppState, id: Uuid) -> Result<LocalPluggyItem, AppError> {
    sqlx::query_as(
        "SELECT id, pluggy_id, connector_id, connector_name, status, error,
                last_sync_at, created_at
         FROM pluggy_items WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("pluggy item not found"))
}

fn pluggy_item_json(local_id: Uuid, item: &pluggy::PluggyItem) -> Value {
    json!({
        "id": local_id,
        "pluggy_id": item.id,
        "status": item.status,
        "execution_status": item.execution_status,
        "connector_name": item.connector.as_ref().map(|c| c.name.clone()),
        "error": item.error,
        "status_detail": item.status_detail,
        "last_updated_at": item.last_updated_at,
    })
}

/// For OAuth connectors waiting for input, the authorize URL to open.
fn oauth_url(item: &pluggy::PluggyItem) -> Option<String> {
    if item.status != "WAITING_USER_INPUT" {
        return None;
    }
    item.parameter
        .as_ref()
        .and_then(|p| p.get("data").and_then(|d| d.as_str()))
        .map(|s| s.to_string())
}

/// Trigger a sync on one item and poll until terminal (up to ~60s). Returns
/// the final status and the number of newly imported items.
async fn sync_one(
    state: &AppState,
    client: &PluggyClient,
    local: &LocalPluggyItem,
) -> Result<(String, usize), AppError> {
    let mut current = client.update_item(&local.pluggy_id, None).await?;
    pluggy::upsert_pluggy_item(&state.pool, &current).await?;

    for _ in 0..30 {
        match current.status.as_str() {
            "UPDATED" | "LOGIN_DONE" => {
                let imported = finish_import(state, client, local).await?;
                return Ok((current.status.clone(), imported));
            }
            "ERROR" | "PARTIAL_SUCCESS" => {
                return Ok((current.status.clone(), 0));
            }
            _ => {}
        }
        sleep(Duration::from_secs(2)).await;
        current = match client.get_item(&local.pluggy_id).await {
            Ok(i) => {
                pluggy::upsert_pluggy_item(&state.pool, &i).await?;
                i
            }
            Err(_) => break,
        };
    }
    Ok((current.status.clone(), 0))
}

/// Fetch accounts + transactions for an item and import them.
async fn finish_import(
    state: &AppState,
    client: &PluggyClient,
    local: &LocalPluggyItem,
) -> Result<usize, AppError> {
    let accounts = client.list_accounts(&local.pluggy_id).await?;
    pluggy::upsert_accounts(&state.pool, local.id, &accounts).await?;
    let imported = pluggy::import_item_transactions(&state.pool, client, local).await?;
    sqlx::query("UPDATE pluggy_items SET last_sync_at = now(), status = 'UPDATED' WHERE id = $1")
        .bind(local.id)
        .execute(&state.pool)
        .await?;
    Ok(imported)
}
