-- Purchase series: link the parcels of one installment purchase across faturas.
-- Reconstructed from the source documents (each fatura carries one parcel per
-- series; purchase date + monthly cadence disambiguate identical purchases).

CREATE TABLE IF NOT EXISTS purchase_series (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  source_id         uuid REFERENCES sources(id) ON DELETE SET NULL,
  description       text NOT NULL,
  installment_count integer NOT NULL,
  -- Original purchase date (from the document), when known — stable across the
  -- series and the tiebreaker for identical purchases from the same merchant.
  purchase_date     date,
  created_at        timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE items ADD COLUMN series_id uuid REFERENCES purchase_series(id) ON DELETE SET NULL;

CREATE INDEX items_series_idx ON items(series_id);
CREATE INDEX purchase_series_key_idx ON purchase_series(source_id, description, installment_count);
