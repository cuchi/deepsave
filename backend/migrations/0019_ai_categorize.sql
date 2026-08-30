-- Generalize the AI batch/suggestion tables for categorization (F2): batches
-- carry a `kind` ('tags' | 'categorize'); suggestions may carry a category
-- proposal alongside (or instead of) tags.

ALTER TABLE ai_tag_batches ADD COLUMN kind text NOT NULL DEFAULT 'tags';
ALTER TABLE ai_tag_suggestions ADD COLUMN suggested_category text NOT NULL DEFAULT '';
