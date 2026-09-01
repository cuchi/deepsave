use axum::Json;
use axum::extract::{Query, State};
use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::AppState;
use crate::error::AppError;

#[derive(Debug, Deserialize)]
pub struct DashboardQuery {
    /// YYYY-MM, optional (used only when no date range is given).
    pub month: Option<String>,
    // Same filters as the items list.
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
    pub search: Option<String>,
    /// Comma-separated category ids (OR).
    pub category_ids: Option<String>,
    pub kind: Option<String>,
    /// Comma-separated tags (OR: item carries any).
    pub tags: Option<String>,
    pub bank: Option<String>,
    pub installments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TrendQuery {
    pub months: Option<i32>,
    pub date_to: Option<NaiveDate>,
    pub search: Option<String>,
    /// Comma-separated category ids (OR).
    pub category_ids: Option<String>,
    pub kind: Option<String>,
    /// Comma-separated tags (OR: item carries any).
    pub tags: Option<String>,
    pub bank: Option<String>,
    pub installments: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CategoryTotal {
    pub category_id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub total_cents: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct MerchantTotal {
    pub merchant: String,
    pub total_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct Dashboard {
    /// Range label ("YYYY-MM-DD..YYYY-MM-DD" or "YYYY-MM").
    pub month: String,
    pub total_spend_cents: i64,
    pub total_income_cents: i64,
    pub by_category: Vec<CategoryTotal>,
    pub top_merchants: Vec<MerchantTotal>,
}

#[derive(Debug, Serialize)]
pub struct TrendPoint {
    pub month: String,
    pub spend_cents: i64,
    pub income_cents: i64,
}

/// Shared WHERE for dashboard/trend aggregation. Params $1..$8: date_from,
/// date_to, search, category_ids, kind, tags, bank, installments. Unlike the
/// items list, rejected items are always excluded (they're not real activity).
const AGG_FILTERS: &str = "
    ($1::date IS NULL OR items.occurred_on >= $1)
    AND ($2::date IS NULL OR items.occurred_on <= $2)
    AND items.status != 'rejected'
    AND ($3::text IS NULL
         OR items.description ILIKE '%' || $3 || '%'
         OR COALESCE(items.merchant, '') ILIKE '%' || $3 || '%'
         OR array_to_string(items.tags, ' ') ILIKE '%' || $3 || '%')
    AND (cardinality($4) = 0
         OR items.category_id::text = ANY($4)
         OR ('__none' = ANY($4) AND items.category_id IS NULL))
    AND ($5::text IS NULL OR items.kind = $5
         OR ($5 = 'expense' AND items.kind = 'refund' AND rc.kind = 'expense'))
    AND (cardinality($6) = 0
         OR items.tags && $6
         OR ('__none' = ANY($6) AND cardinality(items.tags) = 0))
    AND ($7::text IS NULL OR EXISTS (
          SELECT 1 FROM documents d
          JOIN sources s ON s.id = d.source_id
          WHERE d.id = items.document_id AND s.bank = $7))
    AND ($8::text IS NULL OR $8 = 'all'
         OR ($8 = 'first_only' AND NOT (COALESCE(items.installment_count, 0) > 1 AND COALESCE(items.installment, 0) > 1))
         OR ($8 = 'only' AND COALESCE(items.installment_count, 0) > 1))
";

/// When the installments filter is 'first_only', the first parcel stands in for
/// the whole purchase (parcel × count). References $8 (the installments param).
const AGG_AMOUNT_ADJ: &str = "
    CASE WHEN $8 = 'first_only' AND items.installment_count > 1 AND items.installment = 1
         THEN items.amount_cents * items.installment_count
         ELSE items.amount_cents END";

/// Expense side of spend aggregations: an expense, or a **linked refund** — the
/// latter subtracts (money back) exactly where its charge was counted. Requires
/// the `rc` join (the charge) to be present. Self-contained (no nested
/// `{…}` placeholders — `format!` would not expand them inside a substituted
/// const), so the first_only installment adjustment is inlined for expenses.
const NET_AMOUNT: &str = "
    CASE WHEN items.kind = 'expense' THEN -(
           CASE WHEN $8 = 'first_only' AND items.installment_count > 1 AND items.installment = 1
                THEN items.amount_cents * items.installment_count
                ELSE items.amount_cents END)
         WHEN items.kind = 'refund' AND rc.kind = 'expense' THEN -items.amount_cents
         ELSE 0 END";

/// Bucket key: the charge's field when the row is a linked refund (so the
/// refund nets in the charge's month/category/merchant).
const BUCKET_DATE: &str = "COALESCE(rc.occurred_on, items.occurred_on)";
const BUCKET_CAT: &str = "COALESCE(rc.category_id, items.category_id)";
const BUCKET_MERCHANT: &str = "COALESCE(rc.merchant, items.merchant)";
const BUCKET_TAGS: &str = "COALESCE(rc.tags, items.tags)";
/// Expense rows: expenses plus refunds linked to an expense charge.
const EXPENSE_OR_REFUND: &str =
    "(items.kind = 'expense' OR (items.kind = 'refund' AND rc.kind = 'expense'))";
/// Join to the charge a refund reverses (netting target).
const REFUND_JOIN: &str =
    "LEFT JOIN items rc ON rc.id = items.refunded_item_id AND rc.kind = 'expense'";

/// Resolve the aggregation window: explicit date range wins; else `month`;
/// else no date filter (all history). The UI pre-fills the last complete month
/// on first load, but the API itself must not — "cleared dates" means "tudo".
fn resolve_range(
    date_from: Option<NaiveDate>,
    date_to: Option<NaiveDate>,
    month: Option<&str>,
) -> Result<(Option<NaiveDate>, Option<NaiveDate>, String), AppError> {
    if let (Some(from), Some(to)) = (date_from, date_to) {
        if from > to {
            return Err(AppError::bad_request("date_from must not be after date_to"));
        }
        return Ok((Some(from), Some(to), format!("{from}..{to}")));
    }

    let Some(m) = month else {
        return Ok((None, None, "tudo".to_string()));
    };
    let (y, mo) = m
        .split_once('-')
        .ok_or_else(|| AppError::bad_request("month must be YYYY-MM"))?;
    let year: i32 = y
        .parse()
        .map_err(|_| AppError::bad_request("invalid month"))?;
    let month_num: u32 = mo
        .parse()
        .map_err(|_| AppError::bad_request("invalid month"))?;
    if !(1..=12).contains(&month_num) {
        return Err(AppError::bad_request("invalid month"));
    }

    let start = NaiveDate::from_ymd_opt(year, month_num, 1)
        .ok_or_else(|| AppError::bad_request("invalid month"))?;
    let end = if month_num == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month_num + 1, 1)
    }
    .ok_or_else(|| AppError::bad_request("invalid month"))?
        - chrono::Duration::days(1);
    Ok((Some(start), Some(end), format!("{year:04}-{month_num:02}")))
}

/// Parse a comma-separated filter list (see `routes::items::split_filters`).
pub(crate) fn split_filters(s: &Option<String>) -> Vec<String> {
    s.as_deref()
        .map(|v| {
            v.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub async fn dashboard(
    State(state): State<AppState>,
    Query(q): Query<DashboardQuery>,
) -> Result<Json<Dashboard>, AppError> {
    Ok(Json(dashboard_data(&state.pool, &q).await?))
}

/// Core aggregation, kept pool-level so integration tests can drive it without
/// an `AppState`. Sums **root** items only (receipt children are allocations of
/// their parent and would double-count).
pub async fn dashboard_data(pool: &PgPool, q: &DashboardQuery) -> Result<Dashboard, AppError> {
    let (from, to, label) = resolve_range(q.date_from, q.date_to, q.month.as_deref())?;
    let category_ids = split_filters(&q.category_ids);
    let tags = split_filters(&q.tags);
    let (spend, income): (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT COALESCE(SUM({NET_AMOUNT}), 0)::bigint,
                COALESCE(SUM(CASE WHEN items.kind = 'income' THEN {AGG_AMOUNT_ADJ} ELSE 0 END), 0)::bigint
         FROM items
         {REFUND_JOIN}
         WHERE {AGG_FILTERS}"
    )))
    .bind(from)
    .bind(to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_one(pool)
    .await?;

    let by_category: Vec<CategoryTotal> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT c.id AS category_id, c.name AS name, c.color AS color,
                COALESCE(SUM(-{AGG_AMOUNT_ADJ}), 0)::bigint AS total_cents
         FROM items
         {REFUND_JOIN}
         JOIN categories c ON c.id = {BUCKET_CAT}
         WHERE {EXPENSE_OR_REFUND} AND {AGG_FILTERS}
         GROUP BY c.id, c.name, c.color
         ORDER BY total_cents DESC"
    )))
    .bind(from)
    .bind(to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
    .await?;

    let top_merchants: Vec<MerchantTotal> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {BUCKET_MERCHANT} AS merchant, SUM(-{AGG_AMOUNT_ADJ})::bigint AS total_cents
         FROM items
         {REFUND_JOIN}
         WHERE {EXPENSE_OR_REFUND} AND {BUCKET_MERCHANT} IS NOT NULL
           AND {AGG_FILTERS}
         GROUP BY {BUCKET_MERCHANT}
         ORDER BY total_cents DESC
         LIMIT 10"
    )))
    .bind(from)
    .bind(to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
    .await?;

    Ok(Dashboard {
        month: label,
        total_spend_cents: spend,
        total_income_cents: income,
        by_category,
        top_merchants,
    })
}

