use anyhow::Result;
use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

use crate::services::tags;

/// Normalize a name for comparison/matching: trim + lowercase + strip accents.
pub fn normalize_name(s: &str) -> String {
    tags::strip_accents(s.trim()).to_lowercase()
}

/// Does an item's text identify as this alias?
///
/// Exact normalized equality wins. Otherwise, tolerate a trailing money value
/// embedded in the name: the normalized alias must be a prefix at a token
/// boundary and the remainder must look like an amount (digits with optional
/// separators, possibly prefixed by "r$"). This captures varying payments under
/// one alias — e.g. "PREST HAB 1847,32" / "PREST HAB 1832,10" → alias "prest hab".
pub fn matches_alias(text: &str, alias: &str) -> bool {
    let n = normalize_name(text);
    let a = normalize_name(alias);
    if n == a {
        return true;
    }
    let Some(rest) = n.strip_prefix(&a) else {
        return false;
    };
    let rest = rest.trim_start_matches(|c: char| matches!(c, ' ' | '-' | '/' | ':' | '.'));
    let rest = rest.strip_prefix("r$").map(str::trim_start).unwrap_or(rest);
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ' '))
}

/// Map a median day-gap between occurrences to a (frequency, interval) window.
/// Kept when the detection feature was removed — the add-flow window suggestion reuses it.
/// Windows are deliberately a bit loose: real bills land 28–35 days apart (day-of-week
/// jitter, weekends, holidays), not exactly 30.
pub fn classify_gap(median_gap: i64) -> Option<(String, i32)> {
    if (6..=8).contains(&median_gap) {
        Some(("weekly".to_string(), 1))
    } else if (25..=35).contains(&median_gap) {
        Some(("monthly".to_string(), 1))
    } else if (55..=70).contains(&median_gap) {
        Some(("monthly".to_string(), 2))
    } else if (340..=380).contains(&median_gap) {
        Some(("yearly".to_string(), 1))
    } else {
        None
    }
}

fn add_months(d: NaiveDate, n: u32) -> NaiveDate {
    d.checked_add_months(chrono::Months::new(n)).unwrap_or(d)
}

/// Advance `next` by the rule's window until it is `>= today`.
/// Pure and idempotent — used at read time so next dates are never in the past.
pub fn advance_next_due(next: NaiveDate, frequency: &str, interval: i32, today: NaiveDate) -> NaiveDate {
    let interval = interval.max(1);
    let mut d = next;
    // Safety cap (~100 years of monthly steps) against pathological inputs.
    for _ in 0..1200 {
        if d >= today {
            break;
        }
        d = match frequency {
            "weekly" => d + chrono::Duration::days(7 * interval as i64),
            "monthly" => add_months(d, interval as u32),
            _ => add_months(d, interval as u32 * 12), // yearly / fallback
        };
    }
    d
}

/// Validate name entries for create/update:
/// - each name must exist in the data (as an item merchant or description, normalized);
/// - auto aliases must be globally unique across rules.
/// Returns a list of pt-BR error messages (empty = valid).
pub async fn validate_entries(
    pool: &PgPool,
    aliases: &[String],
    isolated_cases: &[String],
    exclude_rule_id: Option<Uuid>,
) -> Result<Vec<String>> {
    let mut errors = Vec::new();

    // Names that exist in the data (merchant or description). Items store raw
    // names, so the comparison happens in Rust — same as matching. `matches_alias`
    // tolerates a trailing amount, so one alias can cover a varying payment
    // (e.g. "PREST HAB 1847,32" / "PREST HAB 1832,10" → "prest hab").
    let merchants: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT merchant FROM items WHERE merchant IS NOT NULL AND merchant <> ''",
    )
    .fetch_all(pool)
    .await?;
    let descriptions: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT description FROM items").fetch_all(pool).await?;

    for name in aliases.iter().chain(isolated_cases.iter()) {
        let n = normalize_name(name);
        if n.is_empty() {
            errors.push(format!("'{name}' é inválido"));
            continue;
        }
        let exists = merchants
            .iter()
            .chain(descriptions.iter())
            .any(|raw| matches_alias(raw, &n));
        if !exists {
            errors.push(format!("'{name}' não existe nos dados"));
        }
    }

    for name in aliases {
        let n = normalize_name(name);
        if n.is_empty() {
            continue;
        }
        let conflict: Option<(Uuid,)> = sqlx::query_as(
            "SELECT rule_id FROM recurring_aliases
             WHERE name = $1 AND is_alias AND ($2::uuid IS NULL OR rule_id <> $2)",
        )
        .bind(&n)
        .bind(exclude_rule_id)
        .fetch_optional(pool)
        .await?;
        if let Some((other_rule,)) = conflict {
            let other_name: Option<String> = sqlx::query_scalar(
                "SELECT name FROM recurring_rules WHERE id = $1",
            )
            .bind(other_rule)
            .fetch_optional(pool)
            .await?;
            errors.push(format!(
                "alias '{name}' já usado pela regra '{}'",
                other_name.unwrap_or_default()
            ));
        }
    }

    Ok(errors)
}

