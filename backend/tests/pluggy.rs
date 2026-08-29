//! Pluggy integration tests: config-driven account sync against a
//! wiremock-faked Pluggy API + a real Postgres (`#[sqlx::test]`).

use deepsave_backend::config::PluggyAccountConf;
use deepsave_backend::services::pluggy::{
    seed_configured_accounts, sync_all_accounts, PluggyClient,
};
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
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

fn conf(id: &str, bank: &str, kind: &str, name: &str) -> PluggyAccountConf {
    PluggyAccountConf {
        id: id.into(),
        bank: bank.into(),
        kind: kind.into(),
        name: name.into(),
    }
}

#[sqlx::test]
async fn imports_new_items_and_is_idempotent(pool: sqlx::PgPool) {
    common::migrate(&pool).await;
    let mock = MockServer::start().await;
    let client = seed_client(&mock).await;

    // One checking account, one credit card.
    let confs = vec![
        conf("acc-checking", "nubank", "BANK", "Nubank — Conta"),
        conf("acc-card", "nubank", "CREDIT", "Nubank — Cartão"),
    ];
    seed_configured_accounts(&pool, &confs).await.unwrap();

    // Transactions: checking (boleto + salary) and card (charge + skipped payment).
    Mock::given(method("GET"))
        .and(path("/v2/transactions"))
        .and(query_param("accountId", "acc-checking"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "id": "tx-celesc", "description": "Pagamento efetuado|CELESC DISTRIBUICAO S.A",
                  "amount": -82.93, "type": "DEBIT", "date": "2026-07-06T12:00:00.000Z",
                  "status": "POSTED", "category": "Electricity" },
                { "id": "tx-salary", "description": "SALARIO EMPRESA XYZ LTDA",
                  "amount": 8500, "type": "CREDIT", "date": "2026-07-05T12:00:00.000Z",
                  "status": "POSTED", "category": "Income" }
            ],
            "next": null
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/transactions"))
        .and(query_param("accountId", "acc-card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "id": "tx-netflix", "description": "NETFLIX.COM", "amount": 55.9, "type": "DEBIT",
                  "date": "2026-07-08T12:00:00.000Z", "status": "POSTED",
                  "creditCardMetadata": { "installmentNumber": 2, "totalInstallments": 6 } },
                { "id": "tx-cardpay", "description": "Pagamento fatura", "amount": -1500, "type": "CREDIT",
                  "date": "2026-07-09T12:00:00.000Z", "status": "POSTED" }
            ],
            "next": null
        })))
        .mount(&mock)
        .await;

    // First sync: 3 new (celesc, salary, netflix; card payment is skipped).
    let results = sync_all_accounts(&pool, &client, None, None).await.unwrap();
    let total_new: usize = results.iter().map(|r| r.new).sum();
    assert_eq!(total_new, 3);

    // Mapping sanity: income vs expense, installments, source.
    let rows: Vec<(String, String, i64, Option<i32>, Option<i32>)> = sqlx::query_as(
        "SELECT description, kind, amount_cents, installment, installment_count
         FROM items ORDER BY occurred_on",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, "SALARIO EMPRESA XYZ LTDA");
    assert_eq!(rows[0].1, "income");
    assert_eq!(rows[0].2, 850_000);
    assert_eq!(rows[1].0, "Pagamento efetuado|CELESC DISTRIBUICAO S.A");
    assert_eq!(rows[1].1, "expense");
    assert_eq!(rows[1].2, -8_293);
    assert_eq!(rows[2].0, "NETFLIX.COM");
    assert_eq!(rows[2].1, "expense");
    assert_eq!(rows[2].2, -5_590);
    assert_eq!(rows[2].3, Some(2));
    assert_eq!(rows[2].4, Some(6));

    // Re-sync is idempotent (external_id unique index).
    let results2 = sync_all_accounts(&pool, &client, None, None).await.unwrap();
    assert_eq!(results2.iter().map(|r| r.new).sum::<usize>(), 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
}

#[sqlx::test]
async fn accounts_are_seeded_and_linked(pool: sqlx::PgPool) {
    common::migrate(&pool).await;
    let confs = vec![
        conf("acc-1", "caixa", "BANK", "Caixa — Conta"),
        conf("acc-2", "caixa", "CREDIT", "Caixa — Cartão"),
    ];
    let n = seed_configured_accounts(&pool, &confs).await.unwrap();
    assert_eq!(n, 2);

    let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT pluggy_account_id, bank, account_type FROM pluggy_accounts ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains(&("acc-1".into(), Some("caixa".into()), Some("BANK".into()))));
    assert!(rows.contains(&("acc-2".into(), Some("caixa".into()), Some("CREDIT".into()))));

    // Each account linked to an `accounts` row.
    let linked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pluggy_accounts pa JOIN accounts a ON a.id = pa.account_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(linked, 2);

    // Re-seeding is idempotent (still 2 accounts rows).
    seed_configured_accounts(&pool, &confs).await.unwrap();
    let accounts_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts_rows, 2);
}

#[sqlx::test]
async fn assigns_installments_to_purchase_series(pool: sqlx::PgPool) {
    common::migrate(&pool).await;
    use deepsave_backend::services::pluggy::assign_installment_series;

    // Two parcels of one 10x purchase + one parcel of another.
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents, installment, installment_count)
         VALUES ('pluggy', 'expense', 'confirmed', '2026-03-16', 'GMAD MADVILLE MADEIRAS JOINVILLE BRA', -20832, 1, 10)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents, installment, installment_count)
         VALUES ('pluggy', 'expense', 'confirmed', '2026-04-15', 'GMAD MADVILLE MADEIRAS JOINVILLE BRA', -20832, 2, 10)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO items (source, kind, status, occurred_on, description, amount_cents, installment, installment_count)
         VALUES ('pluggy', 'expense', 'confirmed', '2026-05-15', 'AIRBNB * HM8XS2HWQA SAO PAULO', -10937, 2, 6)",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(assign_installment_series(&pool).await.unwrap(), 3);

    // Both GMAD parcels share one series; AIRBNB gets its own.
    let (series_count, gmads, airbnb): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM purchase_series),
                (SELECT count(DISTINCT series_id) FROM items WHERE description ILIKE '%GMAD%'),
                (SELECT count(DISTINCT series_id) FROM items WHERE description ILIKE '%AIRBNB%')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(series_count, 2);
    assert_eq!(gmads, 1);
    assert_eq!(airbnb, 1);

    // Idempotent: a re-run assigns nothing new.
    assert_eq!(assign_installment_series(&pool).await.unwrap(), 0);
}
