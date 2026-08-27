pub mod caixa_card;
pub mod csv;

use chrono::NaiveDate;

/// A canonicalized line item parsed from a bank document (CSV or PDF).
#[derive(Debug, Clone)]
pub struct ParsedItem {
    pub occurred_on: NaiveDate,
    /// Original purchase date, when the source document carries it (C6 fatura,
    /// Caixa fatura). Used to disambiguate identical installment purchases — it
    /// is NOT persisted on items, only used at parse/series time.
    pub purchase_date: Option<NaiveDate>,
    pub description: String,
    pub merchant: Option<String>,
    /// Signed cents (negative = expense / outflow).
    pub amount_cents: i64,
    /// 'expense' | 'income' | 'refund' | 'internal'
    pub kind: String,
    /// Mapped category name (already matched to our category tree).
    pub category: Option<String>,
    pub installment: Option<i32>,
    pub installment_count: Option<i32>,
    pub tags: Vec<String>,
}