/// Digest: saved AI narrative per month. `GET` reads the saved digest (no AI
/// call), `POST` generates + upserts it, `DELETE` removes it. Generation is
/// explicit so the user can regenerate anytime and the summary stays stable
/// until then. Graceful: returns `{"ai": false}` when no AI is configured.
#[derive(Debug, Deserialize)]
pub struct DigestQuery {
    pub month: Option<String>,
}

/// "YYYY-MM" → first day of that month as a NaiveDate.
fn month_first_day(month: &str) -> Result<NaiveDate, AppError> {
    let (y, m) = month
        .split_once('-')
        .ok_or_else(|| AppError::bad_request("month must be YYYY-MM"))?;
    let year: i32 = y
        .parse()
        .map_err(|_| AppError::bad_request("invalid month"))?;
    let mo: u32 = m
        .parse()
        .map_err(|_| AppError::bad_request("invalid month"))?;
    NaiveDate::from_ymd_opt(year, mo, 1).ok_or_else(|| AppError::bad_request("invalid month"))
}

fn digest_month(q: &DigestQuery) -> String {
    q.month.clone().unwrap_or_else(|| {
        let t = chrono::Utc::now().date_naive();
        format!("{}-{:02}", t.year(), t.month())
    })
}

/// Read the saved digest for a month (never calls the AI).
pub async fn digest_get(
    State(state): State<AppState>,
    Query(q): Query<DigestQuery>,
) -> Result<Json<Value>, AppError> {
    if !state.ai.enabled() {
        return Ok(Json(json!({ "ai": false })));
    }
    let month = digest_month(&q);
    let (from, _to, label) = match resolve_range(None, None, Some(&month))? {
        (Some(f), Some(t), label) => (f, t, label),
        _ => unreachable!("month always resolves to a range"),
    };
    let saved: Option<(String, String, Value, Value, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as(
            "SELECT to_char(month, 'YYYY-MM'), resumo, destaques, avisos, updated_at
             FROM monthly_digests WHERE month = $1",
        )
        .bind(from)
        .fetch_optional(&state.pool)
        .await?;
    let out = match saved {
        Some((_, resumo, destaques, avisos, updated_at)) => json!({
            "ai": true,
            "month": label,
            "saved": true,
            "resumo": resumo,
            "destaques": destaques,
            "avisos": avisos,
            "updated_at": updated_at,
        }),
        None => json!({ "ai": true, "month": label, "saved": false }),
    };
    Ok(Json(out))
}

/// Generate (or regenerate) + save the digest for a month.
pub async fn digest_post(
    State(state): State<AppState>,
    Query(q): Query<DigestQuery>,
) -> Result<Json<Value>, AppError> {
    let month = digest_month(&q);
    let value = generate_digest(&state, &month).await?;
    let from = month_first_day(&month)?;
    let saved: (chrono::DateTime<chrono::Utc>,) = sqlx::query_as(
        "INSERT INTO monthly_digests (month, resumo, destaques, avisos)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (month) DO UPDATE
           SET resumo = EXCLUDED.resumo, destaques = EXCLUDED.destaques,
               avisos = EXCLUDED.avisos, updated_at = now()
         RETURNING updated_at",
    )
    .bind(from)
    .bind(value.get("resumo").and_then(|v| v.as_str()).unwrap_or(""))
    .bind(
        value
            .get("destaques")
            .cloned()
            .unwrap_or(Value::Array(vec![])),
    )
    .bind(value.get("avisos").cloned().unwrap_or(Value::Array(vec![])))
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(json!({
        "ai": true,
        "month": value.get("month").and_then(|v| v.as_str()).unwrap_or(&month),
        "saved": true,
        "resumo": value.get("resumo").cloned().unwrap_or(Value::String(String::new())),
        "destaques": value.get("destaques").cloned().unwrap_or(Value::Array(vec![])),
        "avisos": value.get("avisos").cloned().unwrap_or(Value::Array(vec![])),
        "updated_at": saved.0,
    })))
}

