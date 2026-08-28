//! Pluggy (open-banking / account aggregation) client + import logic.
//!
//! Flow: create an item (bank connection) → Pluggy syncs it → we pull the
//! accounts + transactions and import them as `confirmed` items, keyed by
//! Pluggy's transaction id (`items.external_id`) so re-syncs are idempotent.
//!
//! Sign conventions (from Pluggy docs):
//! - BANK accounts: amount > 0 = inflow (CREDIT), amount < 0 = outflow (DEBIT).
//! - CREDIT accounts: amount > 0 = charge (expense), amount < 0 = payment/refund.
//!   We import card charges as expenses and skip card-side payments/refunds
//!   (the payment already shows up on the bank-account side).

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use uuid::Uuid;

use crate::services::{recurring, tags};

const API_BASE: &str = "https://api.pluggy.ai";
const PAGE_SIZE: i64 = 500;

// ---------- API types (Pluggy returns camelCase JSON) ----------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    pub id: i32,
    pub name: String,
    pub country: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub oauth: Option<bool>,
    pub has_mfa: Option<bool>,
    pub image_url: Option<String>,
    pub products: Option<Vec<String>>,
    pub is_open_finance: Option<bool>,
    pub credentials: Option<Vec<Credential>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub name: String,
    pub label: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub optional: Option<bool>,
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorPage {
    pub results: Vec<Connector>,
    pub page: i64,
    pub total: i64,
    pub total_pages: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluggyItem {
    pub id: String,
    pub status: String,
    pub execution_status: Option<String>,
    pub connector: Option<Connector>,
    pub error: Option<serde_json::Value>,
    pub parameter: Option<serde_json::Value>,
    pub status_detail: Option<String>,
    pub last_updated_at: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateItemRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    pub connector_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_user_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub subtype: Option<String>,
    pub name: Option<String>,
    pub marketing_name: Option<String>,
    pub number: Option<String>,
    pub balance: Option<f64>,
    pub currency_code: Option<String>,
    pub credit_data: Option<CreditData>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditData {
    pub brand: Option<String>,
    pub level: Option<String>,
    pub balance_close_date: Option<String>,
    pub balance_due_date: Option<String>,
    pub credit_limit: Option<f64>,
    pub available_credit_limit: Option<f64>,
    pub minimum_payment: Option<f64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPage {
    pub results: Vec<Account>,
    pub page: i64,
    pub total: i64,
    pub total_pages: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub id: String,
    pub description: Option<String>,
    pub description_raw: Option<String>,
    pub amount: Option<f64>,
    pub date: Option<String>,
    pub currency_code: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>, // DEBIT | CREDIT
    pub status: Option<String>, // POSTED | PENDING
    pub category: Option<String>,
    pub category_id: Option<String>,
    pub merchant: Option<Merchant>,
    pub payment_data: Option<PaymentData>,
    pub credit_card_metadata: Option<CreditCardMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Merchant {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentData {
    pub payment_method: Option<String>,
    pub receiver: Option<Participant>,
    pub payer: Option<Participant>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditCardMetadata {
    pub installment_number: Option<i64>,
    pub total_installments: Option<i64>,
    pub total_amount: Option<f64>,
    pub bill_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPage {
    pub results: Vec<Transaction>,
    pub page: i64,
    pub total: i64,
    /// Relative query string for the next page (e.g. `?accountId=…&after=…`).
    pub next: Option<String>,
}

// ---------- Client ----------

#[derive(Clone)]
pub struct PluggyClient {
    http: reqwest::Client,
    base_url: String,
    /// Fixed API key (`PLUGGY_API_KEY`) — used directly, never refreshed.
    fixed_key: Option<String>,
    /// Client credentials for the `/auth` flow (fallback / auto-refresh).
    client_id: Option<String>,
    client_secret: Option<String>,
    token: Arc<Mutex<Option<(String, Instant)>>>,
}

use std::sync::Arc;

impl PluggyClient {
    /// API-key mode: the key is used for every request as-is (single-user setups).
    pub fn from_api_key(api_key: String) -> Self {
        Self::from_api_key_with_base(api_key, API_BASE.to_string())
    }

    pub fn from_api_key_with_base(api_key: String, base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            fixed_key: Some(api_key),
            client_id: None,
            client_secret: None,
            token: Arc::new(Mutex::new(None)),
        }
    }

    /// Client-credentials mode: fetches a JWT from `/auth` (cached, refreshed).
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self::new_with_base(client_id, client_secret, API_BASE.to_string())
    }

    /// Test hook: point at a wiremock server instead of the live API.
    pub fn new_with_base(client_id: String, client_secret: String, base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            fixed_key: None,
            client_id: Some(client_id),
            client_secret: Some(client_secret),
            token: Arc::new(Mutex::new(None)),
        }
    }

    /// The key sent as `X-API-KEY`. In API-key mode this is the configured key;
    /// in credentials mode it's a JWT from `/auth`, cached for ~90 minutes.
    async fn api_key(&self) -> Result<String> {
        if let Some(key) = &self.fixed_key {
            return Ok(key.clone());
        }
        let (Some(client_id), Some(client_secret)) = (&self.client_id, &self.client_secret) else {
            anyhow::bail!("Pluggy not configured (no API key or client credentials)");
        };
        let cached = self.token.lock().unwrap().clone();
        if let Some((key, at)) = cached
            && at.elapsed() < Duration::seconds(5400).to_std().unwrap()
        {
            return Ok(key);
        }
        let resp: serde_json::Value = self
            .http
            .post(format!("{}/auth", self.base_url))
            .json(&serde_json::json!({
                "clientId": client_id,
                "clientSecret": client_secret,
            }))
            .send()
            .await
            .context("pluggy auth request failed")?
            .error_for_status()
            .context("pluggy auth rejected")?
            .json()
            .await
            .context("pluggy auth response parse failed")?;
        let key = resp
            .get("apiKey")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow!("pluggy auth response missing apiKey"))?
            .to_string();
        *self.token.lock().unwrap() = Some((key.clone(), Instant::now()));
        Ok(key)
    }

    /// True when this client can authenticate (fixed key or client credentials).
    pub fn is_configured(&self) -> bool {
        self.fixed_key.is_some() || (self.client_id.is_some() && self.client_secret.is_some())
    }

    pub fn uses_fixed_key(&self) -> bool {
        self.fixed_key.is_some()
    }

    /// On 401 with client credentials available, drop the cached JWT (it may
    /// have expired) and retry once. A fixed API key can't be refreshed — the
    /// error is surfaced as-is so the user regenerates it.
    async fn send_with_retry(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let key = self.api_key().await?;
        let resp = req
            .try_clone()
            .expect("request body must be cloneable")
            .header("X-API-KEY", &key)
            .send()
            .await
            .context("pluggy request failed")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            && self.fixed_key.is_none()
            && self.client_id.is_some()
        {
            self.token.lock().unwrap().take();
            let key = self.api_key().await?;
            return req
                .header("X-API-KEY", &key)
                .send()
                .await
                .context("pluggy retry failed");
        }
        Ok(resp)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .send_with_retry(self.http.get(format!("{}{}", self.base_url, path)))
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("pluggy GET {path} → {status}: {body}"));
        }
        resp.json().await.context("pluggy GET parse failed")
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let resp = self
            .send_with_retry(self.http.post(format!("{}{}", self.base_url, path)).json(body))
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("pluggy POST {path} → {status}: {body}"));
        }
        resp.json().await.context("pluggy POST parse failed")
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let resp = self
            .send_with_retry(self.http.delete(format!("{}{}", self.base_url, path)))
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("pluggy DELETE {path} → {status}: {body}"));
        }
        Ok(())
    }

    pub async fn list_connectors(&self, page_size: i64) -> Result<Vec<Connector>> {
        let mut page = 1;
        let mut out = Vec::new();
        loop {
            let p: ConnectorPage = self
                .get(&format!("/connectors?page={page}&pageSize={page_size}"))
                .await?;
            let is_empty = p.results.is_empty();
            let total_pages = p.total_pages.unwrap_or(1).max(1);
            out.extend(p.results);
            if page >= total_pages || is_empty {
                break;
            }
            page += 1;
        }
        Ok(out)
    }

    pub async fn create_item(&self, req: &CreateItemRequest) -> Result<PluggyItem> {
        self.post(
            "/items",
            &serde_json::to_value(req).context("serialize item request")?,
        )
        .await
    }

    pub async fn get_item(&self, pluggy_id: &str) -> Result<PluggyItem> {
        self.get(&format!("/items/{pluggy_id}")).await
    }

    /// Trigger a sync (or send credentials/MFA) for an existing item.
    pub async fn update_item(
        &self,
        pluggy_id: &str,
        parameters: Option<serde_json::Value>,
    ) -> Result<PluggyItem> {
        let body = match parameters {
            Some(p) => serde_json::json!({ "parameters": p }),
            None => serde_json::json!({}),
        };
        self.post(&format!("/items/{pluggy_id}"), &body).await
    }

    pub async fn delete_item(&self, pluggy_id: &str) -> Result<()> {
        self.delete(&format!("/items/{pluggy_id}")).await
    }

    pub async fn list_accounts(&self, pluggy_item_id: &str) -> Result<Vec<Account>> {
        let mut page = 1;
        let mut out = Vec::new();
        loop {
            let p: AccountPage = self
                .get(&format!(
                    "/accounts?itemId={pluggy_item_id}&page={page}&pageSize=200"
                ))
                .await?;
            let is_empty = p.results.is_empty();
            let total_pages = p.total_pages.unwrap_or(1).max(1);
            out.extend(p.results);
            if page >= total_pages || is_empty {
                break;
            }
            page += 1;
        }
        Ok(out)
    }

    /// All transactions for an account, following the cursor.
    pub async fn list_transactions(&self, account_id: &str) -> Result<Vec<Transaction>> {
        let mut out = Vec::new();
        let mut next: Option<String> = Some(format!("/transactions?accountId={account_id}&pageSize={PAGE_SIZE}"));
        while let Some(path) = next {
            let p: TransactionPage = self.get(&path).await?;
            let is_empty = p.results.is_empty();
            let has_next = p.next.is_some();
            out.extend(p.results);
            next = p.next.map(|n| {
                if n.starts_with('/') {
                    n
                } else if n.starts_with('?') {
                    format!("/transactions{n}")
                } else {
                    format!("/transactions?{n}")
                }
            });
            if !has_next || is_empty {
                break;
            }
        }
        Ok(out)
    }
}

