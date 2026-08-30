-- Decommission merchant_memory: the change_log is now the single source of
-- truth for what the AI learns. Existing memory rows become legacy history
-- (source 'legacy') so no curated decision is lost; the table is then dropped.

INSERT INTO change_log (merchant_key, category_after, tags_after, source, created_at)
SELECT
  regexp_replace(merchant, '[^a-z0-9]', '', 'g'),  -- merchant is already accent-stripped + lowercase
  category_id,
  tags,
  'legacy',
  now()
FROM merchant_memory
WHERE category_id IS NOT NULL OR cardinality(tags) > 0;

DROP TABLE merchant_memory;
