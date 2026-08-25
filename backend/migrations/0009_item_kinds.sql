-- Item kinds are now exactly: expense, income, refund, internal.
-- `card_payment` and `investment` are folded into `internal` — they are
-- tracked but excluded from spend/income calculations, same as transfers
-- between the user's own accounts.
UPDATE items SET kind = 'internal' WHERE kind IN ('card_payment', 'investment');

-- Enforce the official kinds going forward.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'items_kind_check' AND conrelid = 'items'::regclass
  ) THEN
    ALTER TABLE items
      ADD CONSTRAINT items_kind_check CHECK (kind IN ('expense', 'income', 'refund', 'internal'));
  END IF;
END $$;
