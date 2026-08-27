use std::collections::{BTreeSet, HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::RecurringRule;
use crate::services::recurring;
use crate::AppState;

/// Base rule columns (no derived fields). `category_id`/`category_name` are
/// derived placeholders — filled in by [`fill_derived`] from linked occurrences.
const RULE_BASE: &str = "r.id, r.name, r.amount_cents, r.currency, \
     NULL::uuid AS category_id, NULL::text AS category_name, r.frequency, r.interval, r.day_of_month, \
     r.next_due_on, r.is_active, r.source, r.created_at, r.updated_at";

/// Same columns without the `r.` alias (for INSERT/UPDATE RETURNING).
const RULE_BASE_UNALIASED: &str = "id, name, amount_cents, currency, \
     NULL::uuid AS category_id, NULL::text AS category_name, frequency, interval, day_of_month, \
     next_due_on, is_active, source, created_at, updated_at";

/// Row shape for the unaliased base columns (INSERT/UPDATE RETURNING).
#[derive(Debug, FromRow)]
struct RuleBase {
    id: Uuid,
    name: String,
    amount_cents: i64,
    currency: String,
    category_id: Option<Uuid>,
    category_name: Option<String>,
    frequency: String,
    interval: i32,
    day_of_month: Option<i32>,
    next_due_on: Option<NaiveDate>,
    is_active: bool,
    source: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn into_rule(base: RuleBase, aliases: Vec<String>, isolated_cases: Vec<String>) -> RecurringRule {
    RecurringRule {
        id: base.id,
        name: base.name,
        amount_cents: base.amount_cents,
        currency: base.currency,
        category_id: base.category_id,
        category_name: base.category_name,
        frequency: base.frequency,
        interval: base.interval,
        day_of_month: base.day_of_month,
        next_due_on: base.next_due_on,
        is_active: base.is_active,
        source: base.source,
        created_at: base.created_at,
        updated_at: base.updated_at,
        aliases,
        isolated_cases,
        tags: Vec::new(),
        tags_conflict: false,
        days_until: None,
    }
}

/// Name entries (auto aliases + isolated cases) as scalar subqueries.
const RULE_ENTRIES: &str = "\
     COALESCE((SELECT array_agg(a.name ORDER BY a.name) FROM recurring_aliases a \
               WHERE a.rule_id = r.id AND a.is_alias), ARRAY[]::text[]) AS aliases, \
     COALESCE((SELECT array_agg(a.name ORDER BY a.name) FROM recurring_aliases a \
               WHERE a.rule_id = r.id AND NOT a.is_alias), ARRAY[]::text[]) AS isolated_cases";

/// Derived placeholders filled in by [`fill_derived`] after fetching.
const RULE_DERIVED: &str = "'{}'::text[] AS tags, false AS tags_conflict, NULL::bigint AS days_until";

fn default_interval() -> i32 {
    1
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct RecurringInput {
    pub name: String,
    /// Signed cents (negative = expense).
    pub amount_cents: i64,
    /// 'weekly' | 'monthly' | 'yearly'
    pub frequency: String,
    #[serde(default = "default_interval")]
    pub interval: i32,
    pub day_of_month: Option<i32>,
    pub next_due_on: Option<NaiveDate>,
    #[serde(default = "default_true")]
    pub is_active: bool,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub isolated_cases: Vec<String>,
}

/// Normalize + dedupe name entries before persisting.
fn normalize_entries(entries: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for e in entries {
        let n = recurring::normalize_name(e);
        if n.is_empty() || !seen.insert(n.clone()) {
            continue;
        }
        out.push(n);
    }
    out
}

/// Clamp a user-supplied next due date to never be in the past.
fn clamp_next_due(next: Option<NaiveDate>) -> Option<NaiveDate> {
    let today = Utc::now().date_naive();
    next.map(|d| if d < today { today } else { d })
}

/// Fill derived fields (tags union, tags_conflict, category from the most recent
/// linked occurrence, effective next date + days_until) for a batch of rules with
/// a single items query.
async fn fill_derived(pool: &PgPool, rules: &mut [RecurringRule]) -> Result<(), AppError> {
    let ids: Vec<Uuid> = rules.iter().map(|r| r.id).collect();
    let mut union: HashMap<Uuid, BTreeSet<String>> = HashMap::new();
    let mut sets: HashMap<Uuid, HashSet<Vec<String>>> = HashMap::new();
    // Most recent occurrence's category per rule — rows are ordered newest-first,
    // so the first non-null category seen per rule is the latest one.
    let mut latest_cat: HashMap<Uuid, Uuid> = HashMap::new();
    if !ids.is_empty() {
        let rows: Vec<(Uuid, Vec<String>, Option<Uuid>)> = sqlx::query_as(
            "SELECT recurring_id, tags, category_id FROM items
             WHERE recurring_id = ANY($1) AND status = 'confirmed'
               AND (cardinality(tags) > 0 OR category_id IS NOT NULL)
             ORDER BY occurred_on DESC, created_at DESC",
        )
        .bind(&ids)
        .fetch_all(pool)
        .await?;
        for (rid, tags, cat) in rows {
            union.entry(rid).or_default().extend(tags.iter().cloned());
            sets.entry(rid).or_default().insert(tags);
            if let Some(c) = cat {
                latest_cat.entry(rid).or_insert(c);
            }
        }
    }

    let cat_ids: Vec<Uuid> = latest_cat.values().copied().collect();
    let mut cat_names: HashMap<Uuid, String> = HashMap::new();
    if !cat_ids.is_empty() {
        cat_names = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, name FROM categories WHERE id = ANY($1)",
        )
        .bind(&cat_ids)
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
    }

    let today = Utc::now().date_naive();
    for r in rules {
        r.tags = union
            .remove(&r.id)
            .map(|s| s.into_iter().collect())
            .unwrap_or_default();
        r.tags_conflict = sets.get(&r.id).map(|s| s.len() > 1).unwrap_or(false);
        r.category_id = latest_cat.get(&r.id).copied();
        r.category_name = r.category_id.and_then(|id| cat_names.get(&id).cloned());
        if let Some(due) = r.next_due_on {
            let eff = recurring::advance_next_due(due, &r.frequency, r.interval, today);
            r.next_due_on = Some(eff);
            r.days_until = Some((eff - today).num_days());
        } else {
            r.days_until = None;
        }
    }
    Ok(())
}

fn select_sql(where_clause: &str) -> String {
    format!(
        "SELECT {RULE_BASE}, {RULE_ENTRIES}, {RULE_DERIVED} \
         FROM recurring_rules r {where_clause}"
    )
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<RecurringRule>>, AppError> {
    let mut rules: Vec<RecurringRule> = sqlx::query_as::<_, RecurringRule>(sqlx::AssertSqlSafe(
        select_sql("ORDER BY r.is_active DESC, r.next_due_on ASC NULLS LAST, r.name"),
    ))
    .fetch_all(&state.pool)
    .await?;
    fill_derived(&state.pool, &mut rules).await?;
    Ok(Json(rules))
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<RecurringInput>,
) -> Result<Json<RecurringRule>, AppError> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::bad_request("nome é obrigatório"));
    }
    let aliases = normalize_entries(&input.aliases);
    let isolated = normalize_entries(&input.isolated_cases);
    let errors = recurring::validate_entries(&state.pool, &aliases, &isolated, None).await?;
    if !errors.is_empty() {
        return Err(AppError::bad_request(errors.join("; ")));
    }
    let next_due_on = clamp_next_due(input.next_due_on);

    let base = sqlx::query_as::<_, RuleBase>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO recurring_rules
           (name, amount_cents, currency, frequency, interval,
            day_of_month, next_due_on, is_active, source)
         VALUES ($1, $2, 'BRL', $3, $4, $5, $6, $7, 'manual')
         RETURNING {RULE_BASE_UNALIASED}"
    )))
    .bind(&name)
    .bind(input.amount_cents)
    .bind(&input.frequency)
    .bind(input.interval)
    .bind(input.day_of_month)
    .bind(next_due_on)
    .bind(input.is_active)
    .fetch_one(&state.pool)
    .await?;

    insert_entries(&state.pool, base.id, &aliases, &isolated).await?;
    recurring::relink_rule(&state.pool, base.id).await?;

    let mut rule = into_rule(base, aliases, isolated);
    fill_derived(&state.pool, std::slice::from_mut(&mut rule)).await?;
    Ok(Json(rule))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<RecurringInput>,
) -> Result<Json<RecurringRule>, AppError> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::bad_request("nome é obrigatório"));
    }
    let aliases = normalize_entries(&input.aliases);
    let isolated = normalize_entries(&input.isolated_cases);
    let errors = recurring::validate_entries(&state.pool, &aliases, &isolated, Some(id)).await?;
    if !errors.is_empty() {
        return Err(AppError::bad_request(errors.join("; ")));
    }
    let next_due_on = clamp_next_due(input.next_due_on);

    let base = sqlx::query_as::<_, RuleBase>(sqlx::AssertSqlSafe(format!(
        "UPDATE recurring_rules
         SET name = $1, amount_cents = $2, frequency = $3, interval = $4,
             day_of_month = $5, next_due_on = $6, is_active = $7, updated_at = now()
         WHERE id = $8
         RETURNING {RULE_BASE_UNALIASED}"
    )))
    .bind(&name)
    .bind(input.amount_cents)
    .bind(&input.frequency)
    .bind(input.interval)
    .bind(input.day_of_month)
    .bind(next_due_on)
    .bind(input.is_active)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("regra recorrente não encontrada"))?;

    // Replace name entries.
    sqlx::query("DELETE FROM recurring_aliases WHERE rule_id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    insert_entries(&state.pool, id, &aliases, &isolated).await?;
    recurring::relink_rule(&state.pool, id).await?;

    let mut rule = into_rule(base, aliases, isolated);
    fill_derived(&state.pool, std::slice::from_mut(&mut rule)).await?;
    Ok(Json(rule))
}