/// Remove the saved digest for a month.
pub async fn digest_delete(
    State(state): State<AppState>,
    Query(q): Query<DigestQuery>,
) -> Result<Json<Value>, AppError> {
    let month = digest_month(&q);
    let from = month_first_day(&month)?;
    sqlx::query("DELETE FROM monthly_digests WHERE month = $1")
        .bind(from)
        .execute(&state.pool)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Build the AI digest for a month (payload + DeepSeek call, recorded in ai_calls).
async fn generate_digest(state: &AppState, month: &str) -> Result<Value, AppError> {
    if !state.ai.enabled() {
        return Ok(json!({ "ai": false }));
    }
    let (from, to, label) = match resolve_range(None, None, Some(month))? {
        (Some(f), Some(t), label) => (f, t, label),
        _ => unreachable!("month always resolves to a range"),
    };

    // How far in the past the analyzed month is (0 = current month), and
    // whether the forecast/recurring context is still relevant: only for the
    // current month or the first 15 days of the next one.
    let today = chrono::Utc::now().date_naive();
    let months_ago = ((today.year() * 12 + today.month() as i32)
        - (from.year() * 12 + from.month() as i32))
        .max(0);
    let previsao_relevante = today <= from + Months::new(1) + chrono::Duration::days(15);

    // Current month aggregates (reuse the dashboard core).
    let dash = dashboard_data(
        &state.pool,
        &DashboardQuery {
            month: Some(month.to_string()),
            date_from: None,
            date_to: None,
            search: None,
            category_ids: None,
            kind: None,
            tags: None,
            bank: None,
            installments: None,
        },
    )
    .await?;

    // Previous month (for the delta).
    let prev_start = from - Months::new(1);
    let prev_end = to - Months::new(1);
    let prev_spend: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CASE WHEN kind='expense' AND status <> 'rejected' THEN -amount_cents ELSE 0 END), 0)::bigint
         FROM items WHERE occurred_on >= $1 AND occurred_on <= $2",
    )
    .bind(prev_start)
    .bind(prev_end)
    .fetch_one(&state.pool)
    .await?;
    let prev_income: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CASE WHEN kind='income' AND status <> 'rejected' THEN amount_cents ELSE 0 END), 0)::bigint
         FROM items WHERE occurred_on >= $1 AND occurred_on <= $2",
    )
    .bind(prev_start)
    .bind(prev_end)
    .fetch_one(&state.pool)
    .await?;

    // Merchants first seen this month.
    let new_merchants: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT merchant FROM items i
         WHERE merchant IS NOT NULL AND merchant <> ''
           AND occurred_on >= $1 AND occurred_on <= $2
           AND NOT EXISTS (SELECT 1 FROM items i2 WHERE i2.merchant = i.merchant AND i2.occurred_on < $1)
         ORDER BY merchant LIMIT 8",
    )
    .bind(from)
    .bind(to)
    .fetch_all(&state.pool)
    .await?;

    // Upcoming obligations (next 30 days) + recurring monthly cost — only
    // relevant for recent months (current month / first 15 days of next).
    let upcoming_total: i64 = if previsao_relevante {
        let upcoming = upcoming_data(&state.pool, 30).await?;
        upcoming.iter().map(|u| u.amount_cents).sum()
    } else {
        0
    };
    let recurring_monthly: i64 = if previsao_relevante {
        sqlx::query_scalar(
            "SELECT COALESCE(SUM(
               CASE
                 WHEN frequency = 'weekly' THEN -amount_cents * 52.0 / 12.0 / GREATEST(interval, 1)
                 WHEN frequency = 'monthly' THEN -amount_cents / GREATEST(interval, 1)
                 ELSE -amount_cents / (12.0 * GREATEST(interval, 1))
               END)::bigint, 0)::bigint
             FROM recurring_rules WHERE is_active AND amount_cents < 0",
        )
        .fetch_one(&state.pool)
        .await?
    } else {
        0
    };

    // Full month history (root items, non-rejected) — the raw material the AI
    // derives insights from (compact fields; amount included so it can compute
    // "maior gasto" without summing).
    let history_rows: Vec<(
        String,
        Option<String>,
        Vec<String>,
        String,
        Option<i32>,
        Option<i32>,
        i64,
    )> = sqlx::query_as(
        "SELECT COALESCE(NULLIF(merchant, ''), description), c.name, i.tags, i.kind,
                    i.installment, i.installment_count, i.amount_cents
             FROM items i LEFT JOIN categories c ON c.id = i.category_id
             WHERE i.status <> 'rejected'
               AND i.occurred_on >= $1 AND i.occurred_on <= $2
             ORDER BY i.occurred_on LIMIT 500",
    )
    .bind(from)
    .bind(to)
    .fetch_all(&state.pool)
    .await?;
    let history: Vec<Value> = history_rows
        .into_iter()
        .map(|(nome, cat, tags, kind, inst, inst_count, amt)| {
            let parcela = match (inst, inst_count) {
                (Some(n), Some(c)) => format!("{n}/{c}"),
                _ => "1/1".to_string(),
            };
            json!({ "nome": nome, "cat": cat, "tags": tags, "tipo": kind, "parcela": parcela, "amt": amt })
        })
        .collect();

    // Active recurring rules: name, amount, category, derived tags, window —
    // only relevant for recent months.
    let mut recorrentes: Vec<Value> = Vec::new();
    if previsao_relevante {
        let rec_rows: Vec<(String, i64, String, i32, Option<String>, Vec<String>)> =
            sqlx::query_as(
                "SELECT r.name, r.amount_cents, r.frequency, r.interval,
                    (SELECT c.name FROM items i JOIN categories c ON c.id = i.category_id
                     WHERE i.recurring_id = r.id ORDER BY i.occurred_on DESC LIMIT 1),
                    COALESCE((SELECT array_agg(DISTINCT t) FROM items i, unnest(i.tags) AS t
                              WHERE i.recurring_id = r.id), '{}')
             FROM recurring_rules r
             WHERE r.is_active AND r.amount_cents < 0 ORDER BY r.name",
            )
            .fetch_all(&state.pool)
            .await?;
        recorrentes = rec_rows
            .into_iter()
            .map(|(nome, amt, freq, interval, cat, tags)| {
                let janela = match freq.as_str() {
                    "weekly" => {
                        if interval > 1 {
                            format!("a cada {interval} semanas")
                        } else {
                            "semanal".to_string()
                        }
                    }
                    "monthly" => {
                        if interval > 1 {
                            format!("a cada {interval} meses")
                        } else {
                            "mensal".to_string()
                        }
                    }
                    _ => {
                        if interval > 1 {
                            format!("a cada {interval} anos")
                        } else {
                            "anual".to_string()
                        }
                    }
                };
                json!({ "nome": nome, "amt": amt, "cat": cat, "tags": tags, "janela": janela })
            })
            .collect();
    }

    // Tag descriptions (F0): what each tag means to the user.
    // Tag descriptions (F0): what each tag means to the user.
    let tag_desc_rows: Vec<(String, String)> =
        sqlx::query_as("SELECT name, description FROM tags WHERE description <> '' ORDER BY name")
            .fetch_all(&state.pool)
            .await?;
    let tag_desc: std::collections::HashMap<String, String> = tag_desc_rows.into_iter().collect();

    // Diary: life notes that explain spending context (all entries — few).
    let diario = crate::services::diary::recent_diary(&state.pool, 20).await?;

    // Previous month's saved digest (if any) — gives the AI continuity.
    let prev_digest: Option<(String, Value, Value)> =
        sqlx::query_as("SELECT resumo, destaques, avisos FROM monthly_digests WHERE month = $1")
            .bind(from - Months::new(1))
            .fetch_optional(&state.pool)
            .await?;
    let digest_anterior = prev_digest.map(|(r, d, a)| {
        json!({
            "resumo": r,
            "destaques": d,
            "avisos": a,
        })
    });

    let mut payload_map = serde_json::Map::new();
    payload_map.insert("mes".into(), json!(label));
    payload_map.insert("meses_atras".into(), json!(months_ago));
    payload_map.insert("previsao_relevante".into(), json!(previsao_relevante));
    payload_map.insert("gasto_total".into(), json!(dash.total_spend_cents));
    payload_map.insert("receita_total".into(), json!(dash.total_income_cents));
    payload_map.insert("gasto_mes_anterior".into(), json!(prev_spend));
    payload_map.insert("receita_mes_anterior".into(), json!(prev_income));
    payload_map.insert("historico".into(), json!(history));
    payload_map.insert("tag_desc".into(), json!(tag_desc));
    payload_map.insert("diario".into(), json!(diario));
    payload_map.insert("digest_anterior".into(), json!(digest_anterior));
    payload_map.insert("novos_mercados".into(), json!(new_merchants));
    if previsao_relevante {
        payload_map.insert("recorrentes".into(), json!(recorrentes));
        payload_map.insert("proximos_30_dias".into(), json!(upcoming_total));
        payload_map.insert("recorrencia_mensal".into(), json!(recurring_monthly));
    }
    let payload = Value::Object(payload_map);

    let system = r#"Você é meu amigo engraçadinho, sádico e muito julgador. Responda APENAS com JSON válido no formato exato:
{
  "resumo": "2-3 frases: total gasto, receitas, comparação com o mês anterior (variação %), deixe algum comentário sarcástico e sádico seu.",
  "destaques": ["2-4 itens: maiores categorias/mercados, coisas notáveis do mês"], use tom de ironia,
  "avisos": ["alertas se relevantes: aumento de gasto, novos mercados com valores altos, compromissos futuros (próximos 30 dias), recorrentes mensais; lista vazia se nada relevante"]
}
Regras: valores em centavos (ex.: 123456 = R$ 1.234,56). Use formatação brasileira. Seja específico (nomes reais).
NUNCA invente o tipo de negócio de um mercado/comerciante (ex.: não diga "o mercado X" ou "o restaurante Y" a menos que a categoria esteja informada no campo "cat" de "historico"). Cite apenas o nome e o valor. Se "cat" estiver presente, pode citá-la; senão, não adivinhe.
"historico" é a lista de transações do mês (nome, cat, tags, tipo, parcela, amt em centavos) — use-a para identificar padrões e maiores gastos. "recorrentes" lista suas regras recorrentes (nome, amt, cat, tags, janela). "tag_desc" explica o significado das tags para o usuário (ex.: "sandero" = gastos com o carro) — respeite esses significados ao comentar. "digest_anterior" (se presente) é o resumo que você gerou no mês passado — use-o para dar continuidade: retome pontos que você levantou, confira se os avisos foram endereçados e compare o que você disse antes com o que aconteceu agora.
TEMPO: você está analisando o mês de "mes", que terminou há "meses_atras" meses (0 = mês atual). Fale no tempo passado sobre esse mês, como se estivesse naquela época. Se "previsao_relevante" for false (mês já distante), os campos "recorrentes", "proximos_30_dias" e "recorrencia_mensal" NÃO existem e não são relevantes — não os cite nem especule sobre o futuro; foque apenas no mês analisado. Se for true (mês atual ou primeiros 15 dias do seguinte), pode usá-los."#;
    let user = json!(payload.to_string());
    let value = state
        .ai
        .chat_json(system, user, state.ai.text_model(), None, "digest")
        .await
        .map_err(AppError::from)?;

    Ok(json!({
        "ai": true,
        "month": label,
        "resumo": value.get("resumo").and_then(|v| v.as_str()).unwrap_or(""),
        "destaques": value.get("destaques").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
        "avisos": value.get("avisos").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
    }))
}

