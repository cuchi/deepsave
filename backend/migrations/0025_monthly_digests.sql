-- Saved monthly digests: the AI summary is generated once (POST), stored,
-- and can be deleted (DELETE) / regenerated (POST again) at any time.

CREATE TABLE IF NOT EXISTS monthly_digests (
  month      date PRIMARY KEY,          -- first day of the month
  resumo     text NOT NULL,
  destaques  jsonb NOT NULL DEFAULT '[]',
  avisos     jsonb NOT NULL DEFAULT '[]',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);
