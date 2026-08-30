-- Pluggy enrichment metadata on items — fed to the AI (tags/categorization) and
-- to the MCC → category rule. Populated at import (and backfilled via a forced
-- resync, since the import updates only these columns on conflict).

ALTER TABLE items ADD COLUMN pluggy_category text;   -- Pluggy's classifier category
ALTER TABLE items ADD COLUMN mcc integer;            -- card Merchant Category Code
ALTER TABLE items ADD COLUMN operation_type text;    -- Open Finance movement type (PIX, BOLETO, PORTABILIDADE_SALARIO…)
ALTER TABLE items ADD COLUMN payment_method text;    -- PIX | TED | DOC
CREATE INDEX items_mcc_idx ON items(mcc) WHERE mcc IS NOT NULL;
