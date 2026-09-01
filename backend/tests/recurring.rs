//! Tests for the recurring rules revamp: date math, name-entry validation,
//! auto-linking (aliases) and one-shot linking (isolated cases), manual-link safety.

use chrono::NaiveDate;
use deepsave_backend::services::recurring;
use sqlx::PgPool;
use uuid::Uuid;

mod common;

async fn seed_item(
    pool: &PgPool,
    merchant: Option<&str>,
    description: &str,
    amount_cents: i64,
    occurred_on: &str,
) -> Uuid {
    let date = NaiveDate::parse_from_str(occurred_on, "%Y-%m-%d").unwrap();
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO items (source, kind, status, occurred_on, merchant, description, amount_cents)
         VALUES ('manual', 'expense', 'confirmed', $1, $2, $3, $4) RETURNING id",
    )
    .bind(date)
    .bind(merchant)
    .bind(description)
    .bind(amount_cents)
    .fetch_one(pool)
    .await
    .unwrap();
    row.0
}

async fn seed_rule(pool: &PgPool, name: &str) -> Uuid {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO recurring_rules (name, amount_cents, frequency, interval, is_active)
         VALUES ($1, -100, 'monthly', 1, true) RETURNING id",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .unwrap();
    row.0
}

async fn link_of(pool: &PgPool, item_id: Uuid) -> (Option<Uuid>, bool) {
    sqlx::query_as("SELECT recurring_id, linked_manually FROM items WHERE id = $1")
        .bind(item_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

// ---------- pure date math ----------

#[test]
fn advance_next_due_never_past() {
    let today = NaiveDate::from_ymd_opt(2025, 6, 15).unwrap();
    let cases = [
        ("weekly", 1, "2025-06-10"),  // past → next weekly
        ("weekly", 1, "2025-06-14"),  // yesterday
        ("monthly", 1, "2025-01-31"), // month-end clamping + advance
        ("yearly", 1, "2024-03-01"),
        ("monthly", 2, "2025-05-20"),
    ];
    for (freq, interval, next) in cases {
        let next = NaiveDate::parse_from_str(next, "%Y-%m-%d").unwrap();
        let out = recurring::advance_next_due(next, freq, interval, today);
        assert!(
            out >= today,
            "{freq}/{interval} from {next} -> {out} is in the past"
        );
    }
    // Future dates stay put.
    let future = NaiveDate::from_ymd_opt(2025, 7, 1).unwrap();
    assert_eq!(recurring::advance_next_due(future, "monthly", 1, today), future);
}

// ---------- validation ----------

#[sqlx::test]
async fn validate_entries_checks_existence_and_uniqueness(pool: PgPool) {
    common::migrate(&pool).await;
    seed_item(&pool, Some("NETFLIX"), "assinatura netflix", -3990, "2025-05-10").await;
    // merchant-less item: description is the fallback match target.
    seed_item(&pool, None, "PAGAMENTO VIA PIX", -5000, "2025-05-11").await;
    let rule = seed_rule(&pool, "Streaming").await;
    let other = seed_rule(&pool, "Outra").await;

    // Alias must exist in the data (merchant or description, normalized).
    let errs = recurring::validate_entries(&pool, &["inexistente".into()], &[], None)
        .await
        .unwrap();
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("não existe"));

    // Both merchant names and descriptions pass.
    let errs = recurring::validate_entries(&pool, &["netflix".into(), "PAGAMENTO VIA PIX".into()], &[], None)
        .await
        .unwrap();
    assert!(errs.is_empty(), "{errs:?}");

    // Aliases are globally unique across rules.
    sqlx::query("INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, 'netflix', true)")
        .bind(rule)
        .execute(&pool)
        .await
        .unwrap();
    let errs = recurring::validate_entries(&pool, &["NETFLIX".into()], &[], None)
        .await
        .unwrap();
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("já usado"));

    // …but the owning rule is allowed to keep it (update path).
    let errs = recurring::validate_entries(&pool, &["netflix".into()], &[], Some(rule))
        .await
        .unwrap();
    assert!(errs.is_empty(), "{errs:?}");

    // Isolated cases repeat across rules without conflict.
    let errs = recurring::validate_entries(&pool, &[], &["netflix".into()], Some(other))
        .await
        .unwrap();
    assert!(errs.is_empty(), "{errs:?}");
}

// ---------- amount tolerance (varying payments) ----------

