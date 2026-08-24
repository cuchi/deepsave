use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;

use super::ParsedItem;
use crate::services::tags::strip_accents;

/// Detect the bank format and parse the CSV content.
/// `billing_month` (first day of the fatura's billing month) is used as the
/// occurred_on date for installment rows, since "Data de Compra" is the
/// *original* purchase date, not when the installment is actually charged.
pub fn parse_csv(content: &str, billing_month: Option<NaiveDate>) -> Result<Vec<ParsedItem>> {
    let content = content.trim_start_matches('\u{feff}');

    // C6 bank statement: metadata preamble before the CSV header.
    if content.to_lowercase().contains("extrato de conta corrente") {
        return parse_c6_bank(content);
    }

    let first_line = content.lines().next().unwrap_or("").to_string();
    let delimiter = if first_line.contains(';') { b';' } else { b',' };
    let header = first_line.to_lowercase();

    if header.contains("data de compra") {
        parse_c6_invoice(content, delimiter, billing_month)
    } else if header.contains("identificador") && header.contains("valor") {
        parse_nubank_account(content, delimiter)
    } else if header.contains("title") || header.contains("date") {
        parse_nubank_card(content, delimiter)
    } else {
        Err(anyhow!("unrecognized CSV format (header: {first_line})"))
    }
}

// Nubank credit card: `date,title,amount`
//   - date: YYYY-MM-DD
//   - amount: comma decimal, quoted; negative = money received (income)
fn parse_nubank_card(content: &str, delimiter: u8) -> Result<Vec<ParsedItem>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_reader(content.as_bytes());
    let mut out = Vec::new();

    for rec in rdr.records() {
        let rec = rec.context("bad CSV record")?;
        if rec.len() < 3 {
            continue;
        }
        let date = NaiveDate::parse_from_str(rec[0].trim(), "%Y-%m-%d")
            .map_err(|e| anyhow!("bad date '{}': {e}", &rec[0]))?;
        let title = rec[1].trim().to_string();
        let (cents, negative) = parse_amount(rec[2].trim())?;

        if negative {
            out.push(ParsedItem {
                occurred_on: date,
                description: title.clone(),
                merchant: Some(title.clone()),
                amount_cents: cents,
                kind: "income".into(),
                category: None,
                installment: None,
                installment_count: None,
                tags: vec![],
            });
        } else {
            out.push(ParsedItem {
                occurred_on: date,
                description: title.clone(),
                merchant: Some(title),
                amount_cents: -cents,
                kind: "expense".into(),
                category: None,
                installment: None,
                installment_count: None,
                tags: vec![],
            });
        }
    }
    Ok(out)
}

// Nubank checking account: `Data,Valor,Identificador,Descrição`
//   - Data: DD/MM/YYYY
//   - Valor: dot decimal, signed (negative = outflow)
//   - Descrição: rich Pix/boleto text, used to classify the kind
fn parse_nubank_account(content: &str, delimiter: u8) -> Result<Vec<ParsedItem>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_reader(content.as_bytes());
    let mut out = Vec::new();

    for rec in rdr.records() {
        let rec = rec.context("bad CSV record")?;
        if rec.len() < 4 {
            continue;
        }
        let date = NaiveDate::parse_from_str(rec[0].trim(), "%d/%m/%Y")
            .map_err(|e| anyhow!("bad date '{}': {e}", &rec[0]))?;
        let (cents, negative) = parse_amount(rec[1].trim())?;
        let description = rec[3].trim().to_string();
        let lower = description.to_lowercase();

        let kind = if lower.contains("estorno") {
            "refund"
        } else if lower.contains("pagamento fatura")
            || lower.contains("pagamento de fatura")
            || lower.contains("pagamento da fatura")
            || lower.contains("pagamento cartão")
            || lower.contains("pagamento cartao")
        {
            "card_payment"
        } else if lower.contains("pagamento recebido") {
            "income"
        } else if negative {
            "expense"
        } else {
            "income"
        };

        let amount_cents = if negative { -cents } else { cents };
        out.push(ParsedItem {
            occurred_on: date,
            description: description.clone(),
            merchant: extract_nubank_merchant(&description),
            amount_cents,
            kind: kind.into(),
            category: None,
            installment: None,
            installment_count: None,
            tags: vec![],
        });
    }
    Ok(out)
}

