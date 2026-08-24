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
}

#[derive(Debug, Deserialize)]
pub struct ItemInput {
    pub parent_id: Option<Uuid>,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub account_id: Option<Uuid>,
    pub installment: Option<i32>,
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

// ---------- Documents ----------

/// Full document row (used internally by the worker).
#[derive(Debug, Clone, FromRow)]
pub struct DocumentRow {
    pub id: Uuid,
    pub kind: String,
    pub account_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub filename: String,
    pub content_type: String,
    pub sha256: String,
    pub file_path: String,
    pub status: String,
    pub error_message: Option<String>,
    pub ocr_text: Option<String>,
    pub uploaded_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

/// List item for documents (no heavy `ocr_text`).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct DocumentSummary {
    pub id: Uuid,
    pub kind: String,
    pub filename: String,
    pub content_type: String,
    pub status: String,
    pub error_message: Option<String>,
    pub uploaded_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    pub item_count: i64,
    pub source_id: Option<Uuid>,
}

/// Full document detail for the API.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentDetail {
    pub id: Uuid,
    pub kind: String,
    pub filename: String,
    pub content_type: String,
    pub status: String,
    pub error_message: Option<String>,
    pub uploaded_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    pub ocr_text: Option<String>,
    pub items: Vec<Item>,
    pub source_id: Option<Uuid>,
}

// ---------- Accounts ----------

// ---------- Matches (receipt → statement links) ----------

#[derive(Debug, Clone, Serialize)]
pub struct MatchDetail {
    pub id: Uuid,
    pub parent_item_id: Uuid,
    pub child_item_id: Uuid,
    pub source: String,
    pub confidence: f32,
    pub status: String,
    pub parent: Item,
    pub child: Item,
}

// ---------- Sources ----------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Source {
    pub id: Uuid,
    pub bank: String,
    pub kind: String,
    pub name: String,
    pub enabled: bool,
    pub account_id: Option<Uuid>,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
}

// ---------- Merchant memory ----------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub merchant: String,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub confidence: f32,
    pub confirm_count: i32,
    pub last_confirmed_at: Option<DateTime<Utc>>,
}