/// Auto-link a single confirmed item to the rule whose auto alias matches its
/// merchant (fallback: description), exact normalized equality. Manual links,
/// receipts, installments and non-confirmed items are skipped.
pub async fn link_item(pool: &PgPool, item_id: Uuid) -> Result<()> {
    let row: Option<(Option<Uuid>, Option<String>, String, String, Option<Uuid>, Option<i32>, bool)> =
        sqlx::query_as(
            "SELECT parent_id, merchant, description, status, recurring_id, installment_count, linked_manually
             FROM items WHERE id = $1",
        )
        .bind(item_id)
        .fetch_optional(pool)
        .await?;
    let Some((parent_id, merchant, description, status, current_rule, installment_count, linked)) = row else {
        return Ok(());
    };
    if parent_id.is_some()
        || status != "confirmed"
        || linked
        || installment_count.unwrap_or(0) > 1
    {
        return Ok(());
    }

    let text = merchant
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(&description);

    // Most specific (longest) alias wins when several could match — e.g. an item
    // "LOJA 10" fits both aliases "loja" (prefix + amount) and "loja 10" (exact).
    let aliases: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT name, rule_id FROM recurring_aliases WHERE is_alias ORDER BY length(name) DESC",
    )
    .fetch_all(pool)
    .await?;
    if let Some((_, rule_id)) = aliases.iter().find(|(a, _)| matches_alias(text, a)) {
        if current_rule != Some(*rule_id) {
            sqlx::query("UPDATE items SET recurring_id = $1, updated_at = now() WHERE id = $2")
                .bind(rule_id)
                .bind(item_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Reconcile one rule against all items, after create/update/delete:
/// 1. unlink its non-manual items;
/// 2. re-link items whose merchant/description matches an auto alias;
/// 3. one-shot link items matching isolated cases (as manual links).
/// Manual links (`linked_manually = true`) are never touched.
pub async fn relink_rule(pool: &PgPool, rule_id: Uuid) -> Result<()> {
    let aliases: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM recurring_aliases WHERE rule_id = $1 AND is_alias",
    )
    .bind(rule_id)
    .fetch_all(pool)
    .await?;
    let isolated: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM recurring_aliases WHERE rule_id = $1 AND NOT is_alias",
    )
    .bind(rule_id)
    .fetch_all(pool)
    .await?;

    // 1. Unlink non-manual items of this rule.
    sqlx::query(
        "UPDATE items SET recurring_id = NULL, linked_manually = false, updated_at = now()
         WHERE recurring_id = $1 AND NOT linked_manually",
    )
    .bind(rule_id)
    .execute(pool)
    .await?;

    // 2/3. Load candidates once, match in Rust (normalized exact).
    let rows: Vec<(Uuid, Option<String>, String, bool, Option<Uuid>)> = sqlx::query_as(
        "SELECT id, merchant, description, linked_manually, recurring_id FROM items
         WHERE status = 'confirmed' AND parent_id IS NULL
           AND (installment_count IS NULL OR installment_count <= 1)",
    )
    .fetch_all(pool)
    .await?;

    // Matching tolerates a trailing amount in the item name (see `matches_alias`),
    // so a varying payment maps to one alias.
    for (id, merchant, description, linked, current) in &rows {
        if *linked {
            continue;
        }
        let text = merchant
            .as_deref()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or(description);
        if aliases.iter().any(|a| matches_alias(text, a))
            && (current.is_none() || *current == Some(rule_id))
        {
            sqlx::query("UPDATE items SET recurring_id = $1, updated_at = now() WHERE id = $2")
                .bind(rule_id)
                .bind(id)
                .execute(pool)
                .await?;
        }
    }
    for (id, merchant, description, linked, current) in &rows {
        if *linked || current.is_some() {
            continue;
        }
        let text = merchant
            .as_deref()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or(description);
        if isolated.iter().any(|a| matches_alias(text, a)) {
            sqlx::query(
                "UPDATE items SET recurring_id = $1, linked_manually = true, updated_at = now()
                 WHERE id = $2",
            )
            .bind(rule_id)
            .bind(id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}