pub async fn trend(
    State(state): State<AppState>,
    Query(q): Query<TrendQuery>,
) -> Result<Json<Vec<TrendPoint>>, AppError> {
    Ok(Json(trend_data(&state.pool, &q).await?))
}

/// 12-month (configurable) spend/income history. The window ends at the month of
/// `date_to` (default: current month) and respects the non-date filters. `from`
/// is NOT used — the trend is always a rolling window, so it shows history even
/// when the range is a single month.
pub async fn trend_data(pool: &PgPool, q: &TrendQuery) -> Result<Vec<TrendPoint>, AppError> {
    let months = q.months.unwrap_or(12).clamp(1, 36);
    let category_ids = split_filters(&q.category_ids);
    let tags = split_filters(&q.tags);

    let today = chrono::Utc::now().date_naive();
    let end_month = match q.date_to {
        Some(d) => NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap(),
        None => NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(),
    };
    let start = end_month - Months::new((months - 1) as u32);
    let end = end_month + Months::new(1) - chrono::Duration::days(1); // inclusive last day

    let rows: Vec<(String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT to_char({BUCKET_DATE}, 'YYYY-MM'),
                COALESCE(SUM({NET_AMOUNT}), 0)::bigint,
                COALESCE(SUM(CASE WHEN items.kind = 'income' THEN {AGG_AMOUNT_ADJ} ELSE 0 END), 0)::bigint
         FROM items
         {REFUND_JOIN}
         WHERE {AGG_FILTERS}
         GROUP BY 1
         ORDER BY 1"
    )))
    .bind(start)
    .bind(end)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
    .await?;

    let mut map = std::collections::HashMap::new();
    for (m, s, i) in rows {
        map.insert(m, (s, i));
    }

    let mut out = Vec::with_capacity(months as usize);
    for k in 0..months {
        let d = start + Months::new(k as u32);
        let key = d.format("%Y-%m").to_string();
        let (s, i) = map.get(&key).copied().unwrap_or((0, 0));
        out.push(TrendPoint {
            month: key,
            spend_cents: s,
            income_cents: i,
        });
    }

    Ok(out)
}

