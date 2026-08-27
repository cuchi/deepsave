#!/usr/bin/env bash
set -euo pipefail

# Seed fake data into DeepSave's Postgres. Run from the host machine.
# The postgres container must be up:  docker compose up -d postgres
#
# Overridable via env vars, e.g.:
#   DEEPSEED_PG_CONTAINER=my-pg ./scripts/seed.sh

CONTAINER="${DEEPSEED_PG_CONTAINER:-deepsave-postgres}"
DB_USER="${DEEPSEED_PG_USER:-deepsave}"
DB_NAME="${DEEPSEED_PG_DB:-deepsave}"

echo "Seeding fake data into container '$CONTAINER' (db '$DB_NAME')..."

docker exec -i "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -v ON_ERROR_STOP=1 <<'SQL'
BEGIN;

-- Reset data tables (keep schema + migrations).
TRUNCATE TABLE matches, merchant_memory, items, recurring_rules, recurring_aliases, documents, ai_calls, accounts, categories CASCADE;

-- ===== Categories =====
INSERT INTO categories (id, name, color) VALUES
  ('00000000-0000-0000-0000-000000000101', 'Supermercado', '#34d399'),
  ('00000000-0000-0000-0000-000000000102', 'Transporte',    '#60a5fa'),
  ('00000000-0000-0000-0000-000000000103', 'Restaurantes',  '#f472b6'),
  ('00000000-0000-0000-0000-000000000104', 'Saúde',         '#f87171'),
  ('00000000-0000-0000-0000-000000000105', 'Moradia',       '#fbbf24'),
  ('00000000-0000-0000-0000-000000000106', 'Lazer',         '#a78bfa'),
  ('00000000-0000-0000-0000-000000000107', 'Assinaturas',   '#22d3ee'),
  ('00000000-0000-0000-0000-000000000108', 'Outros',        '#9ca3af');

-- ===== Own accounts =====
INSERT INTO accounts (id, name, bank, account_number) VALUES
  ('00000000-0000-0000-0000-000000000201', 'Nubank', 'Nubank', '1234-5'),
  ('00000000-0000-0000-0000-000000000202', 'Caixa',  'Caixa Econômica Federal', '5678-9'),
  ('00000000-0000-0000-0000-000000000203', 'C6',     'C6 Bank', '9012-3');

-- ===== Recurring rules (name is a free-form label; matching is via aliases) =====
INSERT INTO recurring_rules (id, name, amount_cents, currency, frequency, interval, day_of_month, next_due_on, is_active, source) VALUES
  ('00000000-0000-0000-0000-000000000401', 'Spotify Premium', -2190, 'BRL', 'monthly', 1, 15, (date_trunc('month', now()) + interval '1 month' + interval '14 days')::date, true, 'manual'),
  ('00000000-0000-0000-0000-000000000402', 'Netflix', -5590, 'BRL', 'monthly', 1, 20, (date_trunc('month', now()) + interval '1 month' + interval '19 days')::date, true, 'manual'),
  ('00000000-0000-0000-0000-000000000403', 'Academia Smart Fit', -9990, 'BRL', 'monthly', 1, 5, (date_trunc('month', now()) + interval '1 month' + interval '4 days')::date, true, 'manual');

-- ===== Recurring aliases (normalized, auto-match) =====
INSERT INTO recurring_aliases (rule_id, name, is_alias) VALUES
  ('00000000-0000-0000-0000-000000000401', 'spotify', true),
  ('00000000-0000-0000-0000-000000000402', 'netflix', true),
  ('00000000-0000-0000-0000-000000000403', 'academia smart fit', true);

-- ===== Items (tree) =====
-- Columns: id, parent_id, source, kind, status, account_id, transfer_group_id,
--          installment, installment_count, recurring_id, occurred_on, merchant,
--          description, amount_cents, category_id, tags
-- (negative amount = expense, positive = income/transfer_in)