async fn insert_entries(
    pool: &PgPool,
    rule_id: Uuid,
    aliases: &[String],
    isolated: &[String],
) -> Result<(), AppError> {
    for name in aliases {
        sqlx::query(
            "INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, $2, true)",
        )
        .bind(rule_id)
        .bind(name)
        .execute(pool)
        .await?;
    }
    for name in isolated {
        sqlx::query(
            "INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES ($1, $2, false)",
        )
        .bind(rule_id)
        .bind(name)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Detach linked items first (also clears their manual marker, so they can
    // auto-link to a future rule again).
    sqlx::query(
        "UPDATE items SET recurring_id = NULL, linked_manually = false, updated_at = now()
         WHERE recurring_id = $1",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;
    let res = sqlx::query("DELETE FROM recurring_rules WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("regra recorrente não encontrada"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------- Autocomplete & profiles ----------

#[derive(Debug, Deserialize)]
pub struct NameQuery {
    pub q: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProfileQuery {
    pub name: Option<String>,
}

/// Distinct names for the alias autocomplete (items + merchant memory). The item
/// name is the merchant when present, else the description — the same source
/// `link_item` matches against — so merchant-less bills like "PREST HAB" show up.
pub async fn merchant_names(pool: &PgPool, q: &str) -> Result<Vec<String>, AppError> {
    let q = q.trim().to_string();
    let mut names: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(merchant, ''), description) FROM items
         WHERE COALESCE(NULLIF(merchant, ''), description) ILIKE '%' || $1 || '%'
         UNION
         SELECT merchant FROM merchant_memory
         WHERE merchant <> '' AND merchant ILIKE '%' || $1 || '%'
         ORDER BY 1 LIMIT 25",
    )
    .bind(&q)
    .fetch_all(pool)
    .await?;
    // Raw item names and normalized memory merchants can both be present;
    // dedupe case/accent-insensitively (keep the first, sorted variant).
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(recurring::normalize_name(n)));
    Ok(names)
}

/// Distinct merchant names (items + merchant memory) for autocomplete.
pub async fn merchants(
    State(state): State<AppState>,
    Query(q): Query<NameQuery>,
) -> Result<Json<Vec<String>>, AppError> {
    Ok(Json(
        merchant_names(&state.pool, q.q.as_deref().unwrap_or_default()).await?,
    ))
}

/// Auto-derivation payload for the add flow — only meaningful when the name
/// matches an existing merchant (exact, normalized). 404 otherwise.
pub async fn merchant_profile(
    State(state): State<AppState>,
    Query(q): Query<ProfileQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = q.name.unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return Err(AppError::bad_request("name é obrigatório"));
    }
    let normalized = recurring::normalize_name(&name);

    // Recent confirmed items (2 years), filtered by exact normalized match in Rust.
    let since = Utc::now().date_naive() - chrono::Duration::days(730);
    let rows: Vec<(Option<String>, String, i64, NaiveDate, Option<Uuid>, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT merchant, description, amount_cents, occurred_on, category_id, created_at
             FROM items WHERE status = 'confirmed' AND occurred_on >= $1
             ORDER BY occurred_on DESC, created_at DESC",
        )
        .bind(since)
        .fetch_all(&state.pool)
        .await?;
    let matched: Vec<(Option<String>, String, i64, NaiveDate, Option<Uuid>)> = rows
        .into_iter()
        .filter(|(m, d, _, _, _, _)| {
            let text = m
                .as_deref()
                .filter(|x| !x.trim().is_empty())
                .unwrap_or(d);
            // Tolerant of a trailing amount in the name (varying payments).
            recurring::matches_alias(text, &normalized)
        })
        .map(|(m, d, amount, date, cat, _)| (m, d, amount, date, cat))
        .collect();

    let Some(last) = matched.first() else {
        return Err(AppError::not_found("nenhum item encontrado para este nome"));
    };

    // Category: memory first (merchant_memory stores normalized merchants), else last item's.
    let mem_category: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT category_id FROM merchant_memory WHERE merchant = $1",
    )
    .bind(&normalized)
    .fetch_optional(&state.pool)
    .await?;
    let category_id = mem_category.flatten().or(last.4);
    let category_name: Option<String> = match category_id {
        Some(cid) => sqlx::query_scalar("SELECT name FROM categories WHERE id = $1")
            .bind(cid)
            .fetch_optional(&state.pool)
            .await?,
        None => None,
    };

    // Suggested window from the median gap of this merchant's occurrences.
    let mut dates: Vec<NaiveDate> = matched.iter().map(|(_, _, _, d, _)| *d).collect();
    dates.sort_unstable();
    let mut suggested_frequency = "yearly".to_string();
    let mut suggested_interval = 1;
    let gaps: Vec<i64> = dates.windows(2).map(|w| (w[1] - w[0]).num_days()).collect();
    if !gaps.is_empty() {
        let mut g = gaps.clone();
        g.sort_unstable();
        if let Some((freq, interval)) = recurring::classify_gap(g[g.len() / 2]) {
            suggested_frequency = freq;
            suggested_interval = interval;
        }
    }

    // Next due date: last occurrence advanced by the suggested window (never in
    // the past) — used by the alias-click prefill in the rule form.
    let next_due_on =
        recurring::advance_next_due(last.3, &suggested_frequency, suggested_interval, Utc::now().date_naive());

    Ok(Json(json!({
        "merchant": name,
        "amount_cents": last.2,
        "category_id": category_id,
        "category_name": category_name,
        "last_occurred_on": last.3,
        "suggested_frequency": suggested_frequency,
        "suggested_interval": suggested_interval,
        "next_due_on": next_due_on,
    })))
}

/// Latest linked occurrences for a rule (source of truth: `items.recurring_id`).
pub async fn occurrences(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM recurring_rules WHERE id = $1)",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if !exists {
        return Err(AppError::not_found("regra recorrente não encontrada"));
    }
    let rows: Vec<(NaiveDate, String, i64, Vec<String>, bool)> = sqlx::query_as(
        "SELECT occurred_on, description, amount_cents, tags, linked_manually FROM items
         WHERE recurring_id = $1 AND status = 'confirmed'
         ORDER BY occurred_on DESC, created_at DESC LIMIT 10",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(occurred_on, description, amount_cents, tags, linked_manually)| {
                json!({
                    "occurred_on": occurred_on,
                    "description": description,
                    "amount_cents": amount_cents,
                    "tags": tags,
                    "linked_manually": linked_manually,
                })
            })
            .collect(),
    ))
}

