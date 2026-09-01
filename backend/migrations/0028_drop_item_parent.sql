-- Dead-code cleanup: the sub-item (tree) concept is gone — receipt lines were
-- children of statement items, and documents are decommissioned. No item has
-- ever had a parent (0 rows), so the self-reference is inert schema.

ALTER TABLE items DROP COLUMN IF EXISTS parent_id;