// ---------- Daily / tags (expenses only) ----------

/// Filters for the daily and top-tags aggregations (no month param — the
/// frontend passes an explicit range; None = unbounded).
#[derive(Debug, Deserialize)]
pub struct DailyQuery {
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
    pub search: Option<String>,
    pub category_ids: Option<String>,
    pub kind: Option<String>,
    pub tags: Option<String>,
    pub bank: Option<String>,
    pub installments: Option<String>,
    /// 'category' (per day per category) or 'none' (plain daily totals).
    #[serde(default = "default_stack_by")]
    pub stack_by: String,
}

fn default_stack_by() -> String {
    "category".to_string()
}

#[derive(Debug, Serialize, FromRow)]
pub struct DailyPoint {
    pub date: NaiveDate,
    /// Category name when `stack_by=category`, else NULL.
    pub key: Option<String>,
    pub total_cents: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct TagTotal {
    pub tag: String,
    pub total_cents: i64,
}

pub async fn daily(
    State(state): State<AppState>,
    Query(q): Query<DailyQuery>,
) -> Result<Json<Vec<DailyPoint>>, AppError> {
    Ok(Json(daily_data(&state.pool, &q).await?))
}

/// Daily expense totals (roots only, rejected excluded). `stack_by=category`
/// buckets each day by category name; `none` returns one row per day. The `kind`
/// filter still applies on top of the hard `kind = 'expense'` (so a kind=income
/// filter yields an empty chart, which is honest feedback).
pub async fn daily_data(pool: &PgPool, q: &DailyQuery) -> Result<Vec<DailyPoint>, AppError> {
    let category_ids = split_filters(&q.category_ids);
    let tags = split_filters(&q.tags);
    let rows = sqlx::query_as::<_, DailyPoint>(sqlx::AssertSqlSafe(format!(
        "SELECT {BUCKET_DATE} AS date,
                CASE WHEN $9 = 'category' THEN COALESCE(c.name, 'Sem categoria') END AS key,
                SUM(-{AGG_AMOUNT_ADJ})::bigint AS total_cents
         FROM items
         {REFUND_JOIN}
         LEFT JOIN categories c ON c.id = {BUCKET_CAT}
         WHERE {EXPENSE_OR_REFUND} AND {AGG_FILTERS}
         GROUP BY {BUCKET_DATE}, key
         ORDER BY {BUCKET_DATE}, key"
    )))
    .bind(q.date_from)
    .bind(q.date_to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .bind(&q.stack_by)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn tags(
    State(state): State<AppState>,
    Query(q): Query<DailyQuery>,
) -> Result<Json<Vec<TagTotal>>, AppError> {
    Ok(Json(tags_data(&state.pool, &q).await?))
}

/// Top tags by expense total. Each tag counts the FULL amount of its items
/// (overlap allowed — that's the point: "spend carrying this tag").
pub async fn tags_data(pool: &PgPool, q: &DailyQuery) -> Result<Vec<TagTotal>, AppError> {
    let category_ids = split_filters(&q.category_ids);
    let tags = split_filters(&q.tags);
    let rows = sqlx::query_as::<_, TagTotal>(sqlx::AssertSqlSafe(format!(
        "SELECT tag, SUM(-{AGG_AMOUNT_ADJ})::bigint AS total_cents
         FROM items
         {REFUND_JOIN}
         CROSS JOIN LATERAL unnest({BUCKET_TAGS}) AS tag
         WHERE {EXPENSE_OR_REFUND} AND {AGG_FILTERS}
         GROUP BY tag
         ORDER BY total_cents DESC
         LIMIT 10"
    )))
    .bind(q.date_from)
    .bind(q.date_to)
    .bind(&q.search)
    .bind(&category_ids)
    .bind(&q.kind)
    .bind(&tags)
    .bind(&q.bank)
    .bind(&q.installments)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------- Expected spend (future forecast) ----------

#[derive(Debug, Deserialize)]
pub struct ExpectedQuery {
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct ExpectedSpend {
    pub installments_cents: i64,
    pub recurring_cents: i64,
    pub total_cents: i64,
}

/// `limit`, inclusive. Shared by the expected KPI, the monthly forecast and the
/// upcoming feed.
async fn future_events(
    pool: &PgPool,
    today: NaiveDate,
    limit: NaiveDate,
) -> Result<(Vec<(NaiveDate, i64)>, Vec<(NaiveDate, i64)>), AppError> {
    // Future parcels of in-progress series: parcel k+1 in month M+1 after the
    // latest billed parcel (month M), at the latest parcel amount.
    let rows: Vec<(i32, i32, NaiveDate, i64)> = sqlx::query_as(
        "SELECT s.installment_count, st.max_inst, st.last_date, p.amount_cents
         FROM purchase_series s
         JOIN LATERAL (
           SELECT MAX(i.installment)::int AS max_inst, MAX(i.occurred_on) AS last_date
           FROM items i WHERE i.series_id = s.id
         ) st ON true
         JOIN LATERAL (
           SELECT i.amount_cents FROM items i
           WHERE i.series_id = s.id ORDER BY i.installment DESC, i.occurred_on DESC LIMIT 1
         ) p ON true
         WHERE st.max_inst IS NOT NULL AND st.max_inst < s.installment_count",
    )
    .fetch_all(pool)
    .await?;

    let mut installments = Vec::new();
    for (count, max_inst, last_date, amount_cents) in rows {
        if amount_cents >= 0 {
            continue;
        }
        for k in (max_inst + 1)..=count {
            let months = (k - max_inst) as u32;
            let Some(d) = last_date.checked_add_months(chrono::Months::new(months)) else {
                continue;
            };
            if d >= today && d <= limit {
                installments.push((d, amount_cents.unsigned_abs() as i64));
            }
        }
    }

    // Future occurrences of active expense rules, anchored at next_due_on
    // (advanced to >= today), stepped by the rule's window.
    let rules: Vec<(i64, String, i32, NaiveDate)> = sqlx::query_as(
        "SELECT amount_cents, frequency, interval, next_due_on
         FROM recurring_rules WHERE is_active AND amount_cents < 0 AND next_due_on IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let mut recurring = Vec::new();
    for (amount_cents, frequency, interval, next_due) in rules {
        let mut d =
            crate::services::recurring::advance_next_due(next_due, &frequency, interval, today);
        for _ in 0..1200 {
            if d > limit {
                break;
            }
            if d >= today {
                recurring.push((d, amount_cents.unsigned_abs() as i64));
            }
            d = step_occurrence(d, &frequency, interval);
        }
    }

    Ok((installments, recurring))
}

fn step_occurrence(d: NaiveDate, frequency: &str, interval: i32) -> NaiveDate {
    let interval = interval.max(1) as u32;
    match frequency {
        "weekly" => d + chrono::Duration::days(7 * interval as i64),
        "monthly" => d
            .checked_add_months(chrono::Months::new(interval))
            .unwrap_or(d),
        _ => d
            .checked_add_months(chrono::Months::new(interval * 12))
            .unwrap_or(d),
    }
}

// ---------- Monthly forecast + upcoming feed ----------

#[derive(Debug, Deserialize)]
pub struct ForecastQuery {
    pub months: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct ForecastPoint {
    pub month: String,
    pub installments_cents: i64,
    pub recurring_cents: i64,
    pub total_cents: i64,
}

/// `GET /dashboard/forecast?months=N` — expected spend per month for the next
/// N months (parcels + recurrences), starting with the current month. The
/// current month's bucket only holds obligations still ahead (dates >= today).
/// Filter-free; expenses only.
pub async fn forecast(
    State(state): State<AppState>,
    Query(q): Query<ForecastQuery>,
) -> Result<Json<Vec<ForecastPoint>>, AppError> {
    Ok(Json(
        forecast_data(&state.pool, q.months.unwrap_or(3)).await?,
    ))
}

pub async fn forecast_data(pool: &PgPool, months: i32) -> Result<Vec<ForecastPoint>, AppError> {
    let months = months.clamp(1, 24);
    let today = chrono::Utc::now().date_naive();
    // The forecast starts with the CURRENT month: its bucket only receives
    // future events (dates >= today), so it shows what's still expected for
    // the month, and the following buckets are full months.
    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let last_first = first
        .checked_add_months(chrono::Months::new(months as u32 - 1))
        .unwrap();
    let limit = last_first + chrono::Months::new(1) - chrono::Duration::days(1);

    let (installments, recurring) = future_events(pool, today, limit).await?;

    let mut out: Vec<ForecastPoint> = (0..months as u32)
        .map(|k| {
            let m = first.checked_add_months(chrono::Months::new(k)).unwrap();
            ForecastPoint {
                month: m.format("%Y-%m").to_string(),
                installments_cents: 0,
                recurring_cents: 0,
                total_cents: 0,
            }
        })
        .collect();
    let bucket = |d: NaiveDate| -> Option<usize> {
        let idx = (d.year() * 12 + d.month() as i32) - (first.year() * 12 + first.month() as i32);
        (0..months).contains(&idx).then_some(idx as usize)
    };
    for (d, a) in installments {
        if let Some(i) = bucket(d) {
            out[i].installments_cents += a;
        }
    }
    for (d, a) in recurring {
        if let Some(i) = bucket(d) {
            out[i].recurring_cents += a;
        }
    }
    for p in &mut out {
        p.total_cents = p.installments_cents + p.recurring_cents;
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
pub struct UpcomingQuery {
    pub days: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct UpcomingItem {
    pub date: NaiveDate,
    /// 'parcel' | 'recurring'
    pub kind: String,
    pub description: String,
    pub category_name: Option<String>,
    pub amount_cents: i64,
    /// "3/12" for parcels, null for recurrences.
    pub progress: Option<String>,
}

/// `GET /dashboard/upcoming?days=N` — flat, dated feed of the next obligations
/// (future parcels + recurring occurrences), sorted by date.
pub async fn upcoming(
    State(state): State<AppState>,
    Query(q): Query<UpcomingQuery>,
) -> Result<Json<Vec<UpcomingItem>>, AppError> {
    Ok(Json(
        upcoming_data(&state.pool, q.days.unwrap_or(90)).await?,
    ))
}

pub async fn upcoming_data(pool: &PgPool, days: i64) -> Result<Vec<UpcomingItem>, AppError> {
    let today = chrono::Utc::now().date_naive();
    let limit = today + chrono::Duration::days(days.clamp(1, 3650));
    let mut out = Vec::new();

    // Parcels, with description + category + progress from the latest billed parcel.
    let rows: Vec<(i32, i32, NaiveDate, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT s.installment_count, st.max_inst, st.last_date, p.amount_cents,
                p.description, c.name
         FROM purchase_series s
         JOIN LATERAL (
           SELECT MAX(i.installment)::int AS max_inst, MAX(i.occurred_on) AS last_date
           FROM items i WHERE i.series_id = s.id
         ) st ON true
         JOIN LATERAL (
           SELECT i.amount_cents, i.description, i.category_id FROM items i
           WHERE i.series_id = s.id ORDER BY i.installment DESC, i.occurred_on DESC LIMIT 1
         ) p ON true
         LEFT JOIN categories c ON c.id = p.category_id
         WHERE st.max_inst IS NOT NULL AND st.max_inst < s.installment_count",
    )
    .fetch_all(pool)
    .await?;
    for (count, max_inst, last_date, amount_cents, description, category_name) in rows {
        if amount_cents >= 0 {
            continue;
        }
        for k in (max_inst + 1)..=count {
            let months = (k - max_inst) as u32;
            let Some(d) = last_date.checked_add_months(chrono::Months::new(months)) else {
                continue;
            };
            if d > limit {
                break;
            }
            if d >= today {
                out.push(UpcomingItem {
                    date: d,
                    kind: "parcel".to_string(),
                    description: description.clone(),
                    category_name: category_name.clone(),
                    amount_cents: amount_cents.unsigned_abs() as i64,
                    progress: Some(format!("{k}/{count}")),
                });
            }
        }
    }

    // Recurrences: rule name + category from its latest linked item.
    let rules: Vec<(i64, String, i32, NaiveDate, String, Option<String>)> = sqlx::query_as(
        "SELECT r.amount_cents, r.frequency, r.interval, r.next_due_on, r.name, c.name
         FROM recurring_rules r
         LEFT JOIN LATERAL (
           SELECT c2.name FROM items i JOIN categories c2 ON c2.id = i.category_id
           WHERE i.recurring_id = r.id ORDER BY i.occurred_on DESC LIMIT 1
         ) c ON true
         WHERE r.is_active AND r.amount_cents < 0 AND r.next_due_on IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    for (amount_cents, frequency, interval, next_due, name, category_name) in rules {
        let mut d =
            crate::services::recurring::advance_next_due(next_due, &frequency, interval, today);
        for _ in 0..1200 {
            if d > limit {
                break;
            }
            if d >= today {
                out.push(UpcomingItem {
                    date: d,
                    kind: "recurring".to_string(),
                    description: name.clone(),
                    category_name: category_name.clone(),
                    amount_cents: amount_cents.unsigned_abs() as i64,
                    progress: None,
                });
            }
            d = step_occurrence(d, &frequency, interval);
        }
    }

    out.sort_by_key(|i| i.date);
    Ok(out)
}