/// Extract a merchant name from Nubank account descriptions like
/// "Transferência enviada pelo Pix - TIM S A - 02.421.421/0001-11 - ITAÚ...".
/// The merchant is the segment right after the action label (or after the
/// action label + original action for estornos). The full description is kept
/// verbatim; this is just a short display-friendly name.
fn extract_nubank_merchant(description: &str) -> Option<String> {
    let parts: Vec<&str> = description.split(" - ").collect();
    let idx = if parts
        .first()
        .is_some_and(|p| p.to_lowercase().contains("estorno"))
    {
        2
    } else {
        1
    };
    let candidate = parts.get(idx)?.trim();
    if candidate.is_empty() {
        return None;
    }
    let digits: String = candidate.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 14 {
        return None; // looks like a CNPJ
    }
    Some(candidate.to_string())
}

// C6 credit-card invoice: semicolon-separated
//   `Data de Compra;Nome no Cartão;Final do Cartão;Categoria;Descrição;Parcela;Valor (em US$);Cotação (em R$);Valor (em R$)`
fn parse_c6_invoice(
    content: &str,
    delimiter: u8,
    billing_month: Option<NaiveDate>,
) -> Result<Vec<ParsedItem>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_reader(content.as_bytes());
    let mut out = Vec::new();

    for rec in rdr.records() {
        let rec = rec.context("bad CSV record")?;
        if rec.len() < 9 {
            continue;
        }
        let purchase_date = NaiveDate::parse_from_str(rec[0].trim(), "%d/%m/%Y")
            .map_err(|e| anyhow!("bad date '{}': {e}", &rec[0]))?;
        let category = rec[3].trim().to_string();
        let description = rec[4].trim().to_string();
        let (installment, installment_count) = parse_parcela(rec[5].trim());
        let (cents, negative) = parse_amount(rec[8].trim())?;
        if cents == 0 {
            continue;
        }

        // Installments are charged in the fatura month, not on "Data de Compra".
        let occurred_on = match installment_count {
            Some(n) if n > 1 => billing_month.unwrap_or(purchase_date),
            _ => purchase_date,
        };

        // Negative Valor(em R$) is a credit on the card: a refund/reversal
        // (or a payment included in the bill, which is neutral).
        let (amount_cents, kind) = if negative {
            let dl = description.to_lowercase();
            if dl.contains("inclusao de pagamento") || dl.contains("pagamento fatura") {
                (-cents, "card_payment")
            } else {
                (cents, "refund")
            }
        } else {
            (-cents, "expense")
        };

        out.push(ParsedItem {
            occurred_on,
            description: description.clone(),
            merchant: Some(description),
            amount_cents,
            kind: kind.into(),
            category: map_c6_category(&category),
            installment,
            installment_count,
            tags: vec![],
        });
    }
    Ok(out)
}

/// Map a C6 fatura "Categoria" string to one of our category names.
fn map_c6_category(raw: &str) -> Option<String> {
    let r = strip_accents(raw).to_lowercase();
    if r.contains("supermercado") || r.contains("mercearia") || r.contains("padaria") || r.contains("conveniencia") {
        Some("Supermercado".to_string())
    } else if r.contains("transporte") || r.contains("automotivo") || r.contains("combustivel") {
        Some("Transporte".to_string())
    } else if r.contains("saude") || r.contains("medica") || r.contains("odontolog") || r.contains("farmacia") {
        Some("Saúde".to_string())
    } else if r.contains("moradia") || r.contains("mobiliario") || r.contains("construcao") || r.contains("casa") {
        Some("Moradia".to_string())
    } else if r.contains("lazer") || r.contains("recreativo") || r.contains("entretenimento") {
        Some("Lazer".to_string())
    } else if r.contains("restaurante") || r.contains("alimentacao") {
        Some("Restaurantes".to_string())
    } else if r.contains("assinatura") || r.contains("streaming") {
        Some("Assinaturas".to_string())
    } else {
        None
    }
}

