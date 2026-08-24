//! Unit tests for the document parsers, using fake fixtures (no real data).

use chrono::NaiveDate;
use deepsave_backend::services::linking;
use deepsave_backend::services::parsers::{caixa_card, csv};
use deepsave_backend::services::sources;

mod common;

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(common::fixture(name)).unwrap()
}

// ---------- CSV parsers ----------

#[test]
fn c6_installments_use_billing_month() {
    let content = "Data de Compra;Nome;Final;Categoria;Descrição;Parcela;USD;Cotação;R$\n\
02/09/2025;X;1;Cat;COMPRA PARCELADA;7/10;0;0;100.00\n\
30/07/2026;X;1;Cat;COMPRA UNICA;Única;0;0;50.00\n";
    let billing = NaiveDate::from_ymd_opt(2026, 8, 1);
    let items = csv::parse_csv(content, billing).unwrap();

    let installment = items.iter().find(|i| i.installment_count == Some(10)).unwrap();
    assert_eq!(installment.occurred_on, billing.unwrap());
    let unica = items.iter().find(|i| i.installment_count.is_none()).unwrap();
    assert_eq!(unica.occurred_on, NaiveDate::from_ymd_opt(2026, 7, 30).unwrap());
}

#[test]
fn nubank_card_payment_detected() {
    let content = "Data,Valor,Identificador,Descrição\n\
15/08/2026,-31620.43,uuid,Pagamento Fatura QR CODE\n\
16/08/2026,-45.90,uuid,Coffee Shop\n";
    let items = csv::parse_csv(content, None).unwrap();
    let card = items.iter().find(|i| i.description.contains("Pagamento Fatura")).unwrap();
    assert_eq!(card.kind, "card_payment");
    let shop = items.iter().find(|i| i.description.contains("Coffee")).unwrap();
    assert_eq!(shop.kind, "expense");
}

#[test]
fn c6_bank_parses_metadata_and_kinds() {
    let content = "\u{feff}EXTRATO DE CONTA CORRENTE C6 BANK\n\nAgência: 1 / Conta: 123456\n\n\
Data Lançamento,Data Contábil,Título,Descrição,Entrada(R$),Saída(R$),Saldo do Dia(R$)\n\
23/09/2025,23/09/2025,IR RES FUNDOS ACOES,investment tax,0.00,627.95,34899.55\n\
24/09/2025,24/09/2025,Pix enviado para X,TRANSF ENVIADA PIX,0.00,20000.00,14899.55\n\
25/09/2025,25/09/2025,RESGATE DE CDB VENC,,12121.88,0.00,14907.43\n\
13/10/2025,13/10/2025,PGTO FAT CARTAO C6,Fatura de cartão,0.00,11229.42,38153.76\n\
10/10/2025,10/10/2025,DEBITO DE CARTAO,Coffee Shop Downtown,0.00,50.00,38300.92\n";
    let items = csv::parse_csv(content, None).unwrap();
    assert_eq!(items.len(), 5);

    let find = |needle: &str| items.iter().find(|i| i.description.to_lowercase().contains(needle)).unwrap();
    assert_eq!(find("ir res fundos").kind, "investment");
    assert_eq!(find("pix enviado").kind, "expense");
    assert_eq!(find("resgate de cdb").kind, "investment");
    assert_eq!(find("pgto fat cartao").kind, "card_payment");
    let debit = find("debito de cartao");
    assert_eq!(debit.kind, "expense");
    assert_eq!(debit.merchant.as_deref(), Some("Coffee Shop Downtown"));
}

#[test]
fn nubank_card_fixture_parses() {
    let items = csv::parse_csv(&read_fixture("nubank_card.csv"), None).unwrap();
    assert_eq!(items.len(), 5);
    let income = items.iter().find(|i| i.kind == "income").unwrap();
    assert_eq!(income.amount_cents, 24380);
    assert!(items.iter().any(|i| i.kind == "expense"));
}

#[test]
fn nubank_account_fixture_parses() {
    let items = csv::parse_csv(&read_fixture("nubank_account.csv"), None).unwrap();
    assert!(items.iter().any(|i| i.kind == "expense"));
    assert!(items.iter().any(|i| i.kind == "income"));
    assert!(items.iter().any(|i| i.kind == "refund"));
    assert!(items.iter().any(|i| i.kind == "card_payment"));

    let sent = items.iter().find(|i| i.description.contains("Person A")).unwrap();
    assert_eq!(sent.kind, "expense");
    assert_eq!(sent.merchant.as_deref(), Some("Person A"));

    let received = items.iter().find(|i| i.description.contains("Person C")).unwrap();
    assert_eq!(received.kind, "income");
}

#[test]
fn nubank_merchant_extraction_keeps_full_description() {
    let content = "Data,Valor,Identificador,Descrição\n\
15/08/2026,-100.00,uuid,Transferência enviada pelo Pix - TIM S A - 02.421.421/0001-11 - ITAÚ UNIBANCO S.A. (0341) Agência: 911 Conta: 20634-0\n";
    let items = csv::parse_csv(content, None).unwrap();
    assert_eq!(items[0].merchant.as_deref(), Some("TIM S A"));
    assert!(items[0].description.contains("Agência: 911"));
}

