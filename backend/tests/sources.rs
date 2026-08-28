//! Tests for bank-statement period extraction (partial-month detection).

use deepsave_backend::services::sources::extract_statement_period;
use chrono::NaiveDate;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn nubank_filename_full_month() {
    let (s, e) = extract_statement_period(
        "NU_32530067_01JAN2026_31JAN2026.csv",
        "Data,Valor,Identificador,Descrição\n05/01/2026,-100,abc,Pix",
    )
    .unwrap();
    assert_eq!((s, e), (d("2026-01-01"), d("2026-01-31")));
}

#[test]
fn nubank_filename_partial_month() {
    let (s, e) = extract_statement_period(
        "NU_32530067_01AGO2026_22AGO2026.csv",
        "Data,Valor,Identificador,Descrição",
    )
    .unwrap();
    assert_eq!((s, e), (d("2026-08-01"), d("2026-08-22")));
}

#[test]
fn c6_header_range() {
    let header = "EXTRATO DE CONTA CORRENTE C6 BANK\n\nAgência: 1 / Conta: 248944070\n\
                  Extrato gerado em 24/08/2026 - as 08:13:47\n\nExtrato de 24/08/2025 a 24/08/2026\n";
    let (s, e) = extract_statement_period("random-name.csv", header).unwrap();
    assert_eq!((s, e), (d("2025-08-24"), d("2026-08-24")));
}

#[test]
fn caixa_style_header_range() {
    let header = "Extrato por período\nPeríodo: 01/01/2026 a 31/01/2026\n";
    let (s, e) = extract_statement_period("caixa.pdf", header).unwrap();
    assert_eq!((s, e), (d("2026-01-01"), d("2026-01-31")));
}

#[test]
fn no_period_returns_none() {
    assert!(extract_statement_period("Fatura_2026-08-15.csv", "Data de compra;Nome").is_none());
    assert!(extract_statement_period("Nubank_2026-08-15.csv", "title,amount").is_none());
}
