-- Change log: durable, append-only record of every category/tag change the user
-- makes (item edit, bulk, AI apply, memory apply, confirm). The AI uses it to
-- follow the user's latest decisions and detect instability.

CREATE TABLE IF NOT EXISTS change_log (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  item_id         uuid REFERENCES items(id) ON DELETE SET NULL,
  merchant_key    text NOT NULL,          -- normalized identity (merchant or description)
  category_before uuid,
  category_after  uuid,
  tags_before     text[] NOT NULL DEFAULT '{}',
  tags_after      text[] NOT NULL DEFAULT '{}',
  source          text NOT NULL,          -- item_edit | bulk | memory_apply | ai_apply | confirm
  created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX change_log_key_idx ON change_log(merchant_key, created_at DESC);
CREATE INDEX change_log_time_idx ON change_log(created_at DESC);
