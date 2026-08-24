-- Track AI-proposed categories that didn't match the existing tree.
ALTER TABLE items ADD COLUMN IF NOT EXISTS suggested_category text;
