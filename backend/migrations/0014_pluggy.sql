-- Pluggy (open-banking aggregation) integration.
-- Items connected via Pluggy replace statement documents: transactions are
-- pulled directly from the bank API and imported as confirmed items.

CREATE TABLE IF NOT EXISTS pluggy_items (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  pluggy_id       text NOT NULL UNIQUE,        -- Pluggy item id
  connector_id    integer,
  connector_name  text,
  status          text NOT NULL DEFAULT 'CREATED',
  error           jsonb,                        -- last Pluggy item error payload
  last_sync_at    timestamptz,
  created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS pluggy_accounts (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  pluggy_account_id text NOT NULL UNIQUE,       -- Pluggy account id
  pluggy_item_id    uuid NOT NULL REFERENCES pluggy_items(id) ON DELETE CASCADE,
  account_id        uuid REFERENCES accounts(id) ON DELETE SET NULL,
  name              text NOT NULL,
  account_type      text,                       -- 'BANK' | 'CREDIT' | 'INVESTMENT' | ...
  subtype           text,                       -- 'CHECKING_ACCOUNT' | 'CREDIT_CARD' | ...
  currency          text NOT NULL DEFAULT 'BRL',
  balance           numeric(14,2),
  credit_limit      numeric(14,2),
  due_date          date,                       -- card: fatura due date
  close_date        date,                       -- card: fatura closing date
  last_sync_at      timestamptz,
  created_at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX pluggy_accounts_item_idx ON pluggy_accounts(pluggy_item_id);

-- Idempotency key for imports: Pluggy's transaction id.
ALTER TABLE items ADD COLUMN external_id text;
CREATE UNIQUE INDEX items_external_id_idx ON items(external_id) WHERE external_id IS NOT NULL;
