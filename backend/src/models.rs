use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ---------- Categories ----------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Category {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
pub struct NewCategory {
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCategory {
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub is_active: bool,
}

// ---------- Tags ----------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TagUsage {
    pub tag: String,
    /// Number of items carrying this tag.
    pub count: i64,
}

// ---------- Items ----------

fn default_kind() -> String {
    "expense".to_string()
}
fn default_currency() -> String {
    "BRL".to_string()
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Item {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub document_id: Option<Uuid>,
    pub source: String,
    pub kind: String,
    pub status: String,
    pub account_id: Option<Uuid>,
    pub transfer_group_id: Option<Uuid>,
    pub installment: Option<i32>,
    pub installment_count: Option<i32>,
    pub recurring_id: Option<Uuid>,
    pub occurred_on: NaiveDate,
    pub posted_on: Option<NaiveDate>,
    pub merchant: Option<String>,
    pub description: String,
    pub amount_cents: i64,
    pub currency: String,
    pub category_id: Option<Uuid>,
    pub suggested_category: Option<String>,
    pub tags: Vec<String>,
    pub raw_line: Option<String>,
    pub match_confidence: Option<f32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Bank slug (nubank|caixa|c6) — derived from the source document or the
    /// Pluggy account, populated by item queries.
    pub bank: Option<String>,
    /// Display label of the source account ("Nubank - Cartão", …).
    pub source_label: Option<String>,
    /// Pluggy transaction id — `NULL` for legacy document items not yet merged.
    pub external_id: Option<String>,
    /// The expense this refund reverses (`kind = 'refund'` → charge id). Graphs
    /// net linked refunds against their charge.
    pub refunded_item_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ItemInput {
    pub parent_id: Option<Uuid>,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub account_id: Option<Uuid>,
    #[serde(default)]
    pub installment: Option<i32>,
    #[serde(default)]
    pub installment_count: Option<i32>,
    pub occurred_on: NaiveDate,
    pub merchant: Option<String>,
    pub description: String,
    pub amount_cents: i64,
    #[serde(default = "default_currency")]
    pub currency: String,
    pub category_id: Option<Uuid>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Filtered summary for the items list (`GET /api/items/summary`).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ItemSummary {
    /// Number of root items matching the filters.
    pub count: i64,
    /// Net sum of `amount_cents` (negative = expense).
    pub total_cents: i64,
}

// ---------- Bulk item updates ----------

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagsMode {
    Replace,
    Add,
    Remove,
}

/// Bulk edit payload for `PATCH /items/bulk`. Omitted fields keep their value:
/// - `kind`: omitted = keep current kind
/// - `category_id`: `None` = keep, `Some(None)` = clear, `Some(Some(id))` = set
/// - `tags`: omitted = keep; `tags_mode` picks replace/add/remove (default replace)
#[derive(Debug, Deserialize)]
pub struct BulkItemUpdate {
    pub ids: Vec<Uuid>,
    pub kind: Option<String>,
    pub category_id: Option<Option<Uuid>>,
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub tags_mode: Option<TagsMode>,
}

// ---------- Accounts ----------

// ---------- Merchant memory ----------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub merchant: String,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    /// Tags remembered for this merchant (accumulated over confirmations).
    pub tags: Vec<String>,
    pub confidence: f32,
    pub confirm_count: i32,
    pub last_confirmed_at: Option<DateTime<Utc>>,
}

// ---------- AI tag batches ----------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AiTagBatch {
    pub id: Uuid,
    pub status: String,
    pub error_message: Option<String>,
    pub item_count: i64,
    pub created_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    /// 'tags' | 'categorize' — which proposal flow this batch runs.
    pub kind: String,
}

/// A tag suggestion joined with the item it refers to (for the review UI).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SuggestionDetail {
    pub id: Uuid,
    pub batch_id: Uuid,
    /// Status of the owning batch ('done' when the suggestions are reviewable).
    pub batch_status: String,
    /// 'tags' | 'categorize'
    pub batch_kind: String,
    pub item_id: Uuid,
    pub suggested_tags: Vec<String>,
    /// Category proposal for 'categorize' batches ('' when none) — an existing
    /// name or "nova: <nome>".
    pub suggested_category: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    // Item columns (joined in).
    pub merchant: Option<String>,
    pub description: String,
    pub amount_cents: i64,
    pub occurred_on: NaiveDate,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub tags: Vec<String>,
    pub document_id: Option<Uuid>,
    // Pluggy enrichment (joined in).
    pub pluggy_category: Option<String>,
    pub mcc: Option<i64>,
    pub operation_type: Option<String>,
    pub payment_method: Option<String>,
}

// ---------- Recurring rules ----------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RecurringRule {
    pub id: Uuid,
    /// Free-form label — plays no role in matching.
    pub name: String,
    pub amount_cents: i64,
    pub currency: String,
    /// Derived from the most recent linked confirmed item (like tags) — rules no
    /// longer carry their own category.
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub frequency: String,
    pub interval: i32,
    pub day_of_month: Option<i32>,
    /// Effective next due date (never in the past) — computed at read time.
    pub next_due_on: Option<NaiveDate>,
    pub is_active: bool,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Auto-match names (normalized exact equality against item merchant/description).
    pub aliases: Vec<String>,
    /// One-shot manual references (no auto-match; linked at save time).
    pub isolated_cases: Vec<String>,
    /// Derived from linked confirmed items (union of their tags).
    pub tags: Vec<String>,
    /// True when linked items carry more than one distinct non-empty tag set.
    pub tags_conflict: bool,
    /// Days until the effective next due date (negative only if already past —
    /// shouldn't happen; read-time advance keeps it >= 0).
    pub days_until: Option<i64>,
}

// ---------- Diary ----------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct DiaryEntry {
    pub id: Uuid,
    pub entry_date: chrono::NaiveDate,
    pub comment: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct DiaryInput {
    pub entry_date: chrono::NaiveDate,
    pub comment: String,
}