// ---------- Local persistence + import ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LocalPluggyItem {
    pub id: Uuid,
    pub pluggy_id: String,
    pub connector_id: Option<i32>,
    pub connector_name: Option<String>,
    pub status: String,
    pub error: Option<serde_json::Value>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LocalPluggyAccount {
    pub id: Uuid,
    pub pluggy_account_id: String,
    pub pluggy_item_id: Uuid,
    pub account_id: Option<Uuid>,
    pub name: String,
    pub account_type: Option<String>,
    pub subtype: Option<String>,
    pub currency: String,
    pub balance: Option<f64>,
    pub credit_limit: Option<f64>,
    pub due_date: Option<NaiveDate>,
    pub close_date: Option<NaiveDate>,
    pub last_sync_at: Option<DateTime<Utc>>,
}

/// Save a Pluggy item locally (upsert by `pluggy_id`).
pub async fn upsert_pluggy_item(pool: &PgPool, item: &PluggyItem) -> Result<Uuid> {
    let connector = item.connector.as_ref();
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO pluggy_items (pluggy_id, connector_id, connector_name, status, error)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (pluggy_id) DO UPDATE SET
           connector_id = EXCLUDED.connector_id,
           connector_name = EXCLUDED.connector_name,
           status = EXCLUDED.status,
           error = EXCLUDED.error
         RETURNING id",
    )
    .bind(&item.id)
    .bind(connector.map(|c| c.id))
    .bind(connector.map(|c| c.name.clone()))
    .bind(&item.status)
    .bind(item.error.clone())
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Sync accounts for an item into `pluggy_accounts` (upsert by pluggy id),
/// creating matching `accounts` rows (for item filtering) on first sight.
pub async fn upsert_accounts(pool: &PgPool, local_item_id: Uuid, accounts: &[Account]) -> Result<()> {
    for a in accounts {
        let name = a
            .marketing_name
            .clone()
            .or_else(|| a.name.clone())
            .unwrap_or_else(|| "Conta".to_string());
        let account_type = a.kind.clone().unwrap_or_default();
        let subtype = a.subtype.clone().unwrap_or_default();
        let currency = a.currency_code.clone().unwrap_or_else(|| "BRL".to_string());
        let (close_date, due_date) = match &a.credit_data {
            Some(cd) => (
                cd.balance_close_date.as_deref().and_then(parse_iso_date),
                cd.balance_due_date.as_deref().and_then(parse_iso_date),
            ),
            None => (None, None),
        };

        // Find/create the local account (keyed by pluggy account id).
        let account_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT account_id FROM pluggy_accounts WHERE pluggy_account_id = $1",
        )
        .bind(&a.id)
        .fetch_optional(pool)
        .await?;
        let account_id = match account_id {
            Some(id) => Some(id),
            None => {
                let (id,): (Uuid,) = sqlx::query_as(
                    "INSERT INTO accounts (name, bank) VALUES ($1, $2) RETURNING id",
                )
                .bind(&name)
                .bind(&account_type)
                .fetch_one(pool)
                .await?;
                Some(id)
            }
        };

        sqlx::query(
            "INSERT INTO pluggy_accounts
               (pluggy_account_id, pluggy_item_id, account_id, name, account_type, subtype,
                currency, balance, credit_limit, due_date, close_date, last_sync_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())
             ON CONFLICT (pluggy_account_id) DO UPDATE SET
               pluggy_item_id = EXCLUDED.pluggy_item_id,
               account_id = EXCLUDED.account_id,
               name = EXCLUDED.name,
               account_type = EXCLUDED.account_type,
               subtype = EXCLUDED.subtype,
               currency = EXCLUDED.currency,
               balance = EXCLUDED.balance,
               credit_limit = EXCLUDED.credit_limit,
               due_date = EXCLUDED.due_date,
               close_date = EXCLUDED.close_date,
               last_sync_at = now()",
        )
        .bind(&a.id)
        .bind(local_item_id)
        .bind(account_id)
        .bind(&name)
        .bind(&account_type)
        .bind(&subtype)
        .bind(&currency)
        .bind(a.balance)
        .bind(a.credit_data.as_ref().and_then(|c| c.credit_limit))
        .bind(due_date)
        .bind(close_date)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Import all transactions of an item into `items` (confirmed), idempotent via
/// `items.external_id` (Pluggy's transaction id). Returns the number of new items.
pub async fn import_item_transactions(
    pool: &PgPool,
    client: &PluggyClient,
    local_item: &LocalPluggyItem,
) -> Result<usize> {
    let accounts: Vec<LocalPluggyAccount> = sqlx::query_as(
        "SELECT id, pluggy_account_id, pluggy_item_id, account_id, name, account_type,
                subtype, currency, balance::float8 AS balance, credit_limit::float8 AS credit_limit,
                due_date, close_date, last_sync_at
         FROM pluggy_accounts WHERE pluggy_item_id = $1",
    )
    .bind(local_item.id)
    .fetch_all(pool)
    .await?;

    let categories = load_categories(pool).await?;
    let mut imported = 0usize;

    for acc in &accounts {
        let txs = client.list_transactions(&acc.pluggy_account_id).await?;
        for tx in txs {
            let Some(mapped) = map_transaction(&tx, acc, &categories) else {
                continue;
            };
            // RETURNING only yields a row for actually-inserted items, so a
            // duplicate (same external_id) skips the recurring-rule linking.
            let inserted: Option<(Uuid,)> = sqlx::query_as(
                "INSERT INTO items
                   (parent_id, document_id, source, kind, status, account_id,
                    installment, installment_count, occurred_on, merchant, description,
                    amount_cents, currency, category_id, tags, raw_line, external_id)
                 VALUES (NULL, NULL, 'pluggy', $1, 'confirmed', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                 ON CONFLICT (external_id) WHERE external_id IS NOT NULL DO NOTHING
                 RETURNING id",
            )
            .bind(&mapped.kind)
            .bind(acc.account_id)
            .bind(mapped.installment)
            .bind(mapped.installment_count)
            .bind(mapped.occurred_on)
            .bind(&mapped.merchant)
            .bind(&mapped.description)
            .bind(mapped.amount_cents)
            .bind(&acc.currency)
            .bind(mapped.category_id)
            .bind(&mapped.tags)
            .bind(&mapped.raw_line)
            .bind(&tx.id)
            .fetch_optional(pool)
            .await?;
            if let Some((item_id,)) = inserted {
                imported += 1;
                recurring::link_item(pool, item_id).await?;
            }
        }
    }
    Ok(imported)
}

// ---------- transaction → item mapping ----------

struct MappedItem {
    kind: String,
    installment: Option<i32>,
    installment_count: Option<i32>,
    occurred_on: NaiveDate,
    merchant: Option<String>,
    description: String,
    amount_cents: i64,
    category_id: Option<Uuid>,
    tags: Vec<String>,
    raw_line: Option<String>,
}

/// Map one Pluggy transaction to an item insert. Returns `None` for rows we
/// intentionally skip (card-side payments/refunds — already seen on the bank side).
fn map_transaction(
    tx: &Transaction,
    acc: &LocalPluggyAccount,
    categories: &[(Uuid, String)],
) -> Option<MappedItem> {
    let is_card = acc.account_type.as_deref() == Some("CREDIT");

    let (amount, kind) = match tx.kind.as_deref() {
        Some("DEBIT") => {
            if is_card {
                // card charge → expense (Pluggy amount is positive for charges).
                (-cents(tx.amount?).abs(), "expense".to_string())
            } else {
                (-cents(tx.amount?).abs(), "expense".to_string())
            }
        }
        Some("CREDIT") => {
            if is_card {
                // card payment/refund → skip (already on the bank side).
                return None;
            }
            (cents(tx.amount?), "income".to_string())
        }
        _ => return None,
    };

    let description = tx
        .description
        .clone()
        .or_else(|| tx.description_raw.clone())
        .or_else(|| {
            tx.merchant
                .as_ref()
                .and_then(|m| m.name.clone())
        })
        .or_else(|| {
            tx.payment_data
                .as_ref()
                .and_then(|p| p.receiver.as_ref().and_then(|r| r.name.clone()))
        })
        .unwrap_or_else(|| "Transação".to_string());

    let merchant = tx
        .merchant
        .as_ref()
        .and_then(|m| m.name.clone())
        .or_else(|| {
            tx.payment_data
                .as_ref()
                .and_then(|p| p.receiver.as_ref().and_then(|r| r.name.clone()))
        });

    let (installment, installment_count) = match &tx.credit_card_metadata {
        Some(cc) if cc.total_installments.unwrap_or(1) > 1 => (
            cc.installment_number.map(|n| n as i32),
            cc.total_installments.map(|n| n as i32),
        ),
        _ => (None, None),
    };

    let category_id = tx
        .category
        .as_deref()
        .and_then(|c| match_category(categories, c));

    let occurred_on = tx
        .date
        .as_deref()
        .and_then(parse_iso_datetime)
        .unwrap_or_else(|| Utc::now().date_naive());

    Some(MappedItem {
        kind,
        installment,
        installment_count,
        occurred_on,
        merchant,
        description: description.trim().to_string(),
        amount_cents: amount,
        category_id,
        tags: Vec::new(),
        raw_line: tx
            .category
            .as_ref()
            .map(|c| format!("pluggy category: {c}")),
    })
}

fn cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

/// Parse an ISO-8601 timestamp into a BR (-3) calendar date.
fn parse_iso_datetime(s: &str) -> Option<NaiveDate> {
    let dt = DateTime::parse_from_rfc3339(s).ok()?;
    let br = chrono::FixedOffset::west_opt(3 * 3600)?;
    Some(dt.with_timezone(&br).date_naive())
}

fn parse_iso_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

async fn load_categories(pool: &PgPool) -> Result<Vec<(Uuid, String)>> {
    Ok(sqlx::query_as("SELECT id, name FROM categories WHERE is_active")
        .fetch_all(pool)
        .await?)
}

/// Best-effort match of a Pluggy category name to our seeded categories.
fn match_category(categories: &[(Uuid, String)], pluggy_category: &str) -> Option<Uuid> {
    let target = alias_category(&tags::strip_accents(pluggy_category.trim()).to_lowercase());
    if target.is_empty() {
        return None;
    }
    categories
        .iter()
        .find(|(_, n)| tags::strip_accents(n).to_lowercase() == target)
        .or_else(|| {
            categories.iter().find(|(_, n)| {
                let nl = tags::strip_accents(n).to_lowercase();
                target.contains(&nl) || nl.contains(&target)
            })
        })
        .map(|(id, _)| *id)
}

/// Map Pluggy category names (en-US taxonomy) to our canonical names.
fn alias_category(s: &str) -> String {
    let table: HashMap<&str, &str> = [
        ("supermarkets", "supermercado"),
        ("groceries", "supermercado"),
        ("restaurants", "restaurantes"),
        ("dining", "restaurantes"),
        ("food", "restaurantes"),
        ("transportation", "transporte"),
        ("transport", "transporte"),
        ("fuel", "transporte"),
        ("gas", "transporte"),
        ("health", "saude"),
        ("pharmacy", "saude"),
        ("medical", "saude"),
        ("housing", "moradia"),
        ("rent", "moradia"),
        ("utilities", "moradia"),
        ("entertainment", "lazer"),
        ("leisure", "lazer"),
        ("streaming", "assinaturas"),
        ("subscriptions", "assinaturas"),
        ("salary", "salario"),
        ("insurance", "saude"),
        ("education", "educacao"),
        ("travel", "lazer"),
        ("shopping", "compras"),
        ("clothing", "compras"),
        ("transfers", "transferencias"),
        ("other", "outros"),
    ]
    .iter()
    .map(|(k, v)| (*k, *v))
    .collect();

    match table.get(s) {
        Some(v) => (*v).to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn account(kind: &str) -> LocalPluggyAccount {
        LocalPluggyAccount {
            id: Uuid::new_v4(),
            pluggy_account_id: "acc-1".into(),
            pluggy_item_id: Uuid::new_v4(),
            account_id: None,
            name: "Conta".into(),
            account_type: Some(kind.to_string()),
            subtype: None,
            currency: "BRL".into(),
            balance: None,
            credit_limit: None,
            due_date: None,
            close_date: None,
            last_sync_at: None,
        }
    }

    fn tx(amount: f64, kind: &str, extra: serde_json::Value) -> Transaction {
        serde_json::from_value(serde_json::json!({
            "id": "tx-1",
            "description": "SALARIO EMPRESA XYZ LTDA",
            "amount": amount,
            "date": "2026-07-10T12:00:00.000Z", // noon UTC → same BR day
            "currencyCode": "BRL",
            "type": kind,
            "status": "POSTED",
        }).merge(&extra))
        .unwrap()
    }

    trait Merge {
        fn merge(self, other: &serde_json::Value) -> serde_json::Value;
    }
    impl Merge for serde_json::Value {
        fn merge(self, other: &serde_json::Value) -> serde_json::Value {
            let mut a = self;
            if let (serde_json::Value::Object(m), serde_json::Value::Object(o)) = (&mut a, other) {
                for (k, v) in o {
                    m.insert(k.clone(), v.clone());
                }
            }
            a
        }
    }

    #[test]
    fn bank_debit_is_expense_negative() {
        let t = tx(-100.0, "DEBIT", json!({}));
        let m = map_transaction(&t, &account("BANK"), &[]).unwrap();
        assert_eq!(m.kind, "expense");
        assert_eq!(m.amount_cents, -10_000);
        assert_eq!(m.occurred_on.to_string(), "2026-07-10");
    }

    #[test]
    fn bank_credit_is_income_positive() {
        let t = tx(8500.0, "CREDIT", json!({}));
        let m = map_transaction(&t, &account("BANK"), &[]).unwrap();
        assert_eq!(m.kind, "income");
        assert_eq!(m.amount_cents, 850_000);
    }

    #[test]
    fn card_charge_is_expense() {
        // Credit card: Pluggy amount is positive for charges.
        let t = tx(55.9, "DEBIT", json!({}));
        let m = map_transaction(&t, &account("CREDIT"), &[]).unwrap();
        assert_eq!(m.kind, "expense");
        assert_eq!(m.amount_cents, -5_590);
    }

    #[test]
    fn card_payment_is_skipped() {
        // Card-side bill payment (negative, CREDIT) — already on the bank side.
        let t = tx(-1500.0, "CREDIT", json!({}));
        assert!(map_transaction(&t, &account("CREDIT"), &[]).is_none());
    }

    #[test]
    fn installment_metadata_maps() {
        let t = tx(335.4, "DEBIT", json!({
            "creditCardMetadata": {
                "installmentNumber": 2,
                "totalInstallments": 6,
                "totalAmount": -335.4,
                "billId": "bill-1"
            }
        }));
        let m = map_transaction(&t, &account("CREDIT"), &[]).unwrap();
        assert_eq!(m.installment, Some(2));
        assert_eq!(m.installment_count, Some(6));
    }

    #[test]
    fn single_installment_has_no_installment_fields() {
        let t = tx(19.9, "DEBIT", json!({
            "creditCardMetadata": { "installmentNumber": 1, "totalInstallments": 1 }
        }));
        let m = map_transaction(&t, &account("CREDIT"), &[]).unwrap();
        assert_eq!(m.installment, None);
        assert_eq!(m.installment_count, None);
    }

    #[test]
    fn br_timezone_shifts_near_midnight() {
        // 00:30 UTC on the 10th is still the 9th in BR (-3).
        let t = tx(-10.0, "DEBIT", json!({ "date": "2026-07-10T00:30:00.000Z" }));
        let m = map_transaction(&t, &account("BANK"), &[]).unwrap();
        assert_eq!(m.occurred_on.to_string(), "2026-07-09");
    }

    #[test]
    fn category_mapping() {
        let categories = vec![
            (Uuid::new_v4(), "Supermercado".to_string()),
            (Uuid::new_v4(), "Restaurantes".to_string()),
        ];
        let t = tx(-20.0, "DEBIT", json!({ "category": "Supermarkets" }));
        let m = map_transaction(&t, &account("BANK"), &categories).unwrap();
        assert_eq!(m.category_id, Some(categories[0].0));
    }

    #[test]
    fn cents_rounds() {
        assert_eq!(cents(1500.0), 150_000);
        assert_eq!(cents(0.1), 10);
        assert_eq!(cents(-0.05), -5);
        assert_eq!(cents(55.9), 5_590);
    }
}
