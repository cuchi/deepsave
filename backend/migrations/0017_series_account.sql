-- Purchase series for Pluggy installments: the old series were reconstructed
-- from documents (source_id); Pluggy items carry installment fields but no
-- series. Assign them to series keyed by account + merchant tokens + count.

ALTER TABLE purchase_series ADD COLUMN account_id uuid REFERENCES accounts(id) ON DELETE SET NULL;
CREATE INDEX purchase_series_account_idx ON purchase_series(account_id);
