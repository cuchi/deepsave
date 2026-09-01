-- Dead-code cleanup: drop tables no code reads.
--   matches  — receipt→statement suggestion feature (documents decommissioned;
--              the accept-suggestion endpoint was removed).
--   users    — single-user auth uses APP_PASSWORD_HASH from env, not this table.

DROP TABLE IF EXISTS matches;
DROP TABLE IF EXISTS users;
