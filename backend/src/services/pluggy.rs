//! Pluggy (open-banking / account aggregation) client + import logic.
//!
//! Accounts are **config-driven**: the user reads their account ids once from
//! the Pluggy dashboard (they are stable) and lists them in `.env`
//! (`PLUGGY_ACCOUNTS`, a JSON array). No items API is needed.
//!
//! Import is **adopt-and-dedupe**: for each Pluggy transaction we first try to
//! match an existing document-sourced item (same |amount|, ±2 days, shared
//! descriptive tokens). On a unique match we reuse that row — just stamping
//! `items.external_id` with Pluggy's transaction id — so the user's curated
//! tags/category/recurring links are preserved and nothing is duplicated.
//! Unmatched transactions are inserted as `confirmed` items (`source='pluggy'`).
//! Re-syncs are idempotent via `items.external_id` (partial unique index).
//!
//! Sign conventions (from Pluggy docs + live data):
//! - BANK accounts: amount > 0 = inflow (CREDIT), amount < 0 = outflow (DEBIT).
//! - CREDIT accounts: amount > 0 = charge (expense), amount < 0 = payment/refund.
//!   Card charges import as expenses; card-side payments/refunds are skipped
//!   (the payment already shows on the bank side as an internal move).
//! - Bank-side "Credit card payment" transactions are imported as `internal`
//!   (they are the fatura payment — the real expense is on the card side).

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

use crate::config::PluggyAccountConf;
use crate::services::{recurring, tags};

