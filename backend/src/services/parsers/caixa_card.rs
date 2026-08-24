use anyhow::{anyhow, Result};
use chrono::{Datelike, NaiveDate};

use super::csv::parse_amount;
use super::ParsedItem;

/// Does this PDF text look like a Caixa *credit card* fatura (as opposed to a
/// Caixa checking-account statement)?
pub fn is_caixa_card_fatura(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("cartões caixa")
        || lower.contains("cartoes caixa")
        || lower.contains("compras (cartão")
        || lower.contains("compras (cartao")
}

/// Parse a Caixa credit-card fatura. Returns the billing month (first day)
/// and the purchase items found under the `COMPRAS (Cartão …)` table.
pub fn parse(text: &str) -> Result<(NaiveDate, Vec<ParsedItem>)> {
    let billing = find_vencimento(text)
        .map(|d| NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap())
        .ok_or_else(|| anyhow!("Caixa card fatura: VENCIMENTO not found"))?;
    let items = parse_compras(text, billing);
    Ok((billing, items))
}

fn find_vencimento(text: &str) -> Option<NaiveDate> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.to_lowercase().contains("vencimento") {
            continue;
        }
        for j in i..lines.len().min(i + 5) {
            if let Some(d) = extract_date_dd_mm_yyyy(lines[j]) {
                return Some(d);
            }
        }
    }
    None
}

fn extract_date_dd_mm_yyyy(s: &str) -> Option<NaiveDate> {
    for word in s.split_whitespace() {
        if let Ok(d) = NaiveDate::parse_from_str(word, "%d/%m/%Y") {
            return Some(d);
        }
    }
    None
}

fn parse_compras(text: &str, billing: NaiveDate) -> Vec<ParsedItem> {
    let mut items = Vec::new();

    let marker = text
        .lines()
        .position(|l| {
            let l = l.to_lowercase();
            l.contains("compras (cartão") || l.contains("compras (cartao")
        })
        .map(|i| i);
    let Some(marker_idx) = marker else {
        return items;
    };

    for line in text.lines().skip(marker_idx + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.starts_with("total") {
            break;
        }
        if lower.starts_with("data descrição") || lower.starts_with("data descricao") {
            continue;
        }
        if let Some(item) = parse_compras_line(trimmed, billing) {
            items.push(item);
        }
    }

    items
}

fn parse_compras_line(line: &str, billing: NaiveDate) -> Option<ParsedItem> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }
    let ddmm = tokens[0];
    if !ddmm.contains('/') {
        return None;
    }
    let amount_token = *tokens.last()?;
    if !(amount_token.ends_with('D') || amount_token.ends_with('C')) {
        return None;
    }
    let (cents, is_credit) = parse_amount_suffix(amount_token)?;
    let occurred_on = infer_date(ddmm, billing)?;
    let description = tokens[1..tokens.len() - 1].join(" ");

    Some(ParsedItem {
        occurred_on,
        description: description.clone(),
        merchant: Some(description),
        amount_cents: if is_credit { cents } else { -cents },
        kind: if is_credit { "income".into() } else { "expense".into() },
        category: None,
        installment: None,
        installment_count: None,
        tags: vec![],
    })
}

/// "12,90D" -> (1290, false); "5,00C" -> (500, true).
fn parse_amount_suffix(token: &str) -> Option<(i64, bool)> {
    let (num, suffix) = token.split_at(token.len() - 1);
    let is_credit = suffix == "C";
    let (cents, _negative) = parse_amount(num).ok()?;
    Some((cents, is_credit))
}

/// Infer the full date from a "DD/MM" purchase date, using the fatura's
/// billing month. If the purchase month is after the billing month, it belongs
/// to the previous year (e.g. a January fatura containing December purchases).
pub fn infer_date(ddmm: &str, billing: NaiveDate) -> Option<NaiveDate> {
    let (day, month) = ddmm.split_once('/')?;
    let day: u32 = day.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    let year = if month > billing.month() {
        billing.year() - 1
    } else {
        billing.year()
    };
    NaiveDate::from_ymd_opt(year, month, day)
}
