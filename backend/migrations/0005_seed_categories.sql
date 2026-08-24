-- Seed a starter category set (idempotent).
INSERT INTO categories (name, color) VALUES
  ('Supermercado',  '#34d399'),
  ('Transporte',    '#60a5fa'),
  ('Restaurantes',  '#f472b6'),
  ('Saúde',         '#f87171'),
  ('Moradia',       '#fbbf24'),
  ('Lazer',         '#a78bfa'),
  ('Assinaturas',   '#22d3ee'),
  ('Outros',        '#9ca3af')
ON CONFLICT (name) DO NOTHING;
