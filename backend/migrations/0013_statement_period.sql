-- Statement period for bank statements (conta corrente), used to detect
-- partial months in coverage. Faturas (card statements) are always complete.
-- - Nubank encodes the period in the filename (NU_..._01JAN2026_31JAN2026.csv)
-- - C6/Caixa include it in the header ("Extrato de 24/08/2025 a 24/08/2026")

ALTER TABLE documents ADD COLUMN statement_start date;
ALTER TABLE documents ADD COLUMN statement_end date;

CREATE INDEX documents_statement_idx ON documents(source_id, statement_start);
