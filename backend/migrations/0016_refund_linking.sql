-- Refund ↔ charge linking: a refund item points at the expense it reverses.
-- Graphs net linked refunds against their charge (bucketed by the charge's
-- month/category/merchant), so a fully-refunded expense shows as zero.

ALTER TABLE items ADD COLUMN refunded_item_id uuid REFERENCES items(id) ON DELETE SET NULL;
CREATE INDEX items_refunded_idx ON items(refunded_item_id);
