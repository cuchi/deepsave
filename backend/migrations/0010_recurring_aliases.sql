-- 0010: recurring rules revamp
-- (PLAN-recurring.md) name is a free-form label; matching is done purely via
-- recurring_aliases (auto aliases + one-shot isolated cases); rule tags are
-- derived from linked items; items gain a manual-link marker.

-- 1. description -> name (free-form label, NOT NULL preserved by the rename).
ALTER TABLE recurring_rules RENAME COLUMN description TO name;

-- 2. Drop unused columns: merchant (never used for matching, referenced nowhere)
--    and tags (now derived from linked items).
ALTER TABLE recurring_rules DROP COLUMN IF EXISTS merchant;
ALTER TABLE recurring_rules DROP COLUMN IF EXISTS tags;

-- 3. Name entries: aliases (auto-match, globally unique) and isolated cases
--    (one-shot manual references, may repeat across rules).
CREATE TABLE recurring_aliases (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  rule_id    uuid NOT NULL REFERENCES recurring_rules(id) ON DELETE CASCADE,
  name       text NOT NULL,                        -- stored normalized
  is_alias   boolean NOT NULL DEFAULT true,        -- true = auto-match, false = isolated case
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (rule_id, name)
);
-- DB-level guarantee: auto aliases are unique across rules.
CREATE UNIQUE INDEX recurring_aliases_alias_uniq
  ON recurring_aliases (name) WHERE is_alias;

-- 4. Manual links (user-picked) are never touched by auto-relink.
ALTER TABLE items ADD COLUMN IF NOT EXISTS linked_manually boolean NOT NULL DEFAULT false;