#[test]
fn matches_alias_tolerates_trailing_amount() {
    use deepsave_backend::services::recurring::matches_alias;

    // Exact normalized equality.
    assert!(matches_alias("ALUGUEL", "aluguel"));

    // Amount embedded in the name — varying mortgage/utility bills.
    assert!(matches_alias("ALUGUEL 184732", "aluguel"));
    assert!(matches_alias("ALUGUEL 1.847,32", "aluguel"));
    assert!(matches_alias("aluguel r$ 1847,32", "aluguel"));
    assert!(matches_alias("CELESC DISTRIBUICAO S.A - 0829", "celesc distribuicao s.a"));

    // A longer real word is NOT a match (remainder must be an amount).
    assert!(!matches_alias("ALUGUEL TERRENO", "aluguel"));
    // Substring in the middle is not a match (prefix only).
    assert!(!matches_alias("MEU ALUGUEL", "aluguel"));
    // Non-amount suffixes still don't match.
    assert!(!matches_alias("NETFLIX BR", "netflix"));
    assert!(!matches_alias("LOJA CENTRO", "loja"));
}

#[sqlx::test]
async fn alias_with_amount_tolerance_links_varying_payments(pool: PgPool) {
    common::migrate(&pool).await;
    let rule = seed_rule(&pool, "Financiamento").await;
    sqlx::query(
        "INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, 'aluguel', true)",
    )
    .bind(rule)
    .execute(&pool)
    .await
    .unwrap();

    // Same alias, three months, different amounts embedded in the merchant name.
    let a = seed_item(&pool, Some("ALUGUEL 184732"), "financiamento", -184732, "2025-05-10").await;
    let b = seed_item(&pool, Some("ALUGUEL 183210"), "financiamento", -183210, "2025-06-10").await;
    let c = seed_item(&pool, Some("ALUGUEL 181900"), "financiamento", -181900, "2025-07-10").await;
    for id in [a, b, c] {
        recurring::link_item(&pool, id).await.unwrap();
        assert_eq!(link_of(&pool, id).await, (Some(rule), false));
    }
}

#[sqlx::test]
async fn validate_and_relink_accept_amount_tolerant_alias(pool: PgPool) {
    common::migrate(&pool).await;
    seed_item(&pool, Some("ALUGUEL 184732"), "financiamento", -184732, "2025-05-10").await;

    // Creating the alias passes validation even though no item matches exactly.
    let errs = recurring::validate_entries(&pool, &["aluguel".into()], &[], None)
        .await
        .unwrap();
    assert!(errs.is_empty(), "{errs:?}");

    // …and relink picks up items whose name embeds a different amount.
    let rule = seed_rule(&pool, "Financiamento").await;
    sqlx::query(
        "INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, 'aluguel', true)",
    )
    .bind(rule)
    .execute(&pool)
    .await
    .unwrap();
    let item = seed_item(&pool, Some("ALUGUEL 183210"), "financiamento", -183210, "2025-06-10").await;
    recurring::relink_rule(&pool, rule).await.unwrap();
    assert_eq!(link_of(&pool, item).await, (Some(rule), false));
}

#[sqlx::test]
async fn merchants_autocomplete_includes_description_fallback(pool: PgPool) {
    common::migrate(&pool).await;
    // Merchant present → merchant is the suggestion.
    seed_item(&pool, Some("CELESC DISTRIBUICAO S.A"), "Pagamento de boleto", -8293, "2025-05-10").await;
    // Merchant-less bill → description is the suggestion ("ALUGUEL").
    seed_item(&pool, None, "ALUGUEL", -484596, "2025-05-14").await;
    seed_item(&pool, None, "ALUGUEL", -484693, "2025-06-14").await;

    let names = deepsave_backend::routes::recurring::merchant_names(&pool, "aluguel").await.unwrap();
    assert_eq!(names, vec!["ALUGUEL"]);

    let names = deepsave_backend::routes::recurring::merchant_names(&pool, "celesc").await.unwrap();
    assert_eq!(names, vec!["CELESC DISTRIBUICAO S.A"]);

    // Empty query lists everything (usable on focus).
    let names = deepsave_backend::routes::recurring::merchant_names(&pool, "").await.unwrap();
    assert!(names.contains(&"ALUGUEL".to_string()));
    assert!(names.contains(&"CELESC DISTRIBUICAO S.A".to_string()));
}

// ---------- auto linking (aliases) ----------