// ---------- Monthly recurring cost KPI ----------

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct MonthlyCost {
    /// Monthly-equivalent cost (cents) of all active expense rules.
    pub monthly_cents: i64,
    /// Number of active expense rules contributing.
    pub rule_count: i64,
}

/// `GET /recurring/monthly-cost` — global KPI: the normalized monthly cost of
/// all active recurring rules (weekly/yearly rules are normalized to months).
/// Ignores all date/filter params on purpose.
pub async fn monthly_cost(State(state): State<AppState>) -> Result<Json<MonthlyCost>, AppError> {
    let rows: Vec<(i64, String, i32)> = sqlx::query_as(
        "SELECT amount_cents, frequency, interval FROM recurring_rules WHERE is_active",
    )
    .fetch_all(&state.pool)
    .await?;

    let mut total = 0.0f64;
    let mut rule_count = 0i64;
    for (amount_cents, frequency, interval) in rows {
        if amount_cents >= 0 {
            continue; // expenses only
        }
        let interval = interval.max(1) as f64;
        let factor = match frequency.as_str() {
            "weekly" => 52.0 / 12.0 / interval,
            "monthly" => 1.0 / interval,
            _ => 1.0 / (12.0 * interval), // yearly / fallback
        };
        total += amount_cents.unsigned_abs() as f64 * factor;
        rule_count += 1;
    }

    Ok(Json(MonthlyCost {
        monthly_cents: total.round() as i64,
        rule_count,
    }))
}
