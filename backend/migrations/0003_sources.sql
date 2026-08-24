-- Fundamental data sources (3 banks × 2 products) and document attribution.

CREATE TABLE IF NOT EXISTS sources (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  bank       text NOT NULL,              -- 'nubank' | 'c6' | 'caixa'
  kind       text NOT NULL,              -- 'bank_statement' | 'card_statement'
  name       text NOT NULL,
  enabled    boolean NOT NULL DEFAULT true,
  account_id uuid NULL REFERENCES accounts(id) ON DELETE SET NULL,
  sort_order integer NOT NULL DEFAULT 0,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (bank, kind)
);

ALTER TABLE documents
  ADD COLUMN IF NOT EXISTS source_id uuid NULL REFERENCES sources(id) ON DELETE SET NULL;

INSERT INTO sources (bank, kind, name, sort_order) VALUES
  ('nubank', 'bank_statement', 'Nubank — Conta',   1),
  ('nubank', 'card_statement', 'Nubank — Cartão',  2),
  ('c6',     'bank_statement', 'C6 — Conta',       3),
  ('c6',     'card_statement', 'C6 — Cartão',      4),
  ('caixa',  'bank_statement', 'Caixa — Conta',    5),
  ('caixa',  'card_statement', 'Caixa — Cartão',   6)
ON CONFLICT (bank, kind) DO NOTHING;

-- Backfill source attribution for documents uploaded before this migration.
UPDATE documents d SET source_id = s.id
FROM sources s
WHERE d.source_id IS NULL AND (
  (s.bank = 'nubank' AND s.kind = 'bank_statement' AND d.filename LIKE 'NU\_%')
  OR (s.bank = 'nubank' AND s.kind = 'card_statement' AND d.filename LIKE 'Nubank\_%')
  OR (s.bank = 'c6' AND s.kind = 'card_statement' AND d.filename LIKE 'Fatura\_%')
  OR (s.bank = 'caixa' AND s.kind = 'bank_statement' AND d.kind = 'bank_statement' AND d.filename LIKE 'comprovante%')
  OR (s.bank = 'caixa' AND s.kind = 'card_statement' AND d.kind = 'card_statement' AND d.filename LIKE 'comprovante%')
);