#[sqlx::test]
async fn link_item_auto_links_exact_normalized_match(pool: PgPool) {
    common::migrate(&pool).await;
    let rule = seed_rule(&pool, "Streaming").await;
    sqlx::query("INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, 'netflix', true)")
        .bind(rule)
        .execute(&pool)
        .await
        .unwrap();

    // Accent/case-insensitive exact match via the item merchant.
    let a = seed_item(&pool, Some("NETFLIX"), "assinatura", -3990, "2025-05-10").await;
    recurring::link_item(&pool, a).await.unwrap();
    assert_eq!(link_of(&pool, a).await, (Some(rule), false));

    // Merchant-less item: description is the target.
    let b = seed_item(&pool, None, "NETFLIX", -3990, "2025-05-11").await;
    recurring::link_item(&pool, b).await.unwrap();
    assert_eq!(link_of(&pool, b).await, (Some(rule), false));

    // Substring is NOT a match (exact only).
    let c = seed_item(&pool, Some("NETFLIX BR"), "assinatura", -3990, "2025-05-12").await;
    recurring::link_item(&pool, c).await.unwrap();
    assert_eq!(link_of(&pool, c).await, (None, false));

    // Manual links are never overridden by auto logic.
    let d = seed_item(&pool, Some("NETFLIX"), "assinatura", -3990, "2025-05-13").await;
    sqlx::query("UPDATE items SET recurring_id = NULL, linked_manually = true WHERE id = $1")
        .bind(d)
        .execute(&pool)
        .await
        .unwrap();
    recurring::link_item(&pool, d).await.unwrap();
    assert_eq!(link_of(&pool, d).await, (None, true));

    // Pending items are not linked yet (they link on confirm).
    let e = seed_item(&pool, Some("NETFLIX"), "assinatura", -3990, "2025-05-14").await;
    sqlx::query("UPDATE items SET status = 'pending_review' WHERE id = $1")
        .bind(e)
        .execute(&pool)
        .await
        .unwrap();
    recurring::link_item(&pool, e).await.unwrap();
    assert_eq!(link_of(&pool, e).await, (None, false));
}

// ---------- relink_rule (create/update reconciliation) ----------

#[sqlx::test]
async fn relink_rule_links_aliases_and_isolated_cases_respecting_manual(pool: PgPool) {
    common::migrate(&pool).await;
    let rule = seed_rule(&pool, "Streaming").await;
    let a = seed_item(&pool, Some("NETFLIX"), "assinatura", -3990, "2025-05-10").await;
    let b = seed_item(&pool, None, "PAGAMENTO VIA PIX", -5000, "2025-05-11").await;
    // Manually linked to the rule before it even has aliases.
    let c = seed_item(&pool, Some("NETFLIX"), "outra coisa", -3990, "2025-05-12").await;
    sqlx::query("UPDATE items SET recurring_id = $1, linked_manually = true WHERE id = $2")
        .bind(rule)
        .bind(c)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, 'netflix', true)")
        .bind(rule)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, 'pagamento via pix', false)",
    )
    .bind(rule)
    .execute(&pool)
    .await
    .unwrap();

    recurring::relink_rule(&pool, rule).await.unwrap();

    // a: auto-linked by alias. b: one-shot isolated case (manual marker).
    assert_eq!(link_of(&pool, a).await, (Some(rule), false));
    assert_eq!(link_of(&pool, b).await, (Some(rule), true));
    // c: manual link preserved.
    assert_eq!(link_of(&pool, c).await, (Some(rule), true));
}

#[sqlx::test]
async fn relink_rule_unlinks_stale_auto_links_but_not_manual(pool: PgPool) {
    common::migrate(&pool).await;
    let rule = seed_rule(&pool, "Streaming").await;
    let a = seed_item(&pool, Some("NETFLIX"), "assinatura", -3990, "2025-05-10").await;
    let b = seed_item(&pool, Some("NETFLIX"), "outra coisa", -3990, "2025-05-11").await;
    sqlx::query("UPDATE items SET recurring_id = $1, linked_manually = true WHERE id = $2")
        .bind(rule)
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();

    // No aliases on the rule (alias was removed).
    recurring::relink_rule(&pool, rule).await.unwrap();

    assert_eq!(link_of(&pool, a).await, (None, false)); // stale auto link cleared
    assert_eq!(link_of(&pool, b).await, (Some(rule), true)); // manual kept
}

#[sqlx::test]
async fn isolated_cases_do_not_steal_linked_items(pool: PgPool) {
    common::migrate(&pool).await;
    let rule_a = seed_rule(&pool, "A").await;
    let rule_b = seed_rule(&pool, "B").await;
    // Item already auto-linked to A.
    sqlx::query("INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, 'loja', true)")
        .bind(rule_a)
        .execute(&pool)
        .await
        .unwrap();
    let item = seed_item(&pool, Some("LOJA"), "compra", -100, "2025-05-10").await;
    recurring::link_item(&pool, item).await.unwrap();
    assert_eq!(link_of(&pool, item).await, (Some(rule_a), false));

    // B adds the same name as an isolated case → must not steal the item.
    sqlx::query(
        "INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, 'loja', false)",
    )
    .bind(rule_b)
    .execute(&pool)
    .await
    .unwrap();
    recurring::relink_rule(&pool, rule_b).await.unwrap();
    assert_eq!(link_of(&pool, item).await, (Some(rule_a), false));
}