// C6 checking account: metadata preamble + `Data Lançamento,Data Contábil,Título,Descrição,Entrada(R$),Saída(R$),Saldo do Dia(R$)`
//   - dates DD/MM/YYYY; separate Entrada (in) / Saída (out) columns, dot decimals.
fn parse_c6_bank(content: &str) -> Result<Vec<ParsedItem>> {
    let header_idx = content
        .lines()
        .position(|l| l.to_lowercase().contains("data lançamento"))
        .ok_or_else(|| anyhow!("C6 bank CSV: header not found"))?;

    let csv_part = content
        .lines()
        .skip(header_idx)
        .collect::<Vec<_>>()
        .join("\n");

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b',')
        .from_reader(csv_part.as_bytes());
    let mut out = Vec::new();

    for rec in rdr.records() {
        let rec = rec.context("bad CSV record")?;
        if rec.len() < 7 {
            continue;
        }
        let date = NaiveDate::parse_from_str(rec[0].trim(), "%d/%m/%Y")
            .map_err(|e| anyhow!("bad date '{}': {e}", &rec[0]))?;
        let title = rec[2].trim().to_string();
        let description_raw = rec[3].trim().to_string();
        let (entrada, _) = parse_amount(rec[4].trim())?;
        let (saida, _) = parse_amount(rec[5].trim())?;
        if entrada == 0 && saida == 0 {
            continue;
        }

        let hay = format!("{} {}", title.to_lowercase(), description_raw.to_lowercase());
        let (kind, amount_cents) = classify_c6_bank(&hay, entrada, saida);

        let description = if title.is_empty() {
            description_raw.clone()
        } else {
            title.clone()
        };
        let merchant = if title.eq_ignore_ascii_case("debito de cartao") {
            Some(description_raw)
        } else {
            None
        };

        out.push(ParsedItem {
            occurred_on: date,
            description,
            merchant,
            amount_cents,
            kind,
            category: None,
            installment: None,
            installment_count: None,
            tags: vec![],
        });
    }
    Ok(out)
}

fn classify_c6_bank(hay: &str, entrada: i64, saida: i64) -> (String, i64) {
    let amount_cents = if saida > 0 { -saida } else { entrada };
    let kind = if hay.contains("pgto fat cartao")
        || hay.contains("fatura de cartão")
        || hay.contains("fatura de cartao")
    {
        "card_payment"
    } else if hay.contains("debito de cartao") || hay.contains("débito de cartão") {
        "expense"
    } else if hay.contains("resgate de cdb")
        || hay.contains("emissao de cdb")
        || hay.contains("emissão de cdb")
        || hay.contains("ir res fundos")
    {
        "investment"
    } else if saida > 0 {
        "expense"
    } else {
        "income"
    };
    (kind.to_string(), amount_cents)
}

/// Parse a money amount that may use comma or dot decimals, with optional sign.
/// Returns absolute cents and whether the value was negative.
pub(crate) fn parse_amount(s: &str) -> Result<(i64, bool)> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let negative = cleaned.starts_with('-');
    let digits = cleaned.trim_start_matches('-').replace(',', ".");
    if digits.is_empty() {
        return Ok((0, negative));
    }
    let value: f64 = digits
        .parse()
        .map_err(|_| anyhow!("bad amount '{s}'"))?;
    let cents = (value * 100.0).round() as i64;
    Ok((cents, negative))
}

/// Parse `Parcela` like "7/10" into (7, 10); empty/unparseable -> (None, None).
fn parse_parcela(s: &str) -> (Option<i32>, Option<i32>) {
    if let Some((a, b)) = s.split_once('/') {
        let ai = a.trim().parse::<i32>().ok();
        let bi = b.trim().parse::<i32>().ok();
        (ai, bi)
    } else {
        (None, None)
    }
}
