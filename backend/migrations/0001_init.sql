-- Baseline schema. Users table only for now (M0);
-- the full schema from PLAN.md §6 lands in later milestones.

CREATE TABLE IF NOT EXISTS users (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  password_hash text NOT NULL,
  created_at    timestamptz NOT NULL DEFAULT now()
);
