pub mod caixa_card;
pub mod csv;

use chrono::NaiveDate;

/// A canonicalized line item parsed from a bank document (CSV or PDF).
#[derive(Debug, Clone)]
pub struct ParsedItem {
    pub occurred_on: NaiveDate,
    pub description: String,
    pub merchant: Option<String>,
    /// Signed cents (negative = expense / outflow).
    pub amount_cents: i64,
    /// 'expense' | 'income' | 'refund' | 'card_payment' | 'investment'
    pub kind: String,
    /// Mapped category name (already matched to our category tree).
    pub category: Option<String>,
    pub installment: Option<i32>,
    pub installment_count: Option<i32>,
    pub tags: Vec<String>,
}
