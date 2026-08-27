-- Rules no longer carry their own category; it's derived at read time from the
-- linked occurrences (like tags). The FK to categories drops with the column.
ALTER TABLE recurring_rules DROP COLUMN IF EXISTS category_id;