#[test]
fn c6_card_fixture_parses() {
    let billing = NaiveDate::from_ymd_opt(2026, 8, 1);
    let items = csv::parse_csv(&read_fixture("c6_card.csv"), billing).unwrap();
    assert_eq!(items.len(), 2);
    // "Única" item keeps its purchase date.
    let unica = items.iter().find(|i| i.description == "Grocery Store").unwrap();
    assert_eq!(unica.occurred_on, NaiveDate::from_ymd_opt(2026, 7, 30).unwrap());
    // C6 "Supermercados" maps to our "Supermercado".
    assert_eq!(unica.category.as_deref(), Some("Supermercado"));
    // Installment item gets the billing month.
    let parc = items.iter().find(|i| i.installment_count == Some(10)).unwrap();
    assert_eq!(parc.occurred_on, billing.unwrap());
    // Unknown C6 category ("Varejo") → no category.
    assert_eq!(parc.category, None);
}

#[test]
fn tags_are_normalized() {
    use deepsave_backend::services::tags;
    let out = tags::normalize(&[
        " Comida ".to_string(),
        "comida".to_string(),
        "Café".to_string(),
        "".to_string(),
        "mercado".to_string(),
    ]);
    assert_eq!(out, vec!["comida", "cafe", "mercado"]);
}

#[test]
fn c6_bank_fixture_parses() {
    let items = csv::parse_csv(&read_fixture("c6_bank.csv"), None).unwrap();
    assert_eq!(items.len(), 5);
    assert!(items.iter().any(|i| i.kind == "investment"));
    assert!(items.iter().any(|i| i.kind == "card_payment"));
    assert!(items.iter().any(|i| i.kind == "expense"));
}

// ---------- Caixa card PDF parser ----------

#[test]
fn parses_compras_table() {
    let text = "VENCIMENTO\n15/08/2026\n\nCOMPRAS (Cartão 5451)\n\n\
Data Descrição Cidade/País Valor U$$ Crédito/Débito\n\n19/07 IFD*iFood Osasco 12,90D\n\nTotal COMPRAS 12,90D";
    let (billing, items) = caixa_card::parse(text).unwrap();
    assert_eq!(billing, NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].description, "IFD*iFood Osasco");
    assert_eq!(items[0].amount_cents, -1290);
    assert_eq!(items[0].kind, "expense");
}

#[test]
fn infer_previous_year() {
    let billing = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    assert_eq!(caixa_card::infer_date("15/12", billing), NaiveDate::from_ymd_opt(2025, 12, 15));
}

#[test]
fn caixa_card_pdf_fixture_parses() {
    let text = pdf_extract::extract_text(common::fixture("caixa_card.pdf")).unwrap();
    assert!(caixa_card::is_caixa_card_fatura(&text));
    let (billing, items) = caixa_card::parse(&text).unwrap();
    assert_eq!(billing, NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());
    let ifood = items.iter().find(|i| i.description.to_lowercase().contains("ifood")).unwrap();
    assert_eq!(ifood.amount_cents, -1290);
}

// ---------- Linking heuristics ----------

#[test]
fn exact_merchant_match() {
    let m = Some("GIASSI SUPERMERCADOS".to_string());
    let d = NaiveDate::from_ymd_opt(2026, 7, 30).unwrap();
    assert_eq!(linking::match_confidence(&m, 58808, d, &m, -58808, d), Some(0.9));
}

#[test]
fn amount_too_small_does_not_match() {
    let m = Some("GIASSI SUPERMERCADOS".to_string());
    let d = NaiveDate::from_ymd_opt(2026, 7, 30).unwrap();
    assert_eq!(linking::match_confidence(&m, 99999, d, &m, -58808, d), None);
}

#[test]
fn different_merchant_does_not_match() {
    let d = NaiveDate::from_ymd_opt(2026, 7, 30).unwrap();
    let a = Some("LOJA A".to_string());
    let b = Some("LOJA B".to_string());
    assert_eq!(linking::match_confidence(&a, 58808, d, &b, -58808, d), None);
}

// ---------- Source detection ----------

#[test]
fn detects_banks_and_kinds() {
    assert_eq!(sources::detect_bank_kind("Data de Compra;Nome;Final;Categoria;Descrição;Parcela"), Some(("c6", "card_statement")));
    assert_eq!(sources::detect_bank_kind("EXTRATO DE CONTA CORRENTE C6 BANK\nData Lançamento,Data Contábil"), Some(("c6", "bank_statement")));
    assert_eq!(sources::detect_bank_kind("Data,Valor,Identificador,Descrição"), Some(("nubank", "bank_statement")));
    assert_eq!(sources::detect_bank_kind("date,title,amount"), Some(("nubank", "card_statement")));
    assert_eq!(sources::detect_bank_kind("Central de Atendimento Cartões Caixa\nCOMPRAS (Cartão 5451)"), Some(("caixa", "card_statement")));
    assert_eq!(sources::detect_bank_kind("Extrato por período\nConta: 03830"), Some(("caixa", "bank_statement")));
    assert_eq!(sources::detect_bank_kind("some random text"), None);
}