INSERT INTO items (id, parent_id, source, kind, status, account_id, transfer_group_id, installment, installment_count, recurring_id, occurred_on, merchant, description, amount_cents, category_id, tags) VALUES

  -- ---- Current month ----
  ('00000000-0000-0000-0000-000000000301', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', NULL, NULL, NULL, NULL, now()::date - 2, 'Supermercado Bom Preço', 'Compras do mês', -32050, '00000000-0000-0000-0000-000000000101', ARRAY['mercado']),
  ('00000000-0000-0000-0000-000000000302', '00000000-0000-0000-0000-000000000301', 'receipt', 'expense', 'confirmed', NULL, NULL, NULL, NULL, NULL, now()::date - 2, 'Supermercado Bom Preço', 'Alimentos', -18000, '00000000-0000-0000-0000-000000000101', ARRAY['mercado']),
  ('00000000-0000-0000-0000-000000000303', '00000000-0000-0000-0000-000000000301', 'receipt', 'expense', 'confirmed', NULL, NULL, NULL, NULL, NULL, now()::date - 2, 'Supermercado Bom Preço', 'Limpeza', -8000, '00000000-0000-0000-0000-000000000101', ARRAY['casa']),
  ('00000000-0000-0000-0000-000000000304', '00000000-0000-0000-0000-000000000301', 'receipt', 'expense', 'confirmed', NULL, NULL, NULL, NULL, NULL, now()::date - 2, 'Supermercado Bom Preço', 'Bebidas', -6050, '00000000-0000-0000-0000-000000000101', ARRAY['bebidas']),
  ('00000000-0000-0000-0000-000000000305', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', NULL, NULL, NULL, NULL, now()::date - 4, 'Posto Shell', 'Combustível', -15000, '00000000-0000-0000-0000-000000000102', ARRAY['combustível']),
  ('00000000-0000-0000-0000-000000000306', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000203', NULL, NULL, NULL, NULL, now()::date - 5, 'iFood', 'Jantar pedido', -4590, '00000000-0000-0000-0000-000000000103', ARRAY['delivery']),
  ('00000000-0000-0000-0000-000000000307', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', NULL, NULL, NULL, '00000000-0000-0000-0000-000000000401', now()::date - 6, 'Spotify', 'Spotify Premium', -2190, '00000000-0000-0000-0000-000000000107', ARRAY['streaming']),
  ('00000000-0000-0000-0000-000000000308', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000203', NULL, NULL, NULL, '00000000-0000-0000-0000-000000000403', now()::date - 7, 'Academia Smart Fit', 'Mensalidade', -9990, '00000000-0000-0000-0000-000000000104', ARRAY['academia']),
  ('00000000-0000-0000-0000-000000000309', NULL, 'bank_statement', 'income', 'confirmed', '00000000-0000-0000-0000-000000000202', NULL, NULL, NULL, NULL, now()::date - 3, NULL, 'Salário', 850000, NULL, ARRAY['salário']),
  ('00000000-0000-0000-0000-000000000310', NULL, 'bank_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', '00000000-0000-0000-0000-000000000501', NULL, NULL, NULL, now()::date - 8, NULL, 'Transferência para Caixa', -50000, NULL, ARRAY['transferência']),
  ('00000000-0000-0000-0000-000000000311', NULL, 'bank_statement', 'income', 'confirmed', '00000000-0000-0000-0000-000000000202', '00000000-0000-0000-0000-000000000501', NULL, NULL, NULL, now()::date - 8, NULL, 'Transferência do Nubank', 50000, NULL, ARRAY['transferência']),

  -- ---- Previous month ----
  ('00000000-0000-0000-0000-000000000312', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '1 month' + interval '6 days', 'Supermercado Bom Preço', 'Compras do mês', -41230, '00000000-0000-0000-0000-000000000101', ARRAY['mercado']),
  ('00000000-0000-0000-0000-000000000313', '00000000-0000-0000-0000-000000000312', 'receipt', 'expense', 'confirmed', NULL, NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '1 month' + interval '6 days', 'Supermercado Bom Preço', 'Alimentos', -25000, '00000000-0000-0000-0000-000000000101', ARRAY['mercado']),
  ('00000000-0000-0000-0000-000000000314', '00000000-0000-0000-0000-000000000312', 'receipt', 'expense', 'confirmed', NULL, NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '1 month' + interval '6 days', 'Supermercado Bom Preço', 'Padaria', -6230, '00000000-0000-0000-0000-000000000101', ARRAY['padaria']),
  ('00000000-0000-0000-0000-000000000315', '00000000-0000-0000-0000-000000000312', 'receipt', 'expense', 'confirmed', NULL, NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '1 month' + interval '6 days', 'Supermercado Bom Preço', 'Limpeza', -10000, '00000000-0000-0000-0000-000000000101', ARRAY['casa']),
  ('00000000-0000-0000-0000-000000000316', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '1 month' + interval '12 days', 'Uber', 'Corrida', -6780, '00000000-0000-0000-0000-000000000102', ARRAY['transporte']),
  ('00000000-0000-0000-0000-000000000317', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', NULL, NULL, NULL, '00000000-0000-0000-0000-000000000402', date_trunc('month', now()) - interval '1 month' + interval '20 days', 'Netflix', 'Netflix', -5590, '00000000-0000-0000-0000-000000000106', ARRAY['streaming']),
  ('00000000-0000-0000-0000-000000000318', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000203', NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '1 month' + interval '15 days', 'Droga Raia', 'Farmácia', -12040, '00000000-0000-0000-0000-000000000104', ARRAY['farmácia']),
  ('00000000-0000-0000-0000-000000000319', NULL, 'bank_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', '00000000-0000-0000-0000-000000000502', NULL, NULL, NULL, date_trunc('month', now()) - interval '1 month' + interval '25 days', NULL, 'Transferência para C6', -30000, NULL, ARRAY['transferência']),
  ('00000000-0000-0000-0000-000000000320', NULL, 'bank_statement', 'income', 'confirmed', '00000000-0000-0000-0000-000000000203', '00000000-0000-0000-0000-000000000502', NULL, NULL, NULL, date_trunc('month', now()) - interval '1 month' + interval '25 days', NULL, 'Transferência do Nubank', 30000, NULL, ARRAY['transferência']),

  -- ---- Two months ago ----
  ('00000000-0000-0000-0000-000000000321', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '2 months' + interval '8 days', 'Supermercado Bom Preço', 'Compras do mês', -38500, '00000000-0000-0000-0000-000000000101', ARRAY['mercado']),
  ('00000000-0000-0000-0000-000000000322', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000201', NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '2 months' + interval '14 days', 'Posto Shell', 'Combustível', -20000, '00000000-0000-0000-0000-000000000102', ARRAY['combustível']),
  ('00000000-0000-0000-0000-000000000323', NULL, 'card_statement', 'expense', 'confirmed', '00000000-0000-0000-0000-000000000203', NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '2 months' + interval '10 days', 'Cantina do Zé', 'Almoço', -8900, '00000000-0000-0000-0000-000000000103', ARRAY['almoço']),
  ('00000000-0000-0000-0000-000000000324', NULL, 'bank_statement', 'income', 'confirmed', '00000000-0000-0000-0000-000000000202', NULL, NULL, NULL, NULL, date_trunc('month', now()) - interval '2 months' + interval '3 days', NULL, 'Salário', 850000, NULL, ARRAY['salário']);

-- ===== Categorization memory =====
INSERT INTO merchant_memory (merchant, category_id, tags, confidence, confirm_count, last_confirmed_at) VALUES
  ('supermercado bom preço', '00000000-0000-0000-0000-000000000101', ARRAY['mercado'], 0.95, 4, now()),
  ('posto shell', '00000000-0000-0000-0000-000000000102', ARRAY['combustível'], 0.85, 3, now()),
  ('ifood', '00000000-0000-0000-0000-000000000103', ARRAY['delivery'], 0.9, 5, now()),
  ('droga raia', '00000000-0000-0000-0000-000000000104', ARRAY['farmácia'], 0.8, 2, now());

COMMIT;

-- ===== Summary =====
SELECT 'categories' AS entity, count(*) FROM categories
UNION ALL SELECT 'accounts', count(*) FROM accounts
UNION ALL SELECT 'items', count(*) FROM items
UNION ALL SELECT 'recurring_rules', count(*) FROM recurring_rules
UNION ALL SELECT 'merchant_memory', count(*) FROM merchant_memory;
SQL

echo "Done."
