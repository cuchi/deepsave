//! Pluggy integration tests: full import pipeline against a wiremock-faked
//! Pluggy API + a real Postgres (`#[sqlx::test]`).

use deepsave_backend::services::pluggy::{
    import_item_transactions, upsert_accounts, upsert_pluggy_item, PluggyClient,
};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;

async fn seed_client(mock: &MockServer) -> PluggyClient {
    // Auth: any POST /auth → apiKey.
    Mock::given(method("POST"))
        .and(path("/auth"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "apiKey": "test-key" })))
        .mount(mock)
        .await;
    PluggyClient::new_with_base("cid".into(), "csecret".into(), mock.uri())
}

#[sqlx::test]
async fn imports_transactions_and_is_idempotent(pool: sqlx::PgPool) {
    common::migrate(&pool).await;
    let mock = MockServer::start().await;
    let client = seed_client(&mock).await;

    // The item: connector + status.
    let item = serde_json::from_value(json!({
        "id": "item-1",
        "status": "UPDATED",
        "executionStatus": "SUCCESS",
        "connector": { "id": 200, "name": "MeuPluggy", "country": "BR", "type": "PERSONAL_BANK" }
    }))
    .unwrap();
    let local_item_id = upsert_pluggy_item(&pool, &item).await.unwrap();

    // Accounts: one checking + one credit card.
    Mock::given(method("GET"))
        .and(path("/accounts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                {
                    "id": "acc-checking",
                    "type": "BANK",
                    "subtype": "CHECKING_ACCOUNT",
                    "marketingName": "GOLD Conta Corrente",
                    "currencyCode": "BRL",
                    "balance": 21544.6
                },
                {
                    "id": "acc-card",
                    "type": "CREDIT",
                    "subtype": "CREDIT_CARD",
                    "marketingName": "BLACK Cartão",
                    "currencyCode": "BRL",
                    "creditData": {
                        "balanceCloseDate": "2026-07-23",
                        "balanceDueDate": "2026-07-28",
                        "creditLimit": 300000
                    }
                }
            ],
            "page": 1, "total": 2, "totalPages": 1
        })))
        .mount(&mock)
        .await;

    let accounts = client.list_accounts("item-1").await.unwrap();
    assert_eq!(accounts.len(), 2);
    upsert_accounts(&pool, local_item_id, &accounts).await.unwrap();

    let local_item = sqlx::query_as::<_, deepsave_backend::services::pluggy::LocalPluggyItem>(
        "SELECT id, pluggy_id, connector_id, connector_name, status, error, last_sync_at, created_at
         FROM pluggy_items WHERE id = $1",
    )
    .bind(local_item_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Transactions per account (one page each, cursor-terminated).
    Mock::given(method("GET"))
        .and(path("/transactions"))
        .and(wiremock::matchers::query_param("accountId", "acc-checking"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "id": "tx-salary",  "description": "SALARIO EMPRESA XYZ LTDA", "amount": 8500, "type": "CREDIT",
                  "date": "2026-07-05T12:00:00.000Z", "status": "POSTED", "category": "Salary" },
                { "id": "tx-boleto",  "description": "Pagamento de boleto", "amount": -100, "type": "DEBIT",
                  "date": "2026-07-06T12:00:00.000Z", "status": "POSTED", "category": "Transfer - Bank Slip" },
                { "id": "tx-pix",     "description": "Pix enviado para Paulo", "amount": -350.5, "type": "DEBIT",
                  "date": "2026-07-07T12:00:00.000Z", "status": "POSTED" }
            ],
            "page": 1, "total": 3, "totalPages": 1, "next": null
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/transactions"))
        .and(wiremock::matchers::query_param("accountId", "acc-card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "id": "tx-netflix", "description": "NETFLIX.COM", "amount": 55.9, "type": "DEBIT",
                  "date": "2026-07-08T12:00:00.000Z", "status": "POSTED",
                  "creditCardMetadata": { "installmentNumber": 2, "totalInstallments": 6, "totalAmount": -335.4 } },
                { "id": "tx-cardpay", "description": "Pagamento fatura", "amount": -1500, "type": "CREDIT",
                  "date": "2026-07-09T12:00:00.000Z", "status": "POSTED" }
            ],
            "page": 1, "total": 2, "totalPages": 1, "next": null
        })))
        .mount(&mock)
        .await;

    // First import → 4 items (card payment is skipped).
    let imported = import_item_transactions(&pool, &client, &local_item)
        .await
        .unwrap();
    assert_eq!(imported, 4);

    let rows: Vec<(String, String, i64, String, Option<i32>, Option<i32>)> = sqlx::query_as(
        "SELECT description, kind, amount_cents, source, installment, installment_count
         FROM items ORDER BY occurred_on",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 4);

    // Salary income.
    assert_eq!(rows[0].0, "SALARIO EMPRESA XYZ LTDA");
    assert_eq!(rows[0].1, "income");
    assert_eq!(rows[0].2, 850_000);
    // Boleto expense.
    assert_eq!(rows[1].1, "expense");
    assert_eq!(rows[1].2, -10_000);
    // Pix expense.
    assert_eq!(rows[2].2, -35_050);
    // Card charge with installments.
    assert_eq!(rows[3].0, "NETFLIX.COM");
    assert_eq!(rows[3].1, "expense");
    assert_eq!(rows[3].2, -5_590);
    assert_eq!(rows[3].3, "pluggy");
    assert_eq!(rows[3].4, Some(2));
    assert_eq!(rows[3].5, Some(6));

    // Re-sync must be idempotent (same external_ids → nothing new).
    let imported_again = import_item_transactions(&pool, &client, &local_item)
        .await
        .unwrap();
    assert_eq!(imported_again, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 4);
}

#[sqlx::test]
async fn account_links_to_local_accounts(pool: sqlx::PgPool) {
    common::migrate(&pool).await;
    let mock = MockServer::start().await;
    let client = seed_client(&mock).await;

    let item = serde_json::from_value(json!({
        "id": "item-2", "status": "UPDATED",
        "connector": { "id": 200, "name": "MeuPluggy", "country": "BR", "type": "PERSONAL_BANK" }
    }))
    .unwrap();
    let local_item_id = upsert_pluggy_item(&pool, &item).await.unwrap();

    Mock::given(method("GET"))
        .and(path("/accounts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "id": "acc-1", "type": "BANK", "subtype": "CHECKING_ACCOUNT",
                "marketingName": "Conta", "currencyCode": "BRL", "balance": 100.5
            }],
            "page": 1, "total": 1, "totalPages": 1
        })))
        .mount(&mock)
        .await;

    let accounts = client.list_accounts("item-2").await.unwrap();
    upsert_accounts(&pool, local_item_id, &accounts).await.unwrap();

    // A matching `accounts` row was created and linked.
    let linked: (uuid::Uuid, uuid::Uuid) = sqlx::query_as(
        "SELECT pa.account_id, a.id FROM pluggy_accounts pa
         JOIN accounts a ON a.id = pa.account_id WHERE pa.pluggy_account_id = 'acc-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(linked.0, linked.1);

    // Upserting the same account again keeps one local accounts row.
    upsert_accounts(&pool, local_item_id, &accounts).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
