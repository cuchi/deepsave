-- Full schema (PLAN.md §6). `users` already exists from 0001_init.sql.

CREATE TABLE IF NOT EXISTS accounts (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  name           text NOT NULL,
  bank           text,
  account_number text,
  created_at     timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS categories (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  parent_id  uuid REFERENCES categories(id) ON DELETE SET NULL,
  name       text NOT NULL UNIQUE,
  color      text,
  icon       text,
  is_active  boolean NOT NULL DEFAULT true
);

CREATE TABLE IF NOT EXISTS recurring_rules (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  merchant      text,
  description   text NOT NULL,
  amount_cents  bigint NOT NULL,
  currency      varchar(3) NOT NULL DEFAULT 'BRL',
  category_id   uuid REFERENCES categories(id) ON DELETE SET NULL,
  tags          text[] NOT NULL DEFAULT '{}',
  frequency     text NOT NULL,
  interval      integer NOT NULL DEFAULT 1,
  day_of_month  integer,
  next_due_on   date,
  is_active     boolean NOT NULL DEFAULT true,
  source        text NOT NULL DEFAULT 'manual',
  created_at    timestamptz NOT NULL DEFAULT now(),
  updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS documents (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  kind          text NOT NULL,
  account_id    uuid REFERENCES accounts(id) ON DELETE SET NULL,
  filename      text NOT NULL,
  content_type  text NOT NULL,
  sha256        text NOT NULL UNIQUE,
  file_path     text NOT NULL,
  status        text NOT NULL,
  error_message text,
  ocr_text      text,
  ai_payload    jsonb,
  uploaded_at   timestamptz NOT NULL DEFAULT now(),
  processed_at  timestamptz
);

CREATE TABLE IF NOT EXISTS ai_calls (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  document_id       uuid REFERENCES documents(id) ON DELETE SET NULL,
  purpose           text NOT NULL,
  model             text NOT NULL,
  prompt_tokens     integer NOT NULL,
  completion_tokens integer NOT NULL,
  total_tokens      integer NOT NULL,
  cache_hit_tokens  integer NOT NULL DEFAULT 0,
  cache_miss_tokens integer NOT NULL DEFAULT 0,
  cost_usd          numeric(12,6) NOT NULL DEFAULT 0,
  duration_ms       integer,
  status            text NOT NULL,
  error_message     text,
  created_at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ai_calls_doc_idx  ON ai_calls(document_id);
CREATE INDEX ai_calls_date_idx ON ai_calls(created_at);

CREATE TABLE IF NOT EXISTS items (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  parent_id         uuid REFERENCES items(id) ON DELETE CASCADE,
  document_id       uuid REFERENCES documents(id) ON DELETE SET NULL,
  source            text NOT NULL,
  kind              text NOT NULL DEFAULT 'expense',
  status            text NOT NULL,
  account_id        uuid REFERENCES accounts(id) ON DELETE SET NULL,
  transfer_group_id uuid,
  installment       integer,
  installment_count integer,
  recurring_id      uuid REFERENCES recurring_rules(id) ON DELETE SET NULL,
  occurred_on       date NOT NULL,
  posted_on         date,
  merchant          text,
  description       text NOT NULL,
  amount_cents      bigint NOT NULL,
  currency          varchar(3) NOT NULL DEFAULT 'BRL',
  category_id       uuid REFERENCES categories(id) ON DELETE SET NULL,
  tags              text[] NOT NULL DEFAULT '{}',
  raw_line          text,
  match_confidence  real,
  created_at        timestamptz NOT NULL DEFAULT now(),
  updated_at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX items_parent_idx   ON items(parent_id);
CREATE INDEX items_month_idx    ON items(occurred_on, status);
CREATE INDEX items_doc_idx      ON items(document_id);
CREATE INDEX items_kind_idx     ON items(kind);
CREATE INDEX items_account_idx  ON items(account_id);
CREATE INDEX items_transfer_idx ON items(transfer_group_id);

CREATE TABLE IF NOT EXISTS matches (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  parent_item_id uuid NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  child_item_id  uuid NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  source         text NOT NULL,
  confidence     real NOT NULL,
  status         text NOT NULL,
  created_at     timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS merchant_memory (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  merchant          text NOT NULL UNIQUE,
  category_id       uuid REFERENCES categories(id) ON DELETE SET NULL,
  tags              text[] NOT NULL DEFAULT '{}',
  confidence        real NOT NULL DEFAULT 0,
  confirm_count     integer NOT NULL DEFAULT 0,
  last_confirmed_at timestamptz,
  created_at        timestamptz NOT NULL DEFAULT now(),
  updated_at        timestamptz NOT NULL DEFAULT now()
);
