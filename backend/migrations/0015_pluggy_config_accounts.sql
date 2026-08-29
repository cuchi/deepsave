-- Config-driven accounts: we no longer create Pluggy items from the app.
-- Account ids come from the Pluggy dashboard (stable) and are configured in
-- .env (PLUGGY_ACCOUNTS), so the item dependency is gone.

ALTER TABLE pluggy_accounts ALTER COLUMN pluggy_item_id DROP NOT NULL;

-- Bank slug ('nubank' | 'caixa' | 'c6') for labels/grouping.
ALTER TABLE pluggy_accounts ADD COLUMN IF NOT EXISTS bank text;