const API_BASE: &str = "https://api.pluggy.ai";

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
    /// Amount in the account's currency for international (FX) transactions
    /// (e.g. USD charge with the BRL equivalent here).
    pub amount_in_account_currency: Option<f64>,
    pub date: Option<String>,
    pub currency_code: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>, // DEBIT | CREDIT
    pub status: Option<String>, // POSTED | PENDING
    pub category: Option<String>,
    pub category_id: Option<String>,
    /// Open Finance movement type (PIX, BOLETO, PORTABILIDADE_SALARIO, …).
    pub operation_type: Option<String>,
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
    /// Merchant Category Code — a strong category signal. (Pluggy sends `payeeMCC`.)
    #[serde(rename = "payeeMCC")]
    pub payee_mcc: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPage {
    pub results: Vec<Transaction>,
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
    async fn send_with_retry(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response> {
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

    /// All transactions for an account (v2, cursor pagination, 500/page).
    /// `date_from`/`date_to` (transaction date, inclusive) narrow the pull —
    /// incremental syncs fetch only what's newer than the last import.
    pub async fn list_transactions(
        &self,
        account_id: &str,
        date_from: Option<NaiveDate>,
        date_to: Option<NaiveDate>,
    ) -> Result<Vec<Transaction>> {
        let mut query = format!("/v2/transactions?accountId={account_id}");
        if let Some(d) = date_from {
            query.push_str(&format!("&dateFrom={d}"));
        }
        if let Some(d) = date_to {
            query.push_str(&format!("&dateTo={d}"));
        }
        let mut out = Vec::new();
        let mut next: Option<String> = Some(query);
        while let Some(path) = next {
            let p: TransactionPage = self.get(&path).await?;
            let is_empty = p.results.is_empty();
            let has_next = p.next.is_some();
            out.extend(p.results);
            next = p.next.map(|n| {
                if n.starts_with('/') {
                    n
                } else if n.starts_with('?') {
                    format!("/v2/transactions{n}")
                } else {
                    format!("/v2/transactions?{n}")
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
pub struct LocalPluggyAccount {
    pub id: Uuid,
    pub pluggy_account_id: String,
    pub pluggy_item_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub name: String,
    pub account_type: Option<String>,
    pub bank: Option<String>,
    pub subtype: Option<String>,
    pub currency: String,
    pub balance: Option<f64>,
    pub credit_limit: Option<f64>,
    pub due_date: Option<NaiveDate>,
    pub close_date: Option<NaiveDate>,
    pub last_sync_at: Option<DateTime<Utc>>,
}

/// Upsert the accounts configured in `.env` into `pluggy_accounts`, creating
/// matching `accounts` rows (for item filtering) on first sight. Returns the
/// number of accounts configured.
pub async fn seed_configured_accounts(pool: &PgPool, accounts: &[PluggyAccountConf]) -> Result<usize> {
    for a in accounts {
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
                .bind(&a.name)
                .bind(&a.kind)
                .fetch_one(pool)
                .await?;
                Some(id)
            }
        };

        sqlx::query(
            "INSERT INTO pluggy_accounts
               (pluggy_account_id, account_id, name, account_type, bank, currency)
             VALUES ($1, $2, $3, $4, $5, 'BRL')
             ON CONFLICT (pluggy_account_id) DO UPDATE SET
               account_id = EXCLUDED.account_id,
               name = EXCLUDED.name,
               account_type = EXCLUDED.account_type,
               bank = EXCLUDED.bank",
        )
        .bind(&a.id)
        .bind(account_id)
        .bind(&a.name)
        .bind(&a.kind)
        .bind(&a.bank)
        .execute(pool)
        .await?;
    }
    Ok(accounts.len())
}

/// Result of syncing one account.
#[derive(Debug, Clone, Serialize)]
pub struct AccountSyncResult {
    pub pluggy_account_id: String,
    pub name: String,
    /// Newly inserted items.
    pub new: usize,
}

/// Import transactions for every configured account. Returns per-account
/// results. Idempotent: items carry `external_id` (Pluggy tx id).
///
/// By default the pull is **incremental**: only transactions posted after the
/// account's last imported date (minus a small overlap for late-posted
/// charges) are fetched. Pass `forced_from`/`forced_to` to re-pull a custom
/// period (full history when both are `None` and the account is empty).
pub async fn sync_all_accounts(
    pool: &PgPool,
    client: &PluggyClient,
    forced_from: Option<NaiveDate>,
    forced_to: Option<NaiveDate>,
) -> Result<Vec<AccountSyncResult>> {
    let accounts: Vec<LocalPluggyAccount> = sqlx::query_as(
        "SELECT id, pluggy_account_id, pluggy_item_id, account_id, name, account_type, bank,
                subtype, currency, balance::float8 AS balance, credit_limit::float8 AS credit_limit,
                due_date, close_date, last_sync_at
         FROM pluggy_accounts ORDER BY bank, name",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(accounts.len());
    for acc in accounts {
        // Incremental default: from = last imported tx date − 3 days (catches
        // charges posted a few days after the cutoff). Empty account → full pull.
        let from = match forced_from {
            Some(f) => Some(f),
            None => last_import_date(pool, &acc)
                .await?
                .map(|d| d - chrono::Duration::days(3)),
        };
        let new = import_account_transactions(pool, client, &acc, from, forced_to).await?;
        out.push(AccountSyncResult {
            pluggy_account_id: acc.pluggy_account_id.clone(),
            name: acc.name,
            new,
        });
    }
    Ok(out)
}

/// Latest transaction date already imported for an account, if any.
async fn last_import_date(pool: &PgPool, acc: &LocalPluggyAccount) -> Result<Option<NaiveDate>> {
    let Some(account_id) = acc.account_id else {
        return Ok(None);
    };
    let d: Option<NaiveDate> =
        sqlx::query_scalar("SELECT max(occurred_on) FROM items WHERE account_id = $1")
            .bind(account_id)
            .fetch_one(pool)
            .await?;
    Ok(d)
}

/// Import one account's transactions into `items` (confirmed), idempotent via
/// `items.external_id`. `date_from`/`date_to` bound the pull (incremental by
/// default). Returns the number of newly inserted items.
pub async fn import_account_transactions(
    pool: &PgPool,
    client: &PluggyClient,
    acc: &LocalPluggyAccount,
    date_from: Option<NaiveDate>,
    date_to: Option<NaiveDate>,
) -> Result<usize> {
    let categories = load_categories(pool).await?;
    let txs = client.list_transactions(&acc.pluggy_account_id, date_from, date_to).await?;

    let mut new = 0usize;
    for tx in txs {
        let Some(mapped) = map_transaction(&tx, acc, &categories) else {
            continue;
        };

        // INSERT with ON CONFLICT (external_id) — only new transactions count
        // as new; existing rows just refresh the enrichment metadata (stable
        // Pluggy fields, never user data).
        let inserted: Option<(Uuid, bool)> = sqlx::query_as(
            "INSERT INTO items
               (parent_id, document_id, source, kind, status, account_id,
                installment, installment_count, occurred_on, merchant, description,
                amount_cents, currency, category_id, tags, raw_line, external_id,
                pluggy_category, mcc, operation_type, payment_method)
             VALUES (NULL, NULL, 'pluggy', $1, 'confirmed', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                     $14, $15, $16, $17)
             ON CONFLICT (external_id) WHERE external_id IS NOT NULL DO UPDATE SET
               pluggy_category = EXCLUDED.pluggy_category,
               mcc = EXCLUDED.mcc,
               operation_type = EXCLUDED.operation_type,
               payment_method = EXCLUDED.payment_method
             RETURNING id, (xmax = 0) AS inserted",
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
        .bind(&mapped.pluggy_category)
        .bind(mapped.mcc)
        .bind(&mapped.operation_type)
        .bind(&mapped.payment_method)
        .fetch_optional(pool)
        .await?;
        if let Some((item_id, is_new)) = inserted {
            if is_new {
                new += 1;
                recurring::link_item(pool, item_id).await?;
            }
        }
    }

    sqlx::query("UPDATE pluggy_accounts SET last_sync_at = now() WHERE id = $1")
        .bind(acc.id)
        .execute(pool)
        .await?;
    Ok(new)
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
    pluggy_category: Option<String>,
    mcc: Option<i32>,
    operation_type: Option<String>,
    payment_method: Option<String>,
}

/// Map one Pluggy transaction to an item. Returns `None` for rows we skip
/// (card-side payments/refunds — already seen on the bank side).
fn map_transaction(
    tx: &Transaction,
    acc: &LocalPluggyAccount,
    categories: &[(Uuid, String)],
) -> Option<MappedItem> {
    let is_card = acc.account_type.as_deref() == Some("CREDIT");
    let category = tx.category.as_deref().unwrap_or("");

    // International (FX) charges carry the BRL value in `amountInAccountCurrency`
    // (e.g. US$ 15.00 → R$ 80.08); `amount` alone would be in the foreign currency.
    let raw_amount = tx.amount_in_account_currency.or(tx.amount)?;
    let description = tx
        .description
        .clone()
        .or_else(|| tx.description_raw.clone())
        .or_else(|| tx.merchant.as_ref().and_then(|m| m.name.clone()))
        .or_else(|| {
            tx.payment_data
                .as_ref()
                .and_then(|p| p.receiver.as_ref().and_then(|r| r.name.clone()))
        })
        .unwrap_or_else(|| "Transação".to_string());
    let (amount, kind) = match tx.kind.as_deref() {
        Some("DEBIT") => {
            if is_card {
                // card charge → expense (Pluggy amount is positive for charges).
                (-cents(raw_amount).abs(), "expense".to_string())
            } else if category == "Credit card payment" {
                // bank-side fatura payment → internal move (expense is on the card).
                (-cents(raw_amount).abs(), "internal".to_string())
            } else {
                (-cents(raw_amount).abs(), "expense".to_string())
            }
        }
        Some("CREDIT") => {
            if is_card {
                // Card payments (fatura) are CREDIT + negative — skip, they're
                // already visible on the bank side. Card **refunds/estornos**
                // are also CREDIT + negative but carry refund keywords; import
                // those as refunds (positive, money back).
                if is_card_refund(&description) {
                    (cents(raw_amount).abs(), "refund".to_string())
                } else {
                    return None;
                }
            } else {
                (cents(raw_amount), "income".to_string())
            }
        }
        _ => return None,
    };

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
        pluggy_category: tx.category.clone(),
        mcc: tx.credit_card_metadata.as_ref().and_then(|c| c.payee_mcc.map(|m| m as i32)),
        operation_type: tx.operation_type.clone(),
        payment_method: tx
            .payment_data
            .as_ref()
            .and_then(|p| p.payment_method.clone()),
    })
}

fn cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

/// A card-side CREDIT that is a refund/estorno (money back) rather than a
/// fatura payment. Pluggy has no explicit flag — the description keywords are
/// the signal ("Estorno Tarifa…", "Refund", "IOF de volta de…").
fn is_card_refund(description: &str) -> bool {
    let d = description.to_lowercase();
    ["estorno", "storno", "refund", "reembolso", "de volta", "cashback", "chargeback"]
        .iter()
        .any(|k| d.contains(k))
}

/// Parse an ISO-8601 timestamp into a BR (-3) calendar date.
fn parse_iso_datetime(s: &str) -> Option<NaiveDate> {
    let dt = DateTime::parse_from_rfc3339(s).ok()?;
    let br = chrono::FixedOffset::west_opt(3 * 3600)?;
    Some(dt.with_timezone(&br).date_naive())
}

async fn load_categories(pool: &PgPool) -> Result<Vec<(Uuid, String)>> {
    Ok(sqlx::query_as("SELECT id, name FROM categories WHERE is_active")
        .fetch_all(pool)
        .await?)
}

// ---------- description tokens (shared by refund linking) ----------

/// Words that carry no discriminating power between two descriptions.
const STOPWORDS: &[&str] = &[
    "transferencia", "enviada", "enviado", "recebida", "recebido", "pix", "pelo", "pagamento",
    "boleto", "efetuado", "tarifa", "estorno", "compra", "debito", "credito", "saque", "ted",
    "doc", "fatura", "para", "via", "no", "na", "em", "do", "da", "dos", "das", "de", "s", "a",
    "e", "o", "ao", "os", "as", "uma", "um", "dia", "valor", "conta", "agencia", "banco", "sao",
    "ltd", "ltda", "s.a", "sa", "ip", "envio", "fat", "total", "brl", "real", "reais", "rua",
    "vila", "cep", "ref", "av", "num", "carta", "cred", "saq", "realizado", "realizada",
    "lancado", "lancamento", "solicitado", "deposito",
];

/// Normalized, deduped, stopword-filtered tokens of a description. Keeps words
/// of >= 3 chars plus short all-numeric tokens ("4" in "PISTA 4").
fn desc_tokens(s: &str) -> std::collections::HashSet<String> {
    let n = tags::strip_accents(s).to_lowercase();
    n.split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|t| {
            (t.len() >= 3)
                || (t.len() <= 2 && t.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter(|t| !STOPWORDS.contains(t))
        .map(str::to_string)
        .collect()
}

/// Two tokens are considered the same descriptor when equal, one is a prefix of
/// the other ("aplic" ≈ "aplicacao"), or one contains the other ("salario" ⊂
/// "TEDSALARIO").
fn token_matches(a: &str, b: &str) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a) || a.contains(b) || b.contains(a)
}

/// Number of candidate tokens sharing a descriptor with the transaction tokens.
fn token_score(candidate: &str, tx_tokens: &std::collections::HashSet<String>) -> usize {
    desc_tokens(candidate)
        .iter()
        .filter(|t| tx_tokens.iter().any(|x| token_matches(t, x)))
        .count()
}

/// Canonical series identity for a description (sorted token set) — stable
/// across minor formatting differences ("DI FATTO" vs "DIFATTO").
fn series_key(description: &str) -> String {
    let mut toks: Vec<String> = desc_tokens(description).into_iter().collect();
    toks.sort();
    toks.join(" ")
}

/// Assign installment items (Pluggy: `installment_count > 1`, no `series_id`)
/// to `purchase_series` rows keyed by (account, merchant tokens, count) so the
/// forecast/expected/upcoming queries can project future parcels. The first
/// parcel seen creates the series; later parcels of the same purchase join it.
/// Idempotent — only touches unassigned items.
pub async fn assign_installment_series(pool: &PgPool) -> Result<usize> {
    // Index existing series by (account_id, token key, count).
    let existing: Vec<(Uuid, Option<Uuid>, String, i32)> = sqlx::query_as(
        "SELECT id, account_id, description, installment_count FROM purchase_series",
    )
    .fetch_all(pool)
    .await?;
    let mut by_key: std::collections::HashMap<(Option<Uuid>, String, i32), Uuid> =
        std::collections::HashMap::new();
    for (id, account_id, description, count) in existing {
        by_key
            .entry((account_id, series_key(&description), count))
            .or_insert(id);
    }

    let items: Vec<(Uuid, Option<Uuid>, String, i32)> = sqlx::query_as(
        "SELECT id, account_id, description, installment_count FROM items
         WHERE installment_count > 1 AND series_id IS NULL",
    )
    .fetch_all(pool)
    .await?;

    let mut assigned = 0usize;
    for (item_id, account_id, description, count) in items {
        let key = (account_id, series_key(&description), count);
        let series_id = match by_key.get(&key) {
            Some(id) => *id,
            None => {
                let (id,): (Uuid,) = sqlx::query_as(
                    "INSERT INTO purchase_series (account_id, description, installment_count)
                     VALUES ($1, $2, $3) RETURNING id",
                )
                .bind(account_id)
                .bind(&description)
                .bind(count)
                .fetch_one(pool)
                .await?;
                by_key.insert(key, id);
                id
            }
        };
        sqlx::query("UPDATE items SET series_id = $1 WHERE id = $2")
            .bind(series_id)
            .bind(item_id)
            .execute(pool)
            .await?;
        assigned += 1;
    }
    Ok(assigned)
}

/// Link each refund to the charge it reverses (`items.refunded_item_id`), so
/// the graphs can net them. Kind-agnostic (the counterpart may be `expense` or
/// `internal`). Two passes, greedy & unique:
///
/// - **Pass 1 (exact)**: equal |amount| + shared merchant tokens within ±45
///   days; best by (token score, nearest date).
/// - **Pass 2 (partial)**: |charge| >= |refund| + shared tokens + charge
///   before refund within ±45 days; nearest date.
/// Returns the number of newly linked refunds.
pub async fn link_refunds(pool: &PgPool) -> Result<usize> {
    let refunds: Vec<(Uuid, i64, NaiveDate, String, Option<String>)> = sqlx::query_as(
        "SELECT id, amount_cents, occurred_on, description, merchant FROM items
         WHERE kind = 'refund' AND refunded_item_id IS NULL",
    )
    .fetch_all(pool)
    .await?;

    let mut linked = 0usize;
    for (rid, amount, date, description, merchant) in refunds {
        let amount = amount.abs();
        let mut tokens = desc_tokens(&description);
        if let Some(m) = &merchant {
            tokens.extend(desc_tokens(m));
        }
        let Some(cid) = find_refund_charge(pool, amount, date, &tokens).await? else {
            continue;
        };
        let affected = sqlx::query(
            "UPDATE items SET refunded_item_id = $1, updated_at = now() WHERE id = $2",
        )
        .bind(cid)
        .bind(rid)
        .execute(pool)
        .await?
        .rows_affected();
        linked += affected as usize;
    }
    Ok(linked)
}

/// Find the unique best charge for a refund: pass 1 exact |amount|, then pass 2
/// partial (|charge| >= |refund|). Both within ±45 days, merchant-token scored,
/// nearest-date tie-break; `None` when absent or ambiguous.
async fn find_refund_charge(
    pool: &PgPool,
    amount: i64,
    date: NaiveDate,
    tokens: &std::collections::HashSet<String>,
) -> Result<Option<Uuid>> {
    // Pass 1: exact |amount|.
    let exact: Vec<(Uuid, String, Option<String>, NaiveDate)> = sqlx::query_as(
        "SELECT id, description, merchant, occurred_on FROM items
         WHERE kind IN ('expense', 'internal')
           AND amount_cents = -$1
           AND occurred_on BETWEEN ($2::date - 45) AND ($2::date + 45)",
    )
    .bind(amount)
    .bind(date)
    .fetch_all(pool)
    .await?;
    if let Some(id) = best_charge(exact, date, tokens) {
        return Ok(Some(id));
    }

    // Pass 2: partial refund — |charge| >= |refund|, charge posted first.
    let partial: Vec<(Uuid, String, Option<String>, NaiveDate)> = sqlx::query_as(
        "SELECT id, description, merchant, occurred_on FROM items
         WHERE kind IN ('expense', 'internal')
           AND amount_cents <= -$1
           AND occurred_on BETWEEN ($2::date - 45) AND $2::date",
    )
    .bind(amount)
    .bind(date)
    .fetch_all(pool)
    .await?;
    Ok(best_charge(partial, date, tokens))
}

/// Score candidate charges by shared merchant tokens (description + merchant
/// field), pick the unique best — tie-broken by nearest date. Requires a token
/// overlap; `None` when no candidate scores or the best is tied.
fn best_charge(
    candidates: Vec<(Uuid, String, Option<String>, NaiveDate)>,
    refund_date: NaiveDate,
    tokens: &std::collections::HashSet<String>,
) -> Option<Uuid> {
    let mut best: Option<(usize, i64, Uuid)> = None;
    let mut ties = 0usize;
    for (id, desc, merchant, cdate) in candidates {
        let mut score = token_score(&desc, tokens);
        if let Some(m) = &merchant {
            score = score.max(token_score(m, tokens));
        }
        if score == 0 {
            continue;
        }
        let dist = (refund_date - cdate).num_days().abs();
        let cand = (score, dist, id);
        match &best {
            Some(b) if b.0 == cand.0 && b.1 == cand.1 => ties += 1,
            Some(b) if b.0 > cand.0 || (b.0 == cand.0 && b.1 < cand.1) => {}
            _ => {
                best = Some(cand);
                ties = 1;
            }
        }
    }
    match best {
        Some((score, _, id)) if ties == 1 && score > 0 => Some(id),
        _ => None,
    }
}

// ---------- category mapping ----------

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

/// Map Pluggy's category taxonomy to our canonical (accent-stripped) names.
/// Unknown categories fall through unchanged (best-effort substring match).
fn alias_category(s: &str) -> String {
    let table: &[(&str, &str)] = &[
        // Supermercado
        ("groceries", "supermercado"),
        // Restaurantes
        ("food delivery", "restaurantes"),
        ("eating out", "restaurantes"),
        ("food and drinks", "restaurantes"),
        // Transporte
        ("parking", "transporte"),
        ("gas stations", "transporte"),
        ("taxi and ride-hailing", "transporte"),
        ("car rental", "transporte"),
        ("tolls and in vehicle payment", "transporte"),
        ("bicycle", "transporte"),
        ("automotive", "transporte"),
        ("vehicle maintenance", "transporte"),
        ("transportation", "transporte"),
        // Saúde
        ("healthcare", "saude"),
        ("pharmacy", "saude"),
        ("insurance", "saude"),
        ("wellness and fitness", "saude"),
        ("gyms and fitness centers", "saude"),
        // Moradia
        ("rent", "moradia"),
        ("housing", "moradia"),
        ("real estate financing", "moradia"),
        ("electricity", "moradia"),
        ("water", "moradia"),
        ("telecommunications", "moradia"),
        // Lazer
        ("accomodation", "lazer"),
        ("travel", "lazer"),
        ("leisure", "lazer"),
        ("gambling", "lazer"),
        ("gaming", "lazer"),
        ("tickets", "lazer"),
        // Assinaturas
        ("video streaming", "assinaturas"),
        ("digital services", "assinaturas"),
        // Outros (everything else stays unmapped → Outros fallback below)
        ("shopping", "compras"),
        ("clothing", "compras"),
        ("houseware", "compras"),
        ("electronics", "compras"),
        ("sports goods", "compras"),
        ("bookstore", "compras"),
        ("kids and toys", "compras"),
        ("services", "servicos"),
        ("pet supplies and vet", "pets"),
        ("investments", "investimentos"),
        ("fixed income", "investimentos"),
        ("automatic investment", "investimentos"),
        ("pension", "investimentos"),
        ("cashback", "cashback"),
        ("income", "renda"),
        ("salary", "salario"),
        ("account fees", "taxas"),
        ("bank fees", "taxas"),
        ("credit card fees", "taxas"),
        ("tax on financial operations", "taxas"),
        ("interests charged", "taxas"),
        ("income taxes", "taxas"),
        ("loans", "emprestimos"),
    ];
    for (k, v) in table {
        if *k == s {
            return (*v).to_string();
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn account(kind: &str) -> LocalPluggyAccount {
        LocalPluggyAccount {
            id: Uuid::new_v4(),
            pluggy_account_id: "acc-1".into(),
            pluggy_item_id: None,
            account_id: None,
            name: "Conta".into(),
            account_type: Some(kind.to_string()),
            bank: None,
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
        let t = tx(55.9, "DEBIT", json!({}));
        let m = map_transaction(&t, &account("CREDIT"), &[]).unwrap();
        assert_eq!(m.kind, "expense");
        assert_eq!(m.amount_cents, -5_590);
    }

    #[test]
    fn card_payment_is_skipped() {
        let t = tx(-1500.0, "CREDIT", json!({ "description": "Pagamento de fatura" }));
        assert!(map_transaction(&t, &account("CREDIT"), &[]).is_none());
    }

    #[test]
    fn card_refund_is_imported_as_positive_refund() {
        let t = tx(-98.0, "CREDIT", json!({ "description": "Estorno Tarifa Anuidade Diferenciada" }));
        let m = map_transaction(&t, &account("CREDIT"), &[]).unwrap();
        assert_eq!(m.kind, "refund");
        assert_eq!(m.amount_cents, 9_800);

        let t = tx(-14.22, "CREDIT", json!({ "description": "IOF de volta de Quotationy.Com" }));
        let m = map_transaction(&t, &account("CREDIT"), &[]).unwrap();
        assert_eq!(m.kind, "refund");
        assert_eq!(m.amount_cents, 1_422);
    }

    #[test]
    fn bank_card_payment_is_internal() {
        let t = tx(-1500.0, "DEBIT", json!({ "category": "Credit card payment" }));
        let m = map_transaction(&t, &account("BANK"), &[]).unwrap();
        assert_eq!(m.kind, "internal");
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
        let t = tx(-10.0, "DEBIT", json!({ "date": "2026-07-10T00:30:00.000Z" }));
        let m = map_transaction(&t, &account("BANK"), &[]).unwrap();
        assert_eq!(m.occurred_on.to_string(), "2026-07-09");
    }

    #[test]
    fn category_mapping() {
        let categories = vec![
            (Uuid::new_v4(), "Supermercado".to_string()),
            (Uuid::new_v4(), "Restaurantes".to_string()),
            (Uuid::new_v4(), "Transporte".to_string()),
        ];
        let t = tx(-20.0, "DEBIT", json!({ "category": "Groceries" }));
        let m = map_transaction(&t, &account("BANK"), &categories).unwrap();
        assert_eq!(m.category_id, Some(categories[0].0));

        let t = tx(-20.0, "DEBIT", json!({ "category": "Eating out" }));
        let m = map_transaction(&t, &account("BANK"), &categories).unwrap();
        assert_eq!(m.category_id, Some(categories[1].0));

        let t = tx(-20.0, "DEBIT", json!({ "category": "Gas stations" }));
        let m = map_transaction(&t, &account("BANK"), &categories).unwrap();
        assert_eq!(m.category_id, Some(categories[2].0));
    }

    #[test]
    fn cents_rounds() {
        assert_eq!(cents(1500.0), 150_000);
        assert_eq!(cents(0.1), 10);
        assert_eq!(cents(-0.05), -5);
        assert_eq!(cents(55.9), 5_590);
    }

    #[test]
    fn alias_category_known() {
        assert_eq!(alias_category("food delivery"), "restaurantes");
        assert_eq!(alias_category("gas stations"), "transporte");
        assert_eq!(alias_category("video streaming"), "assinaturas");
        assert_eq!(alias_category("credit card payment"), "credit card payment"); // left unmapped
    }
}
