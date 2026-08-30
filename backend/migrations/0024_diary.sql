-- Diary: user life notes (date + comment) that enrich the AI agents —
-- e.g. "2025-09-01 - I got divorced" explains months of spending patterns.

CREATE TABLE IF NOT EXISTS diary_entries (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entry_date date NOT NULL,
  comment    text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX diary_entries_date_idx ON diary_entries(entry_date DESC);
