-- Snapshot the transaction date on each change-log entry (self-contained
-- history; the item's occurred_on may change later).

ALTER TABLE change_log ADD COLUMN tx_date date;
