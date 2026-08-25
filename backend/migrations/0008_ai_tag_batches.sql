-- AI-assisted bulk tagging: a batch of items selected by the user, processed in
-- the background by the DeepSeek worker. Suggestions are per-item and go through
-- human review (apply = add tags to the item, dismiss = reject).

CREATE TABLE IF NOT EXISTS ai_tag_batches (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  status        text NOT NULL DEFAULT 'pending', -- pending | processing | done | failed
  error_message text,
  created_at    timestamptz NOT NULL DEFAULT now(),
  processed_at  timestamptz
);

CREATE TABLE IF NOT EXISTS ai_tag_suggestions (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  batch_id       uuid NOT NULL REFERENCES ai_tag_batches(id) ON DELETE CASCADE,
  item_id        uuid NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  suggested_tags text[] NOT NULL DEFAULT '{}',
  status         text NOT NULL DEFAULT 'pending', -- pending | applied | dismissed
  created_at     timestamptz NOT NULL DEFAULT now(),
  UNIQUE (batch_id, item_id)
);

CREATE INDEX ai_tag_suggestions_batch_idx ON ai_tag_suggestions(batch_id);
CREATE INDEX ai_tag_suggestions_status_idx ON ai_tag_suggestions(status);
